//! Wire types shared by the daemon, the hub, the worker and the web UI: the
//! published language (technical design §3.1). Builds for `wasm32`.
//!
//! Errors and warnings cross every boundary as codes with typed arguments,
//! never as sentences; the presentation layer renders them (§8.4).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use serde::{Deserialize, Serialize};

/// Why something failed. Closed: a code this version doesn't know, say from
/// a newer agent, reads as [`ErrorCode::Unknown`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    /// The worker couldn't read the job it was given.
    InvalidJobSpec,
    /// The engine reported a failure; the detail carries its own message.
    OperationFailed,
    /// The job's deadline passed before it finished, so the worker stopped it.
    DeadlinePassed,
    /// The engine couldn't be started, for example because its program is
    /// missing, or it refused the job. The detail carries the platform's or
    /// the adapter's own message.
    EngineUnavailable,
    /// The engine stopped without reporting a result. The argument
    /// `exit_code` is its exit status, left out when none is known: a signal
    /// ended it, its status couldn't be read, or the daemon never learned it.
    EngineExited,
    /// The engine reported events out of order. The argument `transition`
    /// names the step the operation refused.
    ProtocolViolated,
    /// The engine's process didn't start the operation in time, so it never
    /// ran: it didn't take its job, or took it and sent nothing.
    EngineStartTimedOut,
    /// The engine stopped reporting, but its process stayed alive and was
    /// killed.
    EngineExitTimedOut,
    /// The request's `Host` header names neither a loopback address nor a
    /// configured host name (a DNS-rebinding defense, §10.1).
    HostNotAllowed,
    /// The request's `Origin` header isn't the UI's own origin (§10.1).
    OriginNotAllowed,
    /// A mutating request didn't use `Content-Type: application/json` (§10.1).
    JsonRequired,
    /// The configuration file couldn't be read or written (§6.1). The detail
    /// carries the operating system's own message.
    ConfigUnavailable,
    /// The configuration file is larger than any configuration needs, so it
    /// wasn't read.
    ConfigTooLarge,
    /// The configuration file isn't a valid configuration: a key it shouldn't
    /// have, a value out of range, a plan that names a repository it doesn't
    /// have. The detail carries the key and what is wrong with it.
    ConfigInvalid,
    /// The configuration file doesn't say which version of the format it is
    /// in.
    ConfigSchemaMissing,
    /// The configuration file is in a version of the format newer than this
    /// build reads.
    ConfigSchemaNewer,
    /// A repository already exists where one was to be initialised, so
    /// nothing was written. The argument `location` is where it is.
    RepositoryExists,
    /// The configuration already holds a repository under the alias. The
    /// argument `alias` names it.
    RepositoryAliasTaken,
    /// A repository's settings hold a secret reference that couldn't be
    /// resolved. The argument `field` names where it is.
    SecretUnresolved,
    #[serde(other)]
    Unknown,
}

/// A problem with one item that doesn't fail the operation. Closed like [`ErrorCode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningCode {
    SourceUnreadable,
    HookWarning,
    #[serde(other)]
    Unknown,
}

/// A typed message argument. Identifiers, ports, years and hostnames are
/// `Text`, so they are never formatted as numbers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "kebab-case")]
pub enum MsgArg {
    Count(u64),
    Bytes(u64),
    DurationMs(u64),
    /// An instant, as milliseconds since the Unix epoch. The zone, when the
    /// sender has one that matters (a plan's own zone, the agent's zone),
    /// is how the instant is read; without one the viewer's zone applies.
    Timestamp {
        utc_ms: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<IanaZone>,
    },
    Text(String),
}

/// An IANA time-zone name such as `Europe/Berlin`. Checked for syntax only:
/// whether the zone exists is for whoever resolves it. The syntax rules out
/// anything that could walk a file path (`..`, a leading `/`, a backslash).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct IanaZone(String);

/// The text is not shaped like an IANA time-zone name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidZone;

impl std::fmt::Display for InvalidZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not an IANA time-zone name")
    }
}

impl std::error::Error for InvalidZone {}

impl IanaZone {
    /// The longest accepted name; the longest in the database has 32 characters.
    pub const MAX_LEN: usize = 64;

    /// A zone name, if `name` has the shape of one: up to [`MAX_LEN`](Self::MAX_LEN)
    /// characters of `A-Z`, `a-z`, `0-9`, `_`, `+` and `-`, in non-empty
    /// segments joined by `/`.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidZone> {
        let name = name.into();
        let shaped = !name.is_empty()
            && name.len() <= Self::MAX_LEN
            && name.split('/').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
            });
        if shaped {
            Ok(Self(name))
        } else {
            Err(InvalidZone)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for IanaZone {
    type Error = InvalidZone;

    fn try_from(name: String) -> Result<Self, InvalidZone> {
        Self::new(name)
    }
}

impl From<IanaZone> for String {
    fn from(zone: IanaZone) -> String {
        zone.0
    }
}

/// What kind of value a message argument carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArgKind {
    Count,
    Bytes,
    DurationMs,
    Timestamp,
    Text,
}

/// One argument a code's message takes: the name its catalog message uses
/// for it, and what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ArgKind,
    /// Whether the sender may leave it out, as the store does with the exit
    /// status of an engine when none is known. The renderer then supplies
    /// the text `none`, so the message can choose its wording (§8.4). An
    /// optional argument is always text.
    pub optional: bool,
}

impl ErrorCode {
    /// The arguments this code's message takes. A producer sends exactly
    /// these, except that it may leave out an optional one, and the catalog
    /// tests check that the message reads exactly these. A new code lists its
    /// own here, so the compiler asks for them.
    pub const fn args(self) -> &'static [ArgSpec] {
        match self {
            Self::EngineExited => &[ArgSpec {
                name: "exit_code",
                kind: ArgKind::Text,
                optional: true,
            }],
            Self::ProtocolViolated => &[ArgSpec {
                name: "transition",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ConfigUnavailable | Self::ConfigSchemaMissing => &[ArgSpec {
                name: "file",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ConfigTooLarge => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Bytes,
                    optional: false,
                },
            ],
            Self::ConfigInvalid => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "line",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::ConfigSchemaNewer => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "found",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "supported",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::RepositoryExists => &[ArgSpec {
                name: "location",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::RepositoryAliasTaken => &[ArgSpec {
                name: "alias",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::SecretUnresolved => &[ArgSpec {
                name: "field",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::InvalidJobSpec
            | Self::OperationFailed
            | Self::DeadlinePassed
            | Self::EngineUnavailable
            | Self::EngineStartTimedOut
            | Self::EngineExitTimedOut
            | Self::HostNotAllowed
            | Self::OriginNotAllowed
            | Self::JsonRequired
            | Self::Unknown => &[],
        }
    }
}

impl WarningCode {
    /// The arguments this code's message takes (see [`ErrorCode::args`]).
    pub const fn args(self) -> &'static [ArgSpec] {
        match self {
            Self::HookWarning => &[
                ArgSpec {
                    name: "phase",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "exit_code",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::SourceUnreadable | Self::Unknown => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_travel_as_kebab_case_strings() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::InvalidJobSpec).unwrap(),
            r#""invalid-job-spec""#
        );
        assert_eq!(
            serde_json::to_string(&WarningCode::SourceUnreadable).unwrap(),
            r#""source-unreadable""#
        );
    }

    #[test]
    fn a_passed_deadline_travels_as_its_own_code() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::DeadlinePassed).unwrap(),
            r#""deadline-passed""#
        );
        let read: ErrorCode = serde_json::from_str(r#""deadline-passed""#).unwrap();
        assert_eq!(read, ErrorCode::DeadlinePassed);
    }

    #[test]
    fn a_code_from_a_newer_peer_reads_as_unknown() {
        let error: ErrorCode = serde_json::from_str(r#""quota-exceeded""#).unwrap();
        assert_eq!(error, ErrorCode::Unknown);
        let warning: WarningCode = serde_json::from_str(r#""xattr-unreadable""#).unwrap();
        assert_eq!(warning, WarningCode::Unknown);
    }

    #[test]
    fn message_arguments_keep_their_type() {
        for arg in [
            MsgArg::Count(3),
            MsgArg::Bytes(1 << 30),
            MsgArg::DurationMs(90_000),
            MsgArg::Text("9898".to_owned()),
        ] {
            let json = serde_json::to_string(&arg).unwrap();
            assert_eq!(serde_json::from_str::<MsgArg>(&json).unwrap(), arg);
        }
    }

    #[test]
    fn a_zone_name_with_the_shape_of_one_is_accepted_up_to_the_length_cap() {
        let longest = "a".repeat(IanaZone::MAX_LEN);
        for name in [
            "UTC",
            "Europe/Berlin",
            "America/Argentina/Buenos_Aires",
            "America/Port-au-Prince",
            "Etc/GMT+5",
            longest.as_str(),
        ] {
            let zone = IanaZone::new(name).unwrap_or_else(|_| panic!("{name} is a valid shape"));
            assert_eq!(zone.as_str(), name);
        }
    }

    #[test]
    fn a_zone_name_that_could_walk_a_path_or_overflow_is_rejected() {
        let too_long = "a".repeat(IanaZone::MAX_LEN + 1);
        for name in [
            "",
            ".",
            "..",
            "../../etc/passwd",
            "/etc/localtime",
            "Europe/../Berlin",
            "Europe//Berlin",
            "Europe/Berlin/",
            "Europe\\Berlin",
            "Europe/Ber lin",
            "Europe/Berlin\n",
            "Europe/Zürich",
            too_long.as_str(),
        ] {
            assert_eq!(IanaZone::new(name), Err(InvalidZone), "{name:?}");
        }
    }

    #[test]
    fn a_timestamp_travels_as_utc_plus_an_optional_zone() {
        let zoned = MsgArg::Timestamp {
            utc_ms: 1_794_144_600_000,
            zone: Some(IanaZone::new("Europe/Berlin").unwrap()),
        };
        let json = serde_json::to_string(&zoned).unwrap();
        assert_eq!(
            json,
            r#"{"type":"timestamp","value":{"utc_ms":1794144600000,"zone":"Europe/Berlin"}}"#
        );
        assert_eq!(serde_json::from_str::<MsgArg>(&json).unwrap(), zoned);

        // Before the epoch, and without a zone: the field is left out, and a
        // peer that writes `null` instead is read the same way.
        let unzoned = MsgArg::Timestamp {
            utc_ms: -1,
            zone: None,
        };
        let json = serde_json::to_string(&unzoned).unwrap();
        assert_eq!(json, r#"{"type":"timestamp","value":{"utc_ms":-1}}"#);
        assert_eq!(serde_json::from_str::<MsgArg>(&json).unwrap(), unzoned);
        let explicit_null = r#"{"type":"timestamp","value":{"utc_ms":-1,"zone":null}}"#;
        assert_eq!(
            serde_json::from_str::<MsgArg>(explicit_null).unwrap(),
            unzoned
        );
    }

    #[test]
    fn a_timestamp_naming_a_hostile_zone_is_rejected_when_read() {
        let hostile = r#"{"type":"timestamp","value":{"utc_ms":0,"zone":"../../etc/passwd"}}"#;
        assert!(serde_json::from_str::<MsgArg>(hostile).is_err());
    }

    #[test]
    fn a_code_declares_the_arguments_its_producers_send() {
        let spec = |name, kind, optional| ArgSpec {
            name,
            kind,
            optional,
        };
        assert_eq!(
            ErrorCode::EngineExited.args(),
            &[spec("exit_code", ArgKind::Text, true)]
        );
        assert_eq!(
            ErrorCode::ProtocolViolated.args(),
            &[spec("transition", ArgKind::Text, false)]
        );
        for code in [
            ErrorCode::InvalidJobSpec,
            ErrorCode::OperationFailed,
            ErrorCode::DeadlinePassed,
            ErrorCode::EngineUnavailable,
            ErrorCode::EngineStartTimedOut,
            ErrorCode::EngineExitTimedOut,
            ErrorCode::HostNotAllowed,
            ErrorCode::OriginNotAllowed,
            ErrorCode::JsonRequired,
            ErrorCode::Unknown,
        ] {
            assert!(code.args().is_empty(), "{code:?}");
        }
        for code in [WarningCode::SourceUnreadable, WarningCode::Unknown] {
            assert!(code.args().is_empty(), "{code:?}");
        }
    }

    #[test]
    fn config_codes_declare_the_arguments_their_producers_send() {
        let text = |name, optional| ArgSpec {
            name,
            kind: ArgKind::Text,
            optional,
        };
        assert_eq!(ErrorCode::ConfigUnavailable.args(), &[text("file", false)]);
        assert_eq!(
            ErrorCode::ConfigTooLarge.args(),
            &[
                text("file", false),
                ArgSpec {
                    name: "limit",
                    kind: ArgKind::Bytes,
                    optional: false,
                },
            ]
        );
        assert_eq!(
            ErrorCode::ConfigInvalid.args(),
            &[text("file", false), text("line", true)]
        );
        assert_eq!(
            ErrorCode::ConfigSchemaMissing.args(),
            &[text("file", false)]
        );
        assert_eq!(
            ErrorCode::ConfigSchemaNewer.args(),
            &[
                text("file", false),
                text("found", false),
                text("supported", false),
            ]
        );
    }
}

/// How many paths one listed snapshot may carry. An input-derived size,
/// capped (§13.5): a snapshot lists at most a handful of roots.
pub const MAX_SNAPSHOT_PATHS: usize = 64;

/// One snapshot of a repository's list, as the daemon tells the web UI:
/// what a list view needs — id, when it was taken (RFC 3339), which host,
/// its label, tags and paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDto {
    /// 64 hex digits, as rustic writes a snapshot's ID.
    pub id: String,
    /// RFC 3339, UTC.
    pub time: String,
    pub hostname: String,
    /// Empty when the snapshot carries no label.
    pub label: String,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
}

/// Why a snapshot DTO can't be: an ID that isn't 64 hex digits, a time that
/// isn't RFC 3339, or more paths than [`MAX_SNAPSHOT_PATHS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidSnapshot {
    Id,
    Time,
    Paths,
}

impl std::fmt::Display for InvalidSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for InvalidSnapshot {}

impl SnapshotDto {
    /// Builds the DTO from what the engine reported, refusing an ID that
    /// isn't 64 hex digits, a time that isn't RFC 3339, and more paths than
    /// [`MAX_SNAPSHOT_PATHS`] (an input-derived size, capped).
    pub fn new(
        id: String,
        time: String,
        hostname: String,
        label: String,
        tags: Vec<String>,
        paths: Vec<String>,
    ) -> Result<Self, InvalidSnapshot> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(InvalidSnapshot::Id);
        }
        if time.parse::<jiff::Timestamp>().is_err() {
            return Err(InvalidSnapshot::Time);
        }
        if paths.len() > MAX_SNAPSHOT_PATHS {
            return Err(InvalidSnapshot::Paths);
        }
        Ok(Self {
            id,
            time,
            hostname,
            label,
            tags,
            paths,
        })
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::{InvalidSnapshot, MAX_SNAPSHOT_PATHS, SnapshotDto};

    fn dto() -> SnapshotDto {
        SnapshotDto {
            id: "ab".repeat(32),
            time: "2026-11-09T10:15:00Z".to_owned(),
            hostname: "demo-host".to_owned(),
            label: "home".to_owned(),
            tags: vec!["ladderback-plan:demo-home".to_owned()],
            paths: vec!["/home/demo".to_owned()],
        }
    }

    #[test]
    fn a_snapshot_round_trips_through_json() {
        let dto = dto();
        let built = SnapshotDto::new(
            dto.id.clone(),
            dto.time.clone(),
            dto.hostname.clone(),
            dto.label.clone(),
            dto.tags.clone(),
            dto.paths.clone(),
        )
        .unwrap_or_else(|e| panic!("a valid snapshot: {e:?}"));
        let json = serde_json::to_string(&built).unwrap();
        assert_eq!(serde_json::from_str::<SnapshotDto>(&json).unwrap(), built);
        assert_eq!(built, dto);
    }

    #[test]
    fn an_id_that_is_not_64_hex_digits_is_refused() {
        let dto = dto();
        assert_eq!(
            SnapshotDto::new(
                "nope".to_owned(),
                dto.time,
                dto.hostname,
                dto.label,
                dto.tags,
                dto.paths,
            ),
            Err(InvalidSnapshot::Id)
        );
    }

    #[test]
    fn a_time_that_is_not_rfc_3339_is_refused() {
        let dto = dto();
        assert_eq!(
            SnapshotDto::new(
                dto.id,
                "not a time".to_owned(),
                dto.hostname,
                dto.label,
                dto.tags,
                dto.paths,
            ),
            Err(InvalidSnapshot::Time)
        );
    }

    #[test]
    fn paths_beyond_the_cap_are_refused() {
        let dto = dto();
        assert_eq!(
            SnapshotDto::new(
                dto.id,
                dto.time,
                dto.hostname,
                dto.label,
                dto.tags,
                vec!["/home/demo".to_owned(); MAX_SNAPSHOT_PATHS + 1],
            ),
            Err(InvalidSnapshot::Paths)
        );
    }
}
