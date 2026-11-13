//! A failure's message arguments as `operations.error_args` keeps them.
//!
//! `api::MsgArg` is the wire type: the API may rename a variant or reshape one
//! when its clients need it. A database outlives such a change, so the column
//! has a type of its own, spelled out here, and the two are mapped at this
//! edge. Writing matches every variant of `MsgArg`, so a new one doesn't
//! compile until it has a spelling here; reading builds a `MsgArg` from what
//! this module wrote, whatever `api` looks like by then.
//!
//! The column is a JSON array of `[name, argument]` pairs, `[]` for none. An
//! argument is `{"type": …, "value": …}`: `count`, `bytes` and `duration-ms`
//! hold a number, `text` a string and `timestamp` an object with the
//! milliseconds since the Unix epoch, `utc_ms`, and, when there is one, the
//! `zone` it is read in.
//!
//! Reading ignores fields it doesn't know, so a newer version may add one; a
//! `type` this version never wrote, or a value of the wrong shape, is an error.

use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};

use api::{IanaZone, MsgArg};
use serde::{Deserialize, Serialize, Serializer};

use crate::report::shown;

/// Why a stored list of arguments can't be read.
#[derive(Debug)]
pub(crate) enum ArgsError {
    /// Not a JSON list of `[name, argument]` pairs of arguments this version
    /// writes.
    NotPairs(serde_json::Error),
    /// A timestamp is read in a zone that isn't shaped like a time-zone name.
    BadZone { name: String },
}

impl Display for ArgsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPairs(error) => write!(f, "is not a list of pairs: {error}"),
            Self::BadZone { name } => write!(
                f,
                "has a timestamp in {}, which is not a time-zone name",
                shown(name)
            ),
        }
    }
}

/// One argument as the column spells it. What each variant stores is its
/// `rename`, a spelling of its own that doesn't change with the variant's
/// name, with `MsgArg`'s or with anything else outside this module.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
enum StoredArg<'a> {
    #[serde(rename = "count")]
    Count(u64),
    #[serde(rename = "bytes")]
    Bytes(u64),
    #[serde(rename = "duration-ms")]
    DurationMs(u64),
    #[serde(rename = "timestamp")]
    Timestamp {
        utc_ms: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<Cow<'a, str>>,
    },
    #[serde(rename = "text")]
    Text(Cow<'a, str>),
}

impl<'a> From<&'a MsgArg> for StoredArg<'a> {
    /// Names every variant, so a new `MsgArg` doesn't compile until it has a
    /// spelling here.
    fn from(arg: &'a MsgArg) -> Self {
        match arg {
            MsgArg::Count(count) => Self::Count(*count),
            MsgArg::Bytes(bytes) => Self::Bytes(*bytes),
            MsgArg::DurationMs(millis) => Self::DurationMs(*millis),
            MsgArg::Timestamp { utc_ms, zone } => Self::Timestamp {
                utc_ms: *utc_ms,
                zone: zone.as_ref().map(|zone| Cow::Borrowed(zone.as_str())),
            },
            MsgArg::Text(text) => Self::Text(Cow::Borrowed(text)),
        }
    }
}

impl TryFrom<StoredArg<'_>> for MsgArg {
    type Error = ArgsError;

    fn try_from(arg: StoredArg<'_>) -> Result<Self, ArgsError> {
        Ok(match arg {
            StoredArg::Count(count) => Self::Count(count),
            StoredArg::Bytes(bytes) => Self::Bytes(bytes),
            StoredArg::DurationMs(millis) => Self::DurationMs(millis),
            StoredArg::Timestamp { utc_ms, zone } => Self::Timestamp {
                utc_ms,
                zone: zone
                    .map(|name| {
                        IanaZone::new(&*name).map_err(|_| ArgsError::BadZone {
                            name: name.into_owned(),
                        })
                    })
                    .transpose()?,
            },
            StoredArg::Text(text) => Self::Text(text.into_owned()),
        })
    }
}

/// Arguments written as the column's list of pairs, without a copy of them.
struct Pairs<'a>(&'a [(&'a str, MsgArg)]);

impl Serialize for Pairs<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .iter()
                .map(|(name, arg)| (name, StoredArg::from(arg))),
        )
    }
}

/// The column's text for `args`, in order.
pub(crate) fn encode(args: &[(&str, MsgArg)]) -> Result<String, serde_json::Error> {
    serde_json::to_string(&Pairs(args))
}

/// The arguments the column's `text` holds, in order.
pub(crate) fn decode(text: &str) -> Result<Vec<(String, MsgArg)>, ArgsError> {
    let stored: Vec<(String, StoredArg<'_>)> =
        serde_json::from_str(text).map_err(ArgsError::NotPairs)?;
    stored
        .into_iter()
        .map(|(name, arg)| Ok((name, MsgArg::try_from(arg)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use api::IanaZone;

    use super::*;

    fn zone(name: &str) -> IanaZone {
        IanaZone::new(name).expect("a zone name")
    }

    /// Every kind of argument, with the exact text the column holds for it.
    /// These are the format: a change to one is a change to what rows already
    /// written mean.
    fn spellings() -> Vec<(MsgArg, &'static str)> {
        vec![
            (MsgArg::Count(7), r#"{"type":"count","value":7}"#),
            (
                MsgArg::Count(u64::MAX),
                r#"{"type":"count","value":18446744073709551615}"#,
            ),
            (
                MsgArg::Bytes(1_048_576),
                r#"{"type":"bytes","value":1048576}"#,
            ),
            (
                MsgArg::DurationMs(90_000),
                r#"{"type":"duration-ms","value":90000}"#,
            ),
            (
                MsgArg::Timestamp {
                    utc_ms: 1_800_000_000_000,
                    zone: None,
                },
                r#"{"type":"timestamp","value":{"utc_ms":1800000000000}}"#,
            ),
            (
                MsgArg::Timestamp {
                    utc_ms: -1,
                    zone: Some(zone("Europe/Berlin")),
                },
                r#"{"type":"timestamp","value":{"utc_ms":-1,"zone":"Europe/Berlin"}}"#,
            ),
            (
                MsgArg::Timestamp {
                    utc_ms: i64::MIN,
                    zone: Some(zone("Etc/GMT+5")),
                },
                r#"{"type":"timestamp","value":{"utc_ms":-9223372036854775808,"zone":"Etc/GMT+5"}}"#,
            ),
            (
                MsgArg::Text("3".to_owned()),
                r#"{"type":"text","value":"3"}"#,
            ),
            (MsgArg::Text(String::new()), r#"{"type":"text","value":""}"#),
            (
                MsgArg::Text("a \"quote\", a\ttab and é😀".to_owned()),
                r#"{"type":"text","value":"a \"quote\", a\ttab and é😀"}"#,
            ),
        ]
    }

    fn written(args: &[(&str, MsgArg)]) -> String {
        encode(args).expect("arguments are written")
    }

    fn read(text: &str) -> Vec<(String, MsgArg)> {
        decode(text).unwrap_or_else(|error| panic!("{text} is read: {error}"))
    }

    fn pair(name: &str, arg: MsgArg) -> (String, MsgArg) {
        (name.to_owned(), arg)
    }

    #[test]
    fn every_kind_of_argument_is_written_as_the_format_spells_it() {
        for (arg, spelled) in spellings() {
            assert_eq!(
                written(&[("name", arg.clone())]),
                format!(r#"[["name",{spelled}]]"#),
                "{arg:?}"
            );
        }
    }

    #[test]
    fn every_kind_of_argument_is_read_from_the_format_that_spells_it() {
        for (arg, spelled) in spellings() {
            assert_eq!(
                read(&format!(r#"[["name",{spelled}]]"#)),
                vec![pair("name", arg.clone())],
                "{spelled}"
            );
        }
    }

    #[test]
    fn arguments_keep_their_names_and_their_order_and_may_repeat() {
        let args = [
            ("transition", MsgArg::Text("report-progress".to_owned())),
            ("exit_code", MsgArg::Text("3".to_owned())),
            ("transition", MsgArg::Count(2)),
        ];
        let text = r#"[["transition",{"type":"text","value":"report-progress"}],["exit_code",{"type":"text","value":"3"}],["transition",{"type":"count","value":2}]]"#;

        assert_eq!(written(&args), text);
        assert_eq!(read(text), args.map(|(name, arg)| pair(name, arg)).to_vec());
    }

    #[test]
    fn no_arguments_are_an_empty_list() {
        assert_eq!(written(&[]), "[]");
        assert_eq!(read("[]"), Vec::new());
    }

    #[test]
    fn what_was_written_reads_back_as_it_was() {
        let texts = [
            "",
            "plain",
            "line\nbreak\r\n",
            "\"quoted\" and \\ backslash",
            "NUL \u{0} and \u{1f} controls",
            "é € 😀 \u{202e}right-to-left",
            "[[\"looks\",{\"type\":\"like\"}]]",
        ];
        for text in texts {
            let args = [
                ("a", MsgArg::Text(text.to_owned())),
                ("b", MsgArg::Count(0)),
            ];
            let back = read(&written(&args));
            assert_eq!(
                back,
                args.map(|(name, arg)| pair(name, arg)).to_vec(),
                "{text:?}"
            );
        }

        // A message at the worker protocol's limit is stored and read whole.
        let long = "x".repeat(1024 * 1024);
        let args = [("detail", MsgArg::Text(long))];
        let back = read(&written(&args));
        assert_eq!(back, args.map(|(name, arg)| pair(name, arg)).to_vec());
    }

    /// Rows written before the format had a type of its own: the exact text
    /// the store has always written, and what other tools or a hand-edit
    /// could have left.
    #[test]
    fn rows_written_in_the_current_format_are_read() {
        assert_eq!(
            read(r#"[["exit_code",{"type":"text","value":"3"}]]"#),
            vec![pair("exit_code", MsgArg::Text("3".to_owned()))]
        );
        assert_eq!(
            read(r#"[["transition",{"type":"text","value":"report-progress"}]]"#),
            vec![pair(
                "transition",
                MsgArg::Text("report-progress".to_owned())
            )]
        );
        assert_eq!(read("[]"), Vec::new());
    }

    #[test]
    fn how_the_text_is_laid_out_does_not_matter_to_reading() {
        let spaced = " [ [\"exit_code\", {\"type\": \"text\", \"value\": \"3\"}] ,\n [\"signal\" , { \"value\" : \"9\" , \"type\" : \"text\" }] ] ";
        assert_eq!(
            read(spaced),
            vec![
                pair("exit_code", MsgArg::Text("3".to_owned())),
                pair("signal", MsgArg::Text("9".to_owned())),
            ]
        );
    }

    #[test]
    fn fields_this_version_does_not_know_are_ignored() {
        assert_eq!(
            read(
                r#"[["when",{"type":"timestamp","value":{"utc_ms":5,"zone":"UTC","precision":"ms"}}],["n",{"type":"count","value":1,"unit":"files"}]]"#
            ),
            vec![
                pair(
                    "when",
                    MsgArg::Timestamp {
                        utc_ms: 5,
                        zone: Some(zone("UTC")),
                    },
                ),
                pair("n", MsgArg::Count(1)),
            ]
        );
    }

    #[test]
    fn text_that_is_not_a_list_of_pairs_of_arguments_is_refused() {
        let refused = [
            "",
            " ",
            "not json",
            "exit_code=3",
            "null",
            "3",
            "\"text\"",
            r#"{"exit_code":"3"}"#,
            r#"[["exit_code"]]"#,
            r#"[["exit_code",{"type":"text","value":"3"},"extra"]]"#,
            r#"[[1,{"type":"text","value":"3"}]]"#,
            r#"[["exit_code",{"type":"text","value":"3"}]"#,
            r#"[["exit_code",{"type":"text","value":"3"}]] trailing"#,
            r#"[["exit_code",{"type":"text"}]]"#,
            r#"[["exit_code",{"value":"3"}]]"#,
            r#"[["exit_code","3"]]"#,
            r#"[["exit_code",null]]"#,
        ];
        for text in refused {
            let outcome = decode(text);
            assert_matches!(
                outcome,
                Err(ArgsError::NotPairs(_)),
                "{text:?}: {outcome:?}"
            );
        }
    }

    #[test]
    fn an_argument_of_a_kind_this_version_never_wrote_is_refused_not_guessed() {
        let refused = [
            r#"[["a",{"type":"percent","value":50}]]"#,
            r#"[["a",{"type":"Text","value":"3"}]]"#,
            r#"[["a",{"type":"duration_ms","value":3}]]"#,
            r#"[["a",{"type":"","value":3}]]"#,
            r#"[["a",{"type":3,"value":3}]]"#,
        ];
        for text in refused {
            let outcome = decode(text);
            assert_matches!(
                outcome,
                Err(ArgsError::NotPairs(_)),
                "{text:?}: {outcome:?}"
            );
        }
    }

    #[test]
    fn a_value_of_the_wrong_shape_for_its_kind_is_refused() {
        let refused = [
            r#"[["a",{"type":"count","value":"3"}]]"#,
            r#"[["a",{"type":"count","value":-1}]]"#,
            r#"[["a",{"type":"count","value":1.5}]]"#,
            r#"[["a",{"type":"count","value":18446744073709551616}]]"#,
            r#"[["a",{"type":"bytes","value":null}]]"#,
            r#"[["a",{"type":"duration-ms","value":true}]]"#,
            r#"[["a",{"type":"text","value":3}]]"#,
            r#"[["a",{"type":"text","value":null}]]"#,
            r#"[["a",{"type":"text","value":["3"]}]]"#,
            r#"[["a",{"type":"timestamp","value":5}]]"#,
            r#"[["a",{"type":"timestamp","value":{}}]]"#,
            r#"[["a",{"type":"timestamp","value":{"zone":"UTC"}}]]"#,
            r#"[["a",{"type":"timestamp","value":{"utc_ms":"5"}}]]"#,
            r#"[["a",{"type":"timestamp","value":{"utc_ms":9223372036854775808}}]]"#,
            r#"[["a",{"type":"timestamp","value":{"utc_ms":-9223372036854775809}}]]"#,
            r#"[["a",{"type":"timestamp","value":{"utc_ms":5,"zone":7}}]]"#,
        ];
        for text in refused {
            let outcome = decode(text);
            assert_matches!(
                outcome,
                Err(ArgsError::NotPairs(_)),
                "{text:?}: {outcome:?}"
            );
        }
    }

    #[test]
    fn a_zone_that_is_not_shaped_like_a_time_zone_name_is_refused() {
        let too_long = "A".repeat(IanaZone::MAX_LEN + 1);
        let refused = [
            String::new(),
            "../etc/passwd".to_owned(),
            "/Europe/Berlin".to_owned(),
            "Europe//Berlin".to_owned(),
            "Europe\\Berlin".to_owned(),
            "Europe/Berlin ".to_owned(),
            "Europe/Zürich".to_owned(),
            too_long,
        ];
        for name in refused {
            let json = serde_json::to_string(&name).expect("a string is JSON");
            let text = format!(
                r#"[["when",{{"type":"timestamp","value":{{"utc_ms":5,"zone":{json}}}}}]]"#
            );
            let outcome = decode(&text);
            assert_matches!(
                &outcome,
                Err(ArgsError::BadZone { name: found }) if *found == name,
                "{name:?}: {outcome:?}"
            );
        }
        // The longest name is fine.
        let longest = "A".repeat(IanaZone::MAX_LEN);
        let text = format!(
            r#"[["when",{{"type":"timestamp","value":{{"utc_ms":5,"zone":"{longest}"}}}}]]"#
        );
        assert_eq!(
            read(&text),
            vec![pair(
                "when",
                MsgArg::Timestamp {
                    utc_ms: 5,
                    zone: Some(zone(&longest)),
                },
            )]
        );
    }

    #[test]
    fn a_report_says_what_is_wrong_with_the_column() {
        let not_pairs = decode("exit_code=3").expect_err("not JSON").to_string();
        assert!(not_pairs.contains("list of pairs"), "{not_pairs}");

        let bad_zone =
            decode(r#"[["when",{"type":"timestamp","value":{"utc_ms":5,"zone":"../etc"}}]]"#)
                .expect_err("a bad zone")
                .to_string();
        assert!(bad_zone.contains("../etc"), "{bad_zone}");
        assert!(bad_zone.contains("time-zone"), "{bad_zone}");
    }
}
