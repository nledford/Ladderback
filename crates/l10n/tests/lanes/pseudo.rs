//! The pseudolocales (technical design §8.4): `en-XA` accents every letter and
//! doubles the vowels, so text grows by about a third; `ar-XB` turns the
//! letters upside down and runs the page right to left. Both are the `en-US`
//! catalog, transformed. Text that comes out of a catalog changes and text
//! that doesn't, such as an argument, an address or the operating system's
//! own words, stays as it was, so a screen full of untouched English shows what
//! is hard-coded.
//!
//! These tests need the `pseudo` feature, which this crate has by default;
//! `release` is the lane for a build without it.

use api::{ErrorCode, MsgArg};
use l10n::catalog::{Catalog, Source};
use l10n::{Direction, Localizer, MessageId, msg};

use crate::support::catalog::sample_args;
use crate::support::{borrowed, english, source_locale, tag};

const URL: &str = "http://127.0.0.1:9898";

/// An English catalog of the tests' own: some messages German also has, and
/// one it doesn't.
const EN: Catalog = Catalog {
    tag: "en-US",
    sources: &[Source {
        name: "en.ftl",
        text: "\
shared = The English text
english-only = Only in English
",
    }],
};

const DE: Catalog = Catalog {
    tag: "de",
    sources: &[Source {
        name: "de.ftl",
        text: "shared = Der deutsche Text\n",
    }],
};

const AR: Catalog = Catalog {
    tag: "ar",
    sources: &[Source {
        name: "ar.ftl",
        text: "shared = النص العربي\n",
    }],
};

/// A catalog whose plural forms show in the digit chosen, which no
/// pseudolocale changes. Arabic would choose `few` for three.
const COUNTS: Catalog = Catalog {
    tag: "en-US",
    sources: &[Source {
        name: "counts.ftl",
        text: "\
kinds = { NUMBER($count) ->
    [one] 1
    [few] 3
   *[other] 0
  }
",
    }],
};

/// Messages that write a size and a duration through the helper messages.
const PARTS: Catalog = Catalog {
    tag: "en-US",
    sources: &[Source {
        name: "parts.ftl",
        text: "\
free = { $space } free
took = took { $time }
size = { NUMBER($value, maximumFractionDigits: 1) } { $unit ->
    [kib] KiB
   *[other] B
  }
duration = { $form ->
    [min-s] { $minutes } min { $seconds } s
   *[other] { $seconds } s
  }
",
    }],
};

fn speaking(asked: &str) -> Localizer {
    Localizer::new(&[tag(asked)])
}

fn text(value: &str) -> MsgArg {
    MsgArg::Text(value.to_owned())
}

/// The locales of the chain, in the order they are looked up.
fn chain(localizer: &Localizer) -> Vec<String> {
    localizer
        .locales()
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The chain of locales for what is `asked`, over `catalogs`.
fn chain_of(asked: &[&str], catalogs: &[Catalog]) -> Vec<String> {
    let asked: Vec<_> = asked.iter().map(|asked| tag(asked)).collect();
    chain(&Localizer::from_catalogs(&asked, catalogs))
}

#[test]
fn en_xa_accents_the_letters_of_the_catalog_text_and_doubles_its_vowels() {
    let shown = speaking("en-XA").message(msg::CLI_LISTENING, &[("url", text(URL))]);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    assert_eq!(shown.text(), "ŀişŧeeƞiƞɠ ooƞ http://127.0.0.1:9898");
}

#[test]
fn ar_xb_turns_the_letters_of_the_catalog_text_upside_down() {
    let shown = speaking("ar-XB").message(msg::CLI_LISTENING, &[("url", text(URL))]);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    assert_eq!(shown.text(), "ʅısʇǝuıuƃ ou http://127.0.0.1:9898");
}

#[test]
fn a_term_is_transformed_with_the_message_that_reads_it() {
    // The product's name is catalog text like any other. If it stayed as it
    // is, a hard-coded name would pass for a localized one.
    let en_xa = speaking("en-XA").error(ErrorCode::DeadlinePassed, &[], None);
    assert_eq!(
        en_xa.text(),
        "Ŧħee ooƥeeřaaŧiooƞ ƥaaşşeeḓ iŧş ḓeeaaḓŀiƞee, şoo Ŀaaḓḓeeřƀaaƈķ şŧooƥƥeeḓ iŧ. Řuuƞ iŧ aaɠaaiƞ."
    );
    let ar_xb = speaking("ar-XB").error(ErrorCode::DeadlinePassed, &[], None);
    assert_eq!(
        ar_xb.text(),
        "⊥ɥǝ odǝɹɐʇıou dɐssǝp ıʇs pǝɐpʅıuǝ, so ⅂ɐppǝɹqɐɔʞ sʇoddǝp ıʇ. ᴚnu ıʇ ɐƃɐıu."
    );
}

#[test]
fn the_operating_systems_own_words_stay_as_they_came_in_a_transformed_frame() {
    let detail = "No such file or directory (os error 2)";
    let en_xa = speaking("en-XA").error(ErrorCode::OperationFailed, &[], Some(detail));
    assert!(en_xa.problems().is_empty(), "{:?}", en_xa.problems());
    assert_eq!(
        en_xa.text(),
        format!("Ŧħee ooƥeeřaaŧiooƞ ƒaaiŀeeḓ: {detail}")
    );
    let ar_xb = speaking("ar-XB").error(ErrorCode::OperationFailed, &[], Some(detail));
    assert_eq!(ar_xb.text(), format!("⊥ɥǝ odǝɹɐʇıou ɟɐıʅǝp: {detail}"));
}

#[test]
fn what_a_person_types_stays_as_it_is_in_a_transformed_message() {
    // An address, an option, a header: the catalog writes each as a string
    // literal, which is no text of a language, so that no pseudolocale (and
    // no translation) turns it into something that can't be typed.
    const TYPED: &[(MessageId, &[&str])] = &[
        (
            msg::ERROR_HOST_NOT_ALLOWED,
            &["localhost", "127.0.0.1", "--allowed-host"],
        ),
        (
            msg::ERROR_JSON_REQUIRED,
            &["Content-Type: application/json"],
        ),
        (
            msg::CLI_WARN_NETWORK_SIGN_IN_OFF,
            &["ladderback set-password"],
        ),
        (
            msg::CLI_WARN_SIGN_IN_OFF_BEHIND_PROXY,
            &["ladderback set-password"],
        ),
        (msg::UI_SIGN_IN_OFF_NETWORK, &["ladderback set-password"]),
        (
            msg::CLI_REFUSE_NO_ALLOWED_HOST,
            &["--allowed-host", "--allowed-host nas.example.com"],
        ),
        (
            msg::CLI_ADMIN_PASSWORD_UNREADABLE,
            &["ladderback set-password"],
        ),
        (msg::CLI_SET_PASSWORD_MISMATCH, &["ladderback set-password"]),
        (msg::UI_SIGN_IN_PASSWORD_HINT, &["ladderback set-password"]),
        (msg::UI_SIGN_IN_OFF, &["ladderback set-password"]),
        (
            msg::CLI_INVALID_ALLOWED_HOST,
            &[
                "--allowed-host",
                "nas.example.com",
                "192.168.1.5",
                "[fd00::1]",
            ],
        ),
        (msg::UI_MAINTENANCE_ELSEWHERE, &["maintenance = \"local\""]),
        (
            msg::ERROR_MAINTENANCE_NOT_OWNER,
            &["maintenance = \"local\""],
        ),
        (
            msg::WARNING_LOCATION_PLAIN_HTTP_CREDENTIALS,
            &["http-cacert"],
        ),
        (msg::WARNING_SECRET_FILE_READABLE, &["chmod 600"]),
    ];
    let args = [
        ("address", text("192.168.1.5:9898")),
        ("name", text("nas.example.com:9898")),
        ("host", text("nas.example.com")),
        ("alias", text("demo")),
        ("path", text("/demo/data/admin-password")),
        ("detail", text("permission denied")),
        ("position", text("2")),
        ("cause", text("plain-http")),
        ("rule", text("network")),
        ("locations", text("several")),
        ("mode", text("0644")),
    ];
    for asked in ["en-XA", "ar-XB"] {
        let localizer = speaking(asked);
        for (id, literals) in TYPED {
            let shown = localizer.message(*id, &args);
            assert!(
                shown.problems().is_empty(),
                "{asked} {}: {:?}",
                id.as_str(),
                shown.problems()
            );
            // The words around the literals are transformed.
            assert!(
                !shown.text().is_ascii(),
                "{asked} {} is not transformed: {shown}",
                id.as_str()
            );
            for literal in *literals {
                assert!(
                    shown.text().contains(literal),
                    "{asked} {} changes `{literal}`: {shown}",
                    id.as_str()
                );
            }
        }
    }
}

#[test]
fn a_cli_message_in_a_pseudolocale_shows_each_argument_exactly_as_given() {
    let source = source_locale();
    let plain = english();
    for asked in ["en-XA", "ar-XB"] {
        let localizer = speaking(asked);
        let cli = msg::ALL
            .iter()
            .copied()
            .filter(|id| id.as_str().starts_with("cli-"));
        for id in cli {
            let value = source
                .messages
                .get(id.as_str())
                .and_then(|message| message.value.as_ref())
                .unwrap_or_else(|| panic!("`{}` has a value", id.as_str()));
            // Text no formatter may change: digits a locale would group,
            // braces, quotes, a path, and letters a transform would turn.
            let args: Vec<(String, MsgArg)> = value
                .variables
                .iter()
                .map(|name| {
                    let text = format!("{name}: /home/demo/it's \"1,000\" {{ 9898 }}");
                    (name.clone(), MsgArg::Text(text))
                })
                .collect();
            let shown = localizer.message(id, &borrowed(&args));
            assert!(
                shown.problems().is_empty(),
                "{asked} {}: {:?}",
                id.as_str(),
                shown.problems()
            );
            // Verbatim means nothing for text that was never transformed.
            let unchanged = plain.message(id, &borrowed(&args));
            assert_ne!(shown.text(), unchanged.text(), "{asked} {}", id.as_str());
            for (name, arg) in &args {
                let MsgArg::Text(text) = arg else {
                    panic!("a text argument");
                };
                assert!(
                    shown.text().contains(text.as_str()),
                    "{asked} {} changes `{name}`: {shown}",
                    id.as_str()
                );
            }
        }
    }
}

#[test]
fn every_message_is_transformed_in_every_branch_without_a_problem() {
    let source = source_locale();
    let plain = english();
    for asked in ["en-XA", "ar-XB"] {
        let localizer = speaking(asked);
        for id in msg::ALL.iter().copied() {
            let value = source
                .messages
                .get(id.as_str())
                .and_then(|message| message.value.as_ref())
                .unwrap_or_else(|| panic!("`{}` has a value", id.as_str()));
            for args in sample_args(value) {
                let args = borrowed(&args);
                let shown = localizer.message(id, &args);
                assert!(
                    shown.problems().is_empty(),
                    "{asked} {}: {:?}",
                    id.as_str(),
                    shown.problems()
                );
                // The helpers that write a size and a duration hold bits of
                // a unit, which a transform may leave as they were ("3 s"
                // upside down), and `ui-instant` is a `DATETIME` call
                // through and through: the host's format is not the
                // catalog's to transform (§8.4). `the_helpers_that_write_...`
                // has them. `ui-snapshot-time-offset` holds only its two
                // values and "UTC", a string literal a pseudolocale leaves
                // as it is: there is no text of the catalog's to transform.
                if id == msg::SIZE
                    || id == msg::DURATION
                    || id == msg::UI_INSTANT
                    || id == msg::UI_SNAPSHOT_TIME_OFFSET
                {
                    continue;
                }
                let unchanged = plain.message(id, &args);
                assert_ne!(shown.text(), unchanged.text(), "{asked} {}", id.as_str());
                assert!(
                    !shown.text().is_ascii(),
                    "{asked} {} has only English letters left: {shown}",
                    id.as_str()
                );
            }
        }
    }
}

#[test]
fn a_pseudolocale_picks_plural_forms_by_the_english_rules() {
    for asked in ["en-XA", "ar-XB"] {
        let localizer = Localizer::from_catalogs(&[tag(asked)], &[COUNTS]);
        // The pseudolocale is in effect, so the English rules are the ones
        // it chose, and not the only ones there were.
        assert_eq!(chain(&localizer), [asked, "en-US"]);
        let kind = |count| {
            localizer
                .message(MessageId::new("kinds"), &[("count", MsgArg::Count(count))])
                .into_text()
        };
        assert_eq!(kind(1), "1", "{asked}: one");
        assert_eq!(kind(0), "0", "{asked}: zero");
        // English has no `few`, which Arabic has for three to ten.
        assert_eq!(kind(3), "0", "{asked}: three");
    }
}

#[test]
fn the_helpers_that_write_a_size_and_a_duration_speak_the_pseudolocale_too() {
    let size = MsgArg::Bytes(1536);
    let time = MsgArg::DurationMs(90_000);
    let in_en_xa = Localizer::from_catalogs(&[tag("en-XA")], &[PARTS]);
    let free = in_en_xa.message(MessageId::new("free"), &[("space", size.clone())]);
    assert!(free.problems().is_empty(), "{:?}", free.problems());
    assert_eq!(free.text(), "1.5 ĶiƁ ƒřeeee");
    let took = in_en_xa.message(MessageId::new("took"), &[("time", time.clone())]);
    assert!(took.problems().is_empty(), "{:?}", took.problems());
    assert_eq!(took.text(), "ŧooooķ 1 ḿiƞ 30 ş");
    let in_ar_xb = Localizer::from_catalogs(&[tag("ar-XB")], &[PARTS]);
    let free = in_ar_xb.message(MessageId::new("free"), &[("space", size)]);
    assert_eq!(free.text(), "1.5 ӼıԐ ɟɹǝǝ");
    let took = in_ar_xb.message(MessageId::new("took"), &[("time", time)]);
    assert_eq!(took.text(), "ʇooʞ 1 ɯıu 30 s");
}

#[test]
fn isolation_marks_wrap_a_value_in_a_pseudolocale_and_the_value_stays_as_given() {
    let shown = speaking("en-XA")
        .with_isolation(true)
        .message(msg::CLI_LISTENING, &[("url", text(URL))]);
    assert_eq!(
        shown.text(),
        "ŀişŧeeƞiƞɠ ooƞ \u{2068}http://127.0.0.1:9898\u{2069}"
    );
}

#[test]
fn a_pseudolocale_is_asked_for_by_its_exact_tag() {
    for asked in ["en-XA", "en-xa"] {
        assert_eq!(chain(&speaking(asked)), ["en-XA", "en-US"], "{asked}");
    }
    for asked in ["ar-XB", "ar-xb"] {
        assert_eq!(chain(&speaking(asked)), ["ar-XB", "en-US"], "{asked}");
    }
}

#[test]
fn a_language_that_only_resembles_a_pseudolocale_gets_none() {
    for asked in [
        "en", "en-US", "en-GB", "en-XB", "ar", "ar-XA", "ar-EG", "xa",
    ] {
        let localizer = speaking(asked);
        assert_eq!(chain(&localizer), ["en-US"], "{asked}");
        let shown = localizer.message(msg::CLI_LISTENING, &[("url", text(URL))]);
        assert_eq!(
            shown.text(),
            "listening on http://127.0.0.1:9898",
            "{asked}"
        );
        assert_eq!(localizer.language(), tag("en-US"), "{asked}");
        assert_eq!(localizer.direction(), Direction::Ltr, "{asked}");
    }
}

#[test]
fn a_pseudolocale_asked_for_never_brings_the_real_language_it_imitates() {
    // `ar-XB` is not Arabic: the Arabic catalog, when there is one, is not it.
    let catalogs = [EN, AR];
    assert_eq!(chain_of(&["ar-XB"], &catalogs), ["ar-XB", "en-US"]);
    let shown =
        Localizer::from_catalogs(&[tag("ar-XB")], &catalogs).message(MessageId::new("shared"), &[]);
    assert_eq!(shown.text(), "⊥ɥǝ Ǝuƃʅısɥ ʇǝxʇ");
}

#[test]
fn a_pseudolocale_stands_in_for_en_us_in_the_chain() {
    let catalogs = [EN, DE];
    // A language asked for before it keeps its place.
    assert_eq!(chain_of(&["en-XA"], &catalogs), ["en-XA", "en-US"]);
    assert_eq!(
        chain_of(&["de", "ar-XB"], &catalogs),
        ["de", "ar-XB", "en-US"]
    );
    assert_eq!(
        chain_of(&["de", "en-XA", "fr"], &catalogs),
        ["de", "en-XA", "en-US"]
    );
    assert_eq!(chain_of(&["fr", "ar-XB"], &catalogs), ["ar-XB", "en-US"]);
    // Nothing after it is reached, as nothing is after en-US.
    assert_eq!(chain_of(&["ar-XB", "de"], &catalogs), ["ar-XB", "en-US"]);
    assert_eq!(chain_of(&["en-XA", "ar-XB"], &catalogs), ["en-XA", "en-US"]);
    assert_eq!(chain_of(&["en-US", "en-XA"], &catalogs), ["en-XA", "en-US"]);
}

#[test]
fn a_message_a_language_lacks_falls_through_to_the_pseudolocale_and_not_to_plain_english() {
    let localizer = Localizer::from_catalogs(&[tag("de"), tag("ar-XB")], &[EN, DE]);
    let shared = localizer.message(MessageId::new("shared"), &[]);
    assert_eq!(shared.text(), "Der deutsche Text");
    let english_only = localizer.message(MessageId::new("english-only"), &[]);
    assert_eq!(english_only.text(), "Ouʅʎ ıu Ǝuƃʅısɥ");
    // Asked first, the pseudolocale has the message and German is not reached.
    let first = Localizer::from_catalogs(&[tag("en-XA"), tag("de")], &[EN, DE]);
    let shared = first.message(MessageId::new("shared"), &[]);
    assert_eq!(shared.text(), "Ŧħee Eƞɠŀişħ ŧeeẋŧ");
}

#[test]
fn a_pseudolocale_is_built_from_the_en_us_catalog_and_needs_one() {
    // Without it there is nothing to transform, so it is not in the chain.
    let localizer = Localizer::from_catalogs(&[tag("de"), tag("ar-XB")], &[DE]);
    assert_eq!(chain(&localizer), ["de"]);
    let shown = localizer.message(MessageId::new("shared"), &[]);
    assert_eq!(shown.text(), "Der deutsche Text");
    assert_eq!(localizer.language(), tag("de"));
    let none = Localizer::from_catalogs(&[tag("ar-XB")], &[DE]);
    assert!(none.locales().is_empty());
}

#[test]
fn a_page_in_a_pseudolocale_declares_the_language_it_imitates() {
    let en_xa = speaking("en-XA");
    assert_eq!(en_xa.language(), tag("en-XA"));
    assert_eq!(en_xa.direction(), Direction::Ltr);
    // Arabic, so that the page is tested as a right-to-left one.
    let ar_xb = speaking("ar-XB");
    assert_eq!(ar_xb.language(), tag("ar"));
    assert_eq!(ar_xb.direction(), Direction::Rtl);
}

#[test]
fn a_pseudolocale_further_down_the_chain_declares_nothing() {
    let after_german = Localizer::from_catalogs(&[tag("de"), tag("ar-XB")], &[EN, DE]);
    assert_eq!(after_german.language(), tag("de"));
    assert_eq!(after_german.direction(), Direction::Ltr);
    let after_arabic = Localizer::from_catalogs(&[tag("ar"), tag("en-XA")], &[EN, AR]);
    assert_eq!(after_arabic.language(), tag("ar"));
    assert_eq!(after_arabic.direction(), Direction::Rtl);
}
