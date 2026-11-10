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
    /// A repository was to be saved under a name that is not an alias: a
    /// name is up to 64 lower-case letters, digits and hyphens, starting
    /// with a letter or a digit.
    RepositoryAliasInvalid,
    /// A repository's settings hold a secret reference that couldn't be
    /// resolved. The argument `field` names where it is.
    SecretUnresolved,
    /// The configuration holds no plan by that name. The argument `plan`
    /// names it.
    PlanUnknown,
    /// The configuration holds no repository by that name. The argument
    /// `alias` names it.
    RepositoryUnknown,
    /// The request names an operation the log has none of. The argument
    /// `operation` names it.
    OperationUnknown,
    /// The request asked to cancel an operation that is not running, so
    /// there is nothing to stop. The argument `operation` names it.
    OperationNotRunning,
    /// A history request's cursor is not an operation's ID. The argument
    /// `cursor` is what was sent.
    InvalidCursor,
    /// A request's body is not the JSON the route takes.
    InvalidBody,
    /// Another maintenance operation already runs on the repository (I7):
    /// nothing ran. The argument `operation` names the one that holds it.
    MaintenanceBusy,
    /// The plan's retention policy is set and would keep nothing, so the
    /// forget was refused before anything was removed. The argument `plan`
    /// names it.
    RetentionKeepsNothing,
    /// The request asks for a maintenance kind this version doesn't run.
    /// The argument `kind` is what was sent.
    MaintenanceKindUnknown,
    /// The group an adoption named matches no plan of this agent: nothing
    /// was written.
    AdoptionNoMatch,
    /// More than one plan matches the group an adoption named. The argument
    /// `plans` names them all.
    AdoptionAmbiguous,
    /// A rustic profile couldn't be read: the file isn't there, or isn't
    /// readable. The argument `file` is the path as it was given, or the path
    /// of the profile it uses that is at fault.
    ProfileUnreadable,
    /// A rustic profile is larger than any profile needs, so it wasn't read.
    /// The argument `file` names it.
    ProfileTooLarge,
    /// A file isn't a profile rustic accepts. The argument `file` names it;
    /// `key` is the setting at fault and `line` where it is written, each
    /// left out when none is known. The detail says what is wrong.
    ProfileInvalid,
    /// A profile says something Ladderback can't honour, so nothing was
    /// imported. The argument `key` is the setting, `file` the profile and
    /// `line` where it is written, left out when none is known.
    ProfileUnsupported,
    /// The configuration, or the profile itself, already has a plan with this
    /// ID, so nothing was imported. The argument `plan` names it.
    PlanIdTaken,
    /// A plan can't be exported as a profile because its repository's
    /// password is written in the configuration: a profile would have to
    /// repeat it as text. Nothing was written.
    InlineSecret,
    /// A plan can't be exported as a profile because the value at `key` of
    /// the repository is a reference rustic can't resolve, and there is no
    /// variable to set instead. Nothing was written. The argument `key` is the
    /// setting.
    ProfileExportReference,
    #[serde(other)]
    Unknown,
}

/// A problem with one item that doesn't fail the operation. Closed like [`ErrorCode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningCode {
    SourceUnreadable,
    /// An import left out a setting of the profile that Ladderback has no
    /// place for. The argument `key` is its path in the profile.
    ProfileKeyIgnored,
    /// The profile prunes after its forget, and the repository now prunes
    /// from one place.
    ProfilePrune,
    /// The profile's `[global.env]` was left out.
    ProfileEnvIgnored,
    /// Hooks set for every snapshot entry of the profile now run once per
    /// plan.
    ProfileHooksPerPlan,
    /// A profile named by `use-profiles` wasn't found. The argument `name` is
    /// the name as the profile wrote it.
    ProfileUsedMissing,
    /// A secret written in the profile was kept as text in the configuration.
    /// The argument `field` names it.
    InlineSecret,
    /// A tag of Ladderback's own, found on a snapshot entry, was dropped. The
    /// argument `tag` is the tag.
    ProfileTagDropped,
    /// An export left an option out, because it is a reference. The argument
    /// `option` names it and `variable` is the environment variable that
    /// sets it.
    ProfileOptionLeftOut,
    HookWarning,
    /// A forget or an adoption ended before its last engine job; the jobs
    /// that finished stay done.
    MaintenancePartial,
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
            Self::RepositoryAliasInvalid => &[],
            Self::SecretUnresolved => &[ArgSpec {
                name: "field",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::PlanUnknown => &[ArgSpec {
                name: "plan",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::RepositoryUnknown => &[ArgSpec {
                name: "alias",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::OperationUnknown | Self::OperationNotRunning => &[ArgSpec {
                name: "operation",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::InvalidCursor => &[ArgSpec {
                name: "cursor",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::MaintenanceBusy => &[ArgSpec {
                name: "operation",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::RetentionKeepsNothing => &[ArgSpec {
                name: "plan",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::MaintenanceKindUnknown => &[ArgSpec {
                name: "kind",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::AdoptionNoMatch => &[],
            Self::AdoptionAmbiguous => &[ArgSpec {
                name: "plans",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileUnreadable => &[ArgSpec {
                name: "file",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileTooLarge => &[
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
            Self::ProfileInvalid => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "key",
                    kind: ArgKind::Text,
                    optional: true,
                },
                ArgSpec {
                    name: "line",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::ProfileUnsupported => &[
                ArgSpec {
                    name: "file",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "key",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "line",
                    kind: ArgKind::Text,
                    optional: true,
                },
            ],
            Self::PlanIdTaken => &[ArgSpec {
                name: "plan",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileExportReference => &[ArgSpec {
                name: "key",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::InvalidBody
            | Self::InvalidJobSpec
            | Self::OperationFailed
            | Self::DeadlinePassed
            | Self::EngineUnavailable
            | Self::EngineStartTimedOut
            | Self::EngineExitTimedOut
            | Self::HostNotAllowed
            | Self::OriginNotAllowed
            | Self::JsonRequired
            | Self::InlineSecret
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
            Self::MaintenancePartial => &[
                ArgSpec {
                    name: "what",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "done",
                    kind: ArgKind::Count,
                    optional: false,
                },
                ArgSpec {
                    name: "total",
                    kind: ArgKind::Count,
                    optional: false,
                },
            ],
            Self::ProfileKeyIgnored => &[ArgSpec {
                name: "key",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileUsedMissing => &[ArgSpec {
                name: "name",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::InlineSecret => &[ArgSpec {
                name: "field",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileTagDropped => &[ArgSpec {
                name: "tag",
                kind: ArgKind::Text,
                optional: false,
            }],
            Self::ProfileOptionLeftOut => &[
                ArgSpec {
                    name: "option",
                    kind: ArgKind::Text,
                    optional: false,
                },
                ArgSpec {
                    name: "variable",
                    kind: ArgKind::Text,
                    optional: false,
                },
            ],
            Self::SourceUnreadable
            | Self::ProfilePrune
            | Self::ProfileEnvIgnored
            | Self::ProfileHooksPerPlan
            | Self::Unknown => &[],
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
    fn a_partial_maintenance_run_has_its_own_code_and_declares_its_arguments() {
        assert_eq!(
            serde_json::to_string(&WarningCode::MaintenancePartial).unwrap(),
            r#""maintenance-partial""#
        );
        let spec = |name, kind| ArgSpec {
            name,
            kind,
            optional: false,
        };
        assert_eq!(
            WarningCode::MaintenancePartial.args(),
            &[
                spec("what", ArgKind::Text),
                spec("done", ArgKind::Count),
                spec("total", ArgKind::Count),
            ]
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

    #[test]
    fn profile_codes_declare_the_arguments_their_producers_send() {
        let text = |name, optional| ArgSpec {
            name,
            kind: ArgKind::Text,
            optional,
        };
        assert_eq!(ErrorCode::ProfileUnreadable.args(), &[text("file", false)]);
        assert_eq!(
            ErrorCode::ProfileExportReference.args(),
            &[text("key", false)]
        );
        assert!(ErrorCode::InlineSecret.args().is_empty());
        assert_eq!(
            ErrorCode::ProfileInvalid.args(),
            &[text("file", false), text("key", true), text("line", true)]
        );
        assert_eq!(
            ErrorCode::ProfileUnsupported.args(),
            &[text("file", false), text("key", false), text("line", true)]
        );
        assert_eq!(ErrorCode::PlanIdTaken.args(), &[text("plan", false)]);
        assert_eq!(
            WarningCode::ProfileOptionLeftOut.args(),
            &[text("option", false), text("variable", false)]
        );
        for code in [
            WarningCode::ProfilePrune,
            WarningCode::ProfileEnvIgnored,
            WarningCode::ProfileHooksPerPlan,
        ] {
            assert!(code.args().is_empty(), "{code:?}");
        }
    }
}

/// How many paths one listed snapshot may carry. An input-derived size,
/// capped (§13.5): a snapshot lists at most a handful of roots.
pub const MAX_SNAPSHOT_PATHS: usize = 64;

/// How many tags one listed snapshot may carry. An input-derived size,
/// capped (§13.5) like the paths.
pub const MAX_SNAPSHOT_TAGS: usize = 64;

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
    /// The snapshot has more paths than `paths` lists, or one of them is cut
    /// short: a list shows at most [`MAX_SNAPSHOT_PATHS`] of them, each of a
    /// bounded length, and says so here instead of leaving them out silently.
    #[serde(default)]
    pub paths_truncated: bool,
    /// The same for `tags`, which a list shows at most [`MAX_SNAPSHOT_TAGS`]
    /// of.
    #[serde(default)]
    pub tags_truncated: bool,
}

/// Why a snapshot DTO can't be: an ID that isn't 64 hex digits, a time that
/// isn't RFC 3339, or more paths than [`MAX_SNAPSHOT_PATHS`] or tags than
/// [`MAX_SNAPSHOT_TAGS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidSnapshot {
    Id,
    Time,
    Paths,
    Tags,
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
    /// [`MAX_SNAPSHOT_PATHS`] or tags than [`MAX_SNAPSHOT_TAGS`] (input-derived
    /// sizes, capped). The truncation flags start false: the engine's list
    /// says which snapshots it cut, and [`SnapshotDto::truncated`] carries that
    /// over.
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
        if tags.len() > MAX_SNAPSHOT_TAGS {
            return Err(InvalidSnapshot::Tags);
        }
        Ok(Self {
            id,
            time,
            hostname,
            label,
            tags,
            paths,
            paths_truncated: false,
            tags_truncated: false,
        })
    }

    /// The snapshot with what the engine reported it cut: some of its paths,
    /// some of its tags.
    #[must_use]
    pub fn truncated(mut self, paths: bool, tags: bool) -> Self {
        self.paths_truncated = paths;
        self.tags_truncated = tags;
        self
    }
}
/// One live update the daemon pushes to a connected page over the events
/// WebSocket (technical design §4.4): what an operation reported, or that an
/// agent came or went. Identifiers are text, counts are numbers, and times
/// are RFC 3339 strings. The daemon forwards these unchanged: coalescing is
/// the worker's, once (§4.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UiEvent {
    /// An operation left the queue and is running.
    OperationStarted {
        operation: String,
        repository: Option<String>,
        started_at: String,
    },
    /// A running operation moved forward: `current` of `total` steps, when it
    /// says. Progress is transient: it is never stored, and a subscriber
    /// whose buffer fills loses the oldest of these first.
    OperationProgress {
        operation: String,
        current: u64,
        total: Option<u64>,
    },
    /// A problem with one item that doesn't fail the operation.
    OperationWarning {
        operation: String,
        code: WarningCode,
        args: Vec<MsgArg>,
    },
    /// The operation ended well.
    OperationSucceeded {
        operation: String,
        finished_at: String,
    },
    /// The operation ended badly.
    OperationFailed {
        operation: String,
        code: ErrorCode,
        args: Vec<MsgArg>,
        finished_at: String,
    },
    /// An agent joined the fleet.
    AgentOnline { agent: String, at: String },
    /// An agent left the fleet.
    AgentOffline { agent: String, at: String },
}

#[cfg(test)]
mod snapshot_tests {
    use super::{InvalidSnapshot, MAX_SNAPSHOT_PATHS, MAX_SNAPSHOT_TAGS, SnapshotDto};

    fn dto() -> SnapshotDto {
        SnapshotDto {
            id: "ab".repeat(32),
            time: "2026-11-09T10:15:00Z".to_owned(),
            hostname: "demo-host".to_owned(),
            label: "home".to_owned(),
            tags: vec!["ladderback-plan:demo-home".to_owned()],
            paths: vec!["/home/demo".to_owned()],
            paths_truncated: false,
            tags_truncated: false,
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
    fn a_snapshot_that_lost_paths_or_tags_says_which() {
        let truncated = dto().truncated(true, false);
        assert!(truncated.paths_truncated && !truncated.tags_truncated);
        let truncated = dto().truncated(false, true);
        assert!(!truncated.paths_truncated && truncated.tags_truncated);
        let json = serde_json::to_string(&dto().truncated(true, true)).unwrap();
        assert!(json.contains(r#""paths_truncated":true"#), "{json}");
        assert!(json.contains(r#""tags_truncated":true"#), "{json}");
    }

    #[test]
    fn a_snapshot_from_a_daemon_that_never_said_is_not_truncated() {
        let json = r#"{"id":"abababababababababababababababababababababababababababababababab","time":"2026-11-09T10:15:00Z","hostname":"h","label":"","tags":[],"paths":[]}"#;
        let dto: SnapshotDto = serde_json::from_str(json).unwrap();
        assert!(!dto.paths_truncated && !dto.tags_truncated);
    }

    #[test]
    fn tags_beyond_the_cap_are_refused_and_the_cap_itself_is_taken() {
        let dto = dto();
        let build = |tags: usize| {
            SnapshotDto::new(
                dto.id.clone(),
                dto.time.clone(),
                dto.hostname.clone(),
                dto.label.clone(),
                vec!["tag".to_owned(); tags],
                dto.paths.clone(),
            )
        };
        assert_eq!(build(MAX_SNAPSHOT_TAGS + 1), Err(InvalidSnapshot::Tags));
        assert!(build(MAX_SNAPSHOT_TAGS).is_ok());
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

#[cfg(test)]
mod ui_event_tests {
    use serde_json::{Value, from_value, to_value};

    use super::*;

    fn started() -> UiEvent {
        UiEvent::OperationStarted {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            repository: Some("home.demo".into()),
            started_at: "2026-11-09T10:00:00Z".into(),
        }
    }

    fn progress() -> UiEvent {
        UiEvent::OperationProgress {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            current: 3,
            total: Some(10),
        }
    }

    fn succeeded() -> UiEvent {
        UiEvent::OperationSucceeded {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            finished_at: "2026-11-09T10:05:00Z".into(),
        }
    }
    #[test]
    fn each_event_serializes_with_its_kebab_case_type() {
        for (event, kind) in [
            (started(), "operation-started"),
            (progress(), "operation-progress"),
            (
                UiEvent::OperationWarning {
                    operation: "018f0000-0000-7000-8000-000000000001".into(),
                    code: WarningCode::SourceUnreadable,
                    args: Vec::new(),
                },
                "operation-warning",
            ),
            (succeeded(), "operation-succeeded"),
            (
                UiEvent::OperationFailed {
                    operation: "018f0000-0000-7000-8000-000000000001".into(),
                    code: ErrorCode::EngineUnavailable,
                    args: Vec::new(),
                    finished_at: "2026-11-09T10:05:00Z".into(),
                },
                "operation-failed",
            ),
            (
                UiEvent::AgentOnline {
                    agent: "demo".into(),
                    at: "2026-11-09T10:00:00Z".into(),
                },
                "agent-online",
            ),
            (
                UiEvent::AgentOffline {
                    agent: "demo".into(),
                    at: "2026-11-09T10:00:00Z".into(),
                },
                "agent-offline",
            ),
        ] {
            let wire = to_value(&event).unwrap();
            assert_eq!(
                wire.get("type").and_then(Value::as_str),
                Some(kind),
                "{event:?}"
            );
        }
    }

    #[test]
    fn the_fields_of_each_event_carry_their_own_names() {
        let wire = to_value(started()).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "type": "operation-started",
                "operation": "018f0000-0000-7000-8000-000000000001",
                "repository": "home.demo",
                "started_at": "2026-11-09T10:00:00Z",
            })
        );
        let wire = to_value(progress()).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "type": "operation-progress",
                "operation": "018f0000-0000-7000-8000-000000000001",
                "current": 3,
                "total": 10,
            })
        );
        let wire = to_value(&UiEvent::OperationProgress {
            operation: "018f0000-0000-7000-8000-000000000001".into(),
            current: 1,
            total: None,
        })
        .unwrap();
        assert_eq!(wire.get("total"), Some(&Value::Null));
    }

    #[test]
    fn every_event_round_trips_through_its_wire_form() {
        let events = [
            started(),
            progress(),
            UiEvent::OperationProgress {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                current: 1,
                total: None,
            },
            UiEvent::OperationWarning {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                code: WarningCode::SourceUnreadable,
                args: Vec::new(),
            },
            succeeded(),
            UiEvent::OperationFailed {
                operation: "018f0000-0000-7000-8000-000000000001".into(),
                code: ErrorCode::EngineUnavailable,
                args: Vec::new(),
                finished_at: "2026-11-09T10:05:00Z".into(),
            },
            UiEvent::AgentOnline {
                agent: "demo".into(),
                at: "2026-11-09T10:00:00Z".into(),
            },
            UiEvent::AgentOffline {
                agent: "demo".into(),
                at: "2026-11-09T10:00:00Z".into(),
            },
        ];
        for event in events {
            assert_eq!(
                from_value::<UiEvent>(to_value(&event).unwrap()).unwrap(),
                event
            );
        }
    }

    #[test]
    fn an_event_of_an_unknown_type_is_not_one() {
        let wire = serde_json::json!({"type": "operation-paused"});
        assert!(from_value::<UiEvent>(wire).is_err());
    }
}

/// One operation of the history page, as the daemon tells the web UI: what
/// a list view needs — ids, the kind and status, the lifecycle times, and
/// the typed outcome. Times are RFC 3339, UTC.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OperationSummaryDto {
    /// A UUID, hyphenated.
    pub id: String,
    /// The plan the operation belongs to.
    pub plan: String,
    /// `backup`, `prune`, `check`, …
    pub kind: String,
    /// `queued`, `running`, `succeeded`, `succeeded-with-warnings`,
    /// `failed`, `cancelled`.
    pub status: String,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Every warning the operation recorded, by code, newest last.
    pub warning_codes: Vec<WarningCode>,
    /// Set when the operation failed.
    pub failure_code: Option<ErrorCode>,
    /// What the engine reported when the operation succeeded.
    pub result: Option<OperationResultDto>,
}

/// Why an operation DTO can't be: an ID that isn't a UUID, or a time that
/// isn't RFC 3339.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidOperation {
    Id,
    Time,
}

impl std::fmt::Display for InvalidOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for InvalidOperation {}

impl OperationSummaryDto {
    /// Builds the summary, refusing an ID that isn't a hyphenated UUID and
    /// a time that isn't RFC 3339, like [`SnapshotDto::new`]. The arity is
    /// the wire's: one argument per field of the published shape.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        plan: String,
        kind: String,
        status: String,
        queued_at: String,
        started_at: Option<String>,
        finished_at: Option<String>,
        warning_codes: Vec<WarningCode>,
        failure_code: Option<ErrorCode>,
        result: Option<OperationResultDto>,
    ) -> Result<Self, InvalidOperation> {
        if !is_uuid(&id) {
            return Err(InvalidOperation::Id);
        }
        for time in std::iter::once(queued_at.as_str())
            .chain(started_at.as_deref())
            .chain(finished_at.as_deref())
        {
            if time.parse::<jiff::Timestamp>().is_err() {
                return Err(InvalidOperation::Time);
            }
        }
        Ok(Self {
            id,
            plan,
            kind,
            status,
            queued_at,
            started_at,
            finished_at,
            warning_codes,
            failure_code,
            result,
        })
    }
}

/// A hyphenated UUID: hex digits, with hyphens where a UUID has them.
fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

/// One page of the operation history: newest first, and `next` set when
/// older operations remain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OperationsPageDto {
    pub operations: Vec<OperationSummaryDto>,
    pub next: Option<String>,
}

/// One plan of the plans list, as the daemon tells the web UI: the ID a
/// start request names, and the repository its backups go to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PlanSummaryDto {
    pub id: String,
    pub repository: String,
}

/// The plans list: what the operations page offers to start.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PlansDto {
    pub plans: Vec<PlanSummaryDto>,
}

/// The repositories list: what the Repositories page shows and the snapshots
/// page offers to pick from, in the order of their aliases.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoriesDto {
    pub repositories: Vec<RepositoryDto>,
}

/// One configured repository, as a list shows it: where it is, with any
/// password in the location masked, and where its password comes from.
/// Never the password itself (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryDto {
    pub alias: String,
    /// The location, with the password of a URL in it masked.
    pub repository: String,
    /// The hot half of a hot/cold repository, masked the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_hot: Option<String>,
    pub password: PasswordSourceDto,
}

/// Where a configured repository's password comes from, without its value:
/// all five sources the configuration takes, the command by its program
/// only, since its arguments may hold the secret itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum PasswordSourceDto {
    /// The configuration file holds the password.
    Inline,
    /// A file holds it.
    File { path: String },
    /// An entry of the OS keyring, as `service/name`.
    Keyring { entry: String },
    /// An environment variable of the daemon.
    Env { name: String },
    /// The output of a program.
    Command { program: String },
}

/// Where a request says the repository's password is: the four sources the
/// API takes. A `command` source is not one of them: it runs a program, and
/// the first route that may run one is the one behind authentication
/// (technical design §10.1). Its `Debug` form never shows a typed password.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PasswordDto {
    /// The password itself, as the person typed it.
    Inline { value: String },
    /// A file that holds it, by its absolute path.
    File { path: String },
    /// An entry of the OS keyring, as `service/name`.
    Keyring { entry: String },
    /// An environment variable of the daemon that holds it.
    Env { name: String },
}

impl std::fmt::Debug for PasswordDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inline { .. } => f.write_str("Inline { value: *** }"),
            Self::File { path } => f.debug_struct("File").field("path", path).finish(),
            Self::Keyring { entry } => f.debug_struct("Keyring").field("entry", entry).finish(),
            Self::Env { name } => f.debug_struct("Env").field("name", name).finish(),
        }
    }
}

/// What a request says about one repository: where it is and how to open it.
/// The field names are the configuration's own, so a repository written in
/// the file and one typed into the page mean the same. Its `Debug` form shows
/// no text a person could have put a secret in.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RepositorySettingsDto {
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_hot: Option<String>,
    pub password: PasswordDto,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options_hot: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub options_cold: std::collections::BTreeMap<String, String>,
}

impl std::fmt::Debug for RepositorySettingsDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A location can carry a password in a URL and an option can be a
        // credential, so only what is not text a person typed is shown.
        f.debug_struct("RepositorySettingsDto")
            .field("repository", &"***")
            .field("repo_hot", &self.repo_hot.as_ref().map(|_| "***"))
            .field("password", &self.password)
            .field("options", &self.options.keys().collect::<Vec<_>>())
            .field("options_hot", &self.options_hot.keys().collect::<Vec<_>>())
            .field(
                "options_cold",
                &self.options_cold.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// `POST /api/v1/repositories/_test`: open the repository these settings
/// describe, and save nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TestRepositoryDto {
    pub settings: RepositorySettingsDto,
}

/// `POST /api/v1/repositories`: test the repository, and save it under
/// `alias` once the test passed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConnectRepositoryDto {
    pub alias: String,
    pub settings: RepositorySettingsDto,
}

/// `POST /api/v1/repositories/_init`: create a repository where there is
/// none, and save it under `alias` when one is given.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct InitRepositoryDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub settings: RepositorySettingsDto,
}

/// What testing or connecting a repository answers: its ID, as rustic
/// knows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryIdDto {
    pub id: String,
}

/// What initialising a repository answers: its ID, and whether it was saved
/// into the configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryInitDto {
    pub id: String,
    pub saved: bool,
}

/// One repository's snapshots, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SnapshotsDto {
    pub snapshots: Vec<SnapshotDto>,
}

/// One recorded warning of an operation's detail: its typed code and the
/// free-form fields the engine reported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OperationWarningDto {
    pub code: WarningCode,
    /// The affected item, usually a path.
    pub item: Option<String>,
    /// What the engine was doing.
    pub during: String,
    /// The engine's own message, verbatim.
    pub detail: String,
}

/// One operation's detail: its summary and the warnings it recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OperationDetailDto {
    pub operation: OperationSummaryDto,
    pub warnings: Vec<OperationWarningDto>,
}

/// What a successful operation produced, as the history page shows it: a
/// small typed summary per kind, mapped from the app's engine result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum OperationResultDto {
    Backup {
        files: u64,
        bytes: u64,
        duration_ms: u64,
    },
    Prune {
        packs_deleted: u64,
        duration_ms: u64,
    },
    Check {
        errors: u64,
        warnings: u64,
        duration_ms: u64,
    },
    Forget {
        removed: u64,
        duration_ms: u64,
    },
    /// What an adoption produced: the snapshots rewritten.
    Adopt {
        rewritten: u64,
        duration_ms: u64,
    },
}

#[cfg(test)]
mod operation_dto_tests {
    use serde_json::{from_str, to_string};

    use super::{
        InvalidOperation, OperationDetailDto, OperationResultDto, OperationSummaryDto,
        OperationWarningDto, OperationsPageDto, WarningCode,
    };

    fn summary() -> OperationSummaryDto {
        OperationSummaryDto::new(
            "018f0000-0000-7000-8000-000000000001".to_owned(),
            "home".to_owned(),
            "backup".to_owned(),
            "succeeded".to_owned(),
            "2026-11-09T10:00:00Z".to_owned(),
            Some("2026-11-09T10:00:01Z".to_owned()),
            Some("2026-11-09T10:00:02Z".to_owned()),
            vec![WarningCode::SourceUnreadable],
            None,
            Some(OperationResultDto::Backup {
                files: 3,
                bytes: 4096,
                duration_ms: 1000,
            }),
        )
        .unwrap_or_else(|e| panic!("a valid summary: {e:?}"))
    }

    #[test]
    fn a_summary_round_trips_through_json() {
        let dto = summary();
        let json = to_string(&dto).unwrap();
        assert_eq!(from_str::<OperationSummaryDto>(&json).unwrap(), dto);
        assert!(
            json.contains("\"queued-at\""),
            "times are kebab-case on the wire: {json}"
        );
    }

    #[test]
    fn a_summary_refuses_an_id_that_is_not_a_uuid() {
        let dto = summary();
        assert_eq!(
            OperationSummaryDto::new(
                "nope".to_owned(),
                dto.plan,
                dto.kind,
                dto.status,
                dto.queued_at,
                dto.started_at,
                dto.finished_at,
                dto.warning_codes,
                dto.failure_code,
                dto.result,
            ),
            Err(InvalidOperation::Id)
        );
    }

    #[test]
    fn a_summary_refuses_an_id_of_the_wrong_length() {
        let dto = summary();
        // A digit short: the right shape, not a UUID.
        let short = "018f0000-0000-7000-8000-00000000000".to_owned();
        assert_eq!(
            OperationSummaryDto::new(
                short,
                dto.plan,
                dto.kind,
                dto.status,
                dto.queued_at,
                dto.started_at,
                dto.finished_at,
                dto.warning_codes,
                dto.failure_code,
                dto.result,
            ),
            Err(InvalidOperation::Id)
        );
    }

    #[test]
    fn a_summary_refuses_a_time_that_is_not_rfc_3339() {
        let dto = summary();
        assert_eq!(
            OperationSummaryDto::new(
                dto.id,
                dto.plan,
                dto.kind,
                dto.status,
                "not a time".to_owned(),
                dto.started_at,
                dto.finished_at,
                dto.warning_codes,
                dto.failure_code,
                dto.result,
            ),
            Err(InvalidOperation::Time)
        );
    }

    #[test]
    fn a_detail_round_trips_through_json() {
        let detail = OperationDetailDto {
            operation: summary(),
            warnings: vec![OperationWarningDto {
                code: WarningCode::SourceUnreadable,
                item: Some("/home/demo/lost".to_owned()),
                during: "backup".to_owned(),
                detail: "unreadable".to_owned(),
            }],
        };
        let json = to_string(&detail).unwrap();
        assert_eq!(from_str::<OperationDetailDto>(&json).unwrap(), detail);
    }

    #[test]
    fn a_page_round_trips_through_json() {
        let page = OperationsPageDto {
            operations: vec![summary()],
            next: Some("018f0000-0000-7000-8000-000000000000".to_owned()),
        };
        let json = to_string(&page).unwrap();
        assert_eq!(from_str::<OperationsPageDto>(&json).unwrap(), page);
    }

    #[test]
    fn a_result_names_its_kind_on_the_wire() {
        let results = [
            OperationResultDto::Backup {
                files: 3,
                bytes: 4096,
                duration_ms: 1000,
            },
            OperationResultDto::Prune {
                packs_deleted: 2,
                duration_ms: 1000,
            },
            OperationResultDto::Check {
                errors: 0,
                warnings: 1,
                duration_ms: 1000,
            },
        ];
        for result in results {
            let json = to_string(&result).unwrap();
            assert_eq!(from_str::<OperationResultDto>(&json).unwrap(), result);
        }
        assert_eq!(
            to_string(&OperationResultDto::Backup {
                files: 3,
                bytes: 4096,
                duration_ms: 1000
            })
            .unwrap(),
            r#"{"type":"backup","files":3,"bytes":4096,"duration-ms":1000}"#
        );
    }
}

#[cfg(test)]
mod catalog_dto_tests {
    use serde_json::{from_str, to_string};

    use super::{
        PasswordSourceDto, PlanSummaryDto, PlansDto, RepositoriesDto, RepositoryDto, SnapshotDto,
        SnapshotsDto,
    };

    #[test]
    fn the_catalog_lists_carry_their_own_field_names() {
        let plans = PlansDto {
            plans: vec![PlanSummaryDto {
                id: "demo-home".to_owned(),
                repository: "demo".to_owned(),
            }],
        };
        assert_eq!(
            to_string(&plans).unwrap(),
            r#"{"plans":[{"id":"demo-home","repository":"demo"}]}"#
        );
        assert_eq!(
            from_str::<PlansDto>(&to_string(&plans).unwrap()).unwrap(),
            plans
        );

        let repositories = RepositoriesDto {
            repositories: vec![RepositoryDto {
                alias: "demo".to_owned(),
                repository: "/srv/demo/repo".to_owned(),
                repo_hot: None,
                password: PasswordSourceDto::Inline,
            }],
        };
        assert_eq!(
            to_string(&repositories).unwrap(),
            r#"{"repositories":[{"alias":"demo","repository":"/srv/demo/repo","password":{"source":"inline"}}]}"#
        );
        assert_eq!(
            from_str::<RepositoriesDto>(&to_string(&repositories).unwrap()).unwrap(),
            repositories
        );
    }

    #[test]
    fn a_repository_row_names_where_its_password_comes_from_and_never_its_value() {
        for (source, wire) in [
            (PasswordSourceDto::Inline, r#"{"source":"inline"}"#),
            (
                PasswordSourceDto::File {
                    path: "/run/pw".to_owned(),
                },
                r#"{"source":"file","path":"/run/pw"}"#,
            ),
            (
                PasswordSourceDto::Keyring {
                    entry: "ladderback/nas".to_owned(),
                },
                r#"{"source":"keyring","entry":"ladderback/nas"}"#,
            ),
            (
                PasswordSourceDto::Env {
                    name: "PW".to_owned(),
                },
                r#"{"source":"env","name":"PW"}"#,
            ),
            (
                PasswordSourceDto::Command {
                    program: "pass".to_owned(),
                },
                r#"{"source":"command","program":"pass"}"#,
            ),
        ] {
            assert_eq!(to_string(&source).unwrap(), wire);
            assert_eq!(from_str::<PasswordSourceDto>(wire).unwrap(), source);
        }
        let row = RepositoryDto {
            alias: "nas".to_owned(),
            repository: "rest:https://u:***@nas.example/repo".to_owned(),
            repo_hot: Some("/mnt/hot".to_owned()),
            password: PasswordSourceDto::Inline,
        };
        assert_eq!(
            to_string(&row).unwrap(),
            r#"{"alias":"nas","repository":"rest:https://u:***@nas.example/repo","repo-hot":"/mnt/hot","password":{"source":"inline"}}"#
        );
    }

    #[test]
    fn a_repository_snapshot_list_round_trips() {
        let snapshot = SnapshotDto::new(
            "ab".repeat(32),
            "2026-11-09T10:15:00Z".to_owned(),
            "demo-host".to_owned(),
            "home".to_owned(),
            vec!["ladderback-plan:demo-home".to_owned()],
            vec!["/home/demo".to_owned()],
        )
        .unwrap_or_else(|e| panic!("a valid snapshot: {e:?}"));
        let list = SnapshotsDto {
            snapshots: vec![snapshot],
        };
        let wire = to_string(&list).unwrap();
        assert_eq!(from_str::<SnapshotsDto>(&wire).unwrap(), list);
    }
}

#[cfg(test)]
mod repository_request_tests {
    use serde_json::{from_str, json, to_string};

    use super::{
        ConnectRepositoryDto, ErrorCode, InitRepositoryDto, PasswordDto, RepositorySettingsDto,
        TestRepositoryDto,
    };

    fn settings() -> RepositorySettingsDto {
        RepositorySettingsDto {
            repository: "rest:https://user:hunter2@backup.example/repo".to_owned(),
            repo_hot: Some("/mnt/hot".to_owned()),
            password: PasswordDto::Inline {
                value: "correct-horse".to_owned(),
            },
            options: [("api-key".to_owned(), "s3cret".to_owned())].into(),
            options_hot: Default::default(),
            options_cold: Default::default(),
        }
    }

    #[test]
    fn settings_carry_the_configurations_own_field_names() {
        let wire = to_string(&settings()).unwrap();
        assert_eq!(
            from_str::<serde_json::Value>(&wire).unwrap(),
            json!({
                "repository": "rest:https://user:hunter2@backup.example/repo",
                "repo-hot": "/mnt/hot",
                "password": {"source": "inline", "value": "correct-horse"},
                "options": {"api-key": "s3cret"},
            })
        );
        assert_eq!(
            from_str::<RepositorySettingsDto>(&wire).unwrap(),
            settings()
        );
    }

    #[test]
    fn what_a_request_leaves_out_is_empty() {
        let read: RepositorySettingsDto =
            from_str(r#"{"repository":"/x","password":{"source":"env","name":"PW"}}"#).unwrap();
        assert_eq!(read.repo_hot, None);
        assert!(read.options.is_empty() && read.options_hot.is_empty());
        assert!(read.options_cold.is_empty());
        assert_eq!(read.password, PasswordDto::Env { name: "PW".into() });
    }

    #[test]
    fn every_password_source_the_api_takes_reads_back() {
        for (wire, expected) in [
            (
                r#"{"source":"inline","value":"p"}"#,
                PasswordDto::Inline { value: "p".into() },
            ),
            (
                r#"{"source":"file","path":"/run/pw"}"#,
                PasswordDto::File {
                    path: "/run/pw".into(),
                },
            ),
            (
                r#"{"source":"keyring","entry":"ladderback/nas"}"#,
                PasswordDto::Keyring {
                    entry: "ladderback/nas".into(),
                },
            ),
            (
                r#"{"source":"env","name":"PW"}"#,
                PasswordDto::Env { name: "PW".into() },
            ),
        ] {
            assert_eq!(from_str::<PasswordDto>(wire).unwrap(), expected, "{wire}");
            assert_eq!(
                from_str::<serde_json::Value>(&to_string(&expected).unwrap()).unwrap(),
                from_str::<serde_json::Value>(wire).unwrap()
            );
        }
    }

    #[test]
    fn a_body_the_route_does_not_take_is_not_read() {
        for wire in [
            // The command source runs a program: not over this API.
            r#"{"source":"command","command":"pass show x"}"#,
            r#"{"source":"inline"}"#,
            r#"{"source":"inline","value":"p","extra":1}"#,
            r#"{"value":"p"}"#,
            r#"{"source":"file","path":1}"#,
        ] {
            assert!(from_str::<PasswordDto>(wire).is_err(), "{wire}");
        }
        for wire in [
            r#"{"repository":"/x","password":{"source":"env","name":"PW"},"repo_hot":"/y"}"#,
            r#"{"repository":"/x"}"#,
            r#"{"password":{"source":"env","name":"PW"}}"#,
            r#"{"repository":"/x","password":{"source":"env","name":"PW"},"options":{"a":1}}"#,
        ] {
            assert!(from_str::<RepositorySettingsDto>(wire).is_err(), "{wire}");
        }
    }

    #[test]
    fn the_requests_name_their_parts() {
        let body = json!({"settings": settings()});
        assert_eq!(
            from_str::<TestRepositoryDto>(&body.to_string())
                .unwrap()
                .settings,
            settings()
        );
        assert!(from_str::<TestRepositoryDto>("{}").is_err());
        assert!(from_str::<TestRepositoryDto>(r#"{"settings":null}"#).is_err());

        let connect = json!({"alias": "demo", "settings": settings()});
        let connect: ConnectRepositoryDto = from_str(&connect.to_string()).unwrap();
        assert_eq!(connect.alias, "demo");
        assert!(
            from_str::<ConnectRepositoryDto>(&json!({"settings": settings()}).to_string()).is_err()
        );

        let init: InitRepositoryDto = from_str(&body.to_string()).unwrap();
        assert_eq!(init.alias, None);
        let init: InitRepositoryDto =
            from_str(&json!({"alias": "fresh", "settings": settings()}).to_string()).unwrap();
        assert_eq!(init.alias.as_deref(), Some("fresh"));
        // Unknown fields are refused at every level of a request.
        assert!(
            from_str::<InitRepositoryDto>(&json!({"settings": settings(), "x": 1}).to_string())
                .is_err()
        );
    }

    #[test]
    fn debug_shows_no_text_a_person_could_have_put_a_secret_in() {
        let shown = format!(
            "{:?} {:?}",
            settings(),
            ConnectRepositoryDto {
                alias: "demo".into(),
                settings: settings(),
            }
        );
        for secret in [
            "hunter2",
            "correct-horse",
            "s3cret",
            "backup.example",
            "/mnt/hot",
        ] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
        assert!(shown.contains("api-key"), "an option's name helps: {shown}");
    }

    #[test]
    fn debug_names_where_a_password_comes_from_but_never_a_typed_one() {
        let inline = format!(
            "{:?}",
            PasswordDto::Inline {
                value: "hunter2".into()
            }
        );
        assert!(!inline.contains("hunter2"), "{inline}");
        assert!(inline.contains("Inline"), "the source helps: {inline}");
        let file = format!(
            "{:?}",
            PasswordDto::File {
                path: "/etc/demo/pw".into()
            }
        );
        assert!(file.contains("/etc/demo/pw"), "a path is no secret: {file}");
    }

    #[test]
    fn a_name_that_is_not_an_alias_is_a_code_with_no_arguments() {
        assert_eq!(
            to_string(&ErrorCode::RepositoryAliasInvalid).unwrap(),
            r#""repository-alias-invalid""#
        );
        assert!(ErrorCode::RepositoryAliasInvalid.args().is_empty());
    }
}
