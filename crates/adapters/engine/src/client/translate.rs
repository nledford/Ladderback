//! The anti-corruption layer for the worker's protocol: each event a worker
//! sends as the engine's, with the job's secrets masked (invariant I4).

use api::{ErrorCode, WarningCode};
use app::ports::{
    BackupSummary, CheckSummary, DumpAnswer, DumpHeader, EngineEvent, EngineResult, FolderAnswer,
    ForgetSummary, ProbeAnswer, ProgressEvent, ProgressKind as AppProgressKind, PrunePlanSummary,
    PruneSummary, RestorePlanSummary, RestoreSummary, RewriteSummary,
};
use domain::browse::{DisplayName, Entry, EntryName, FolderPage, PAGE_ENTRIES};
use domain::execution::{FailureReason, RepositoryHalf, Warning, WarningKind};
use domain::maintenance::RepositoryStats;
use domain::restore::download::{ArchiveFormat, DownloadShape};
use domain::secrets::Redactor;

use crate::job::{
    ArchiveSpec, DumpFound, DumpResult, FolderResult, Level, OpResult, ProbeResult, ProgressKind,
    TRUNCATED, WorkerEvent,
};
use crate::step::step_of;

/// The event a worker's line says, with the job's secrets masked in every
/// text of the worker's that reaches the daemon's log, the history or the
/// user. The worker masks its own output with what it is given, but not what
/// it can't know: the proxy credentials of its environment, and what it says
/// of a spec before it has read it. And it cuts a long text after it has
/// masked it, so a text it marked as cut ([`TRUNCATED`]) is masked as a cut
/// text: one of those secrets the cut went through leaves nothing behind.
/// A progress bar's text goes no further than here: the masked text names
/// the bar's step, when it is one of rustic's that [`step_of`] knows.
pub(super) fn translate(event: WorkerEvent, redactor: &Redactor) -> Option<EngineEvent> {
    Some(match event {
        WorkerEvent::Started {
            index_read_at,
            repository,
            ..
        } => EngineEvent::Started {
            index_read_at,
            repository,
        },
        WorkerEvent::ProgressStart { key, kind, prefix } => {
            EngineEvent::Progress(ProgressEvent::Begin {
                key,
                kind: match kind {
                    ProgressKind::Spinner => AppProgressKind::Spinner,
                    ProgressKind::Counter => AppProgressKind::Counter,
                    ProgressKind::Bytes => AppProgressKind::Bytes,
                },
                step: step_of(&redactor.redact_clipped(&prefix, TRUNCATED)),
            })
        }
        WorkerEvent::ProgressLength { key, len } => {
            EngineEvent::Progress(ProgressEvent::Length { key, len })
        }
        WorkerEvent::ProgressInc { key, inc } => {
            EngineEvent::Progress(ProgressEvent::Advance { key, by: inc })
        }
        WorkerEvent::ProgressTitle { key, title } => EngineEvent::Progress(ProgressEvent::Title {
            key,
            step: step_of(&redactor.redact_clipped(&title, TRUNCATED)),
        }),
        WorkerEvent::ProgressFinish { key } => EngineEvent::Progress(ProgressEvent::Finish { key }),
        WorkerEvent::Warning {
            item,
            during,
            code,
            detail,
        } => EngineEvent::Warning(Warning {
            kind: match code {
                WarningCode::SourceUnreadable => WarningKind::SourceUnreadable,
                WarningCode::HookWarning => WarningKind::Hook,
                WarningCode::MaintenancePartial => WarningKind::MaintenancePartial,
                WarningCode::CheckWarning => WarningKind::CheckWarning,
                WarningCode::CheckError => WarningKind::CheckError,
                WarningCode::MaintenanceOtherHosts => WarningKind::OtherHosts,
                // The daemon's choice of location says it, never a worker.
                WarningCode::LocationSkipped => WarningKind::LocationSkipped,
                WarningCode::SecretFileReadable => WarningKind::SecretFileReadable,
                // The profile codes belong to the import and export, which
                // run in the CLI; a worker that sent one is reported as the
                // problem it claims, in its own words.
                WarningCode::ProfileKeyIgnored
                | WarningCode::ProfilePrune
                | WarningCode::ProfileEnvIgnored
                | WarningCode::ProfileHooksPerPlan
                | WarningCode::ProfileUsedMissing
                | WarningCode::ProfileUsedNotRead
                | WarningCode::InlineSecret
                | WarningCode::ConfigInlineSecret
                | WarningCode::LocationSecret
                | WarningCode::GlobNeverMatches
                | WarningCode::ProfileTagDropped
                | WarningCode::FullDiskAccessMissing
                | WarningCode::ProfileOptionLeftOut
                | WarningCode::ProfileRetentionEmpty
                | WarningCode::ProfileSettingKept
                | WarningCode::ProfileKeyringCommandMissing
                | WarningCode::ProfileLocationsNotExported
                | WarningCode::ProfileCommandLeftOut
                | WarningCode::LocationPlainHttpCredentials
                | WarningCode::LocationEveryRuled
                | WarningCode::LocationLastRuledNoRetry
                | WarningCode::LocationRuleNameUnreadable
                | WarningCode::LocationRuleMixedLink
                // The daemon's notifier says it, never a worker.
                | WarningCode::NotificationUndelivered
                // Logged as a process starts, never an event.
                | WarningCode::CoreDumpsOn
                | WarningCode::DebuggerAttachAllowed
                // The Repositories page's note, never a worker's.
                | WarningCode::KeyringNotActivatable
                | WarningCode::Unknown => WarningKind::Other,
            },
            item: item.map(|item| redactor.redact_clipped(&item, TRUNCATED)),
            // A word of the worker's own, which it never cuts.
            during: redactor.redact(&during),
            detail: redactor.redact_clipped(&detail, TRUNCATED),
        }),
        // Logs are for the daemon's own log, which gets them under the target
        // `ladderback::worker`.
        WorkerEvent::Log { level, message } => {
            let level = match level {
                Level::Error => log::Level::Error,
                Level::Warn => log::Level::Warn,
                Level::Info => log::Level::Info,
                Level::Debug => log::Level::Debug,
            };
            log::log!(
                target: "ladderback::worker",
                level,
                "{}",
                redactor.redact_clipped(&message, TRUNCATED)
            );
            return None;
        }
        WorkerEvent::Result(result) => EngineEvent::Succeeded(match result {
            OpResult::Fake { steps_completed } => EngineResult::Fake { steps_completed },
            OpResult::Repository { id } => EngineResult::Repository { id },
            OpResult::Probed(found) => EngineResult::Probed(match found {
                ProbeResult::Holds { id } => ProbeAnswer::Holds(id),
                ProbeResult::Refused => ProbeAnswer::Refused,
                ProbeResult::NotARepository => ProbeAnswer::NotARepository,
                ProbeResult::PasswordNotAccepted => ProbeAnswer::PasswordNotAccepted,
            }),
            // A page the daemon can't take fails the read, as an event line
            // it can't read does.
            OpResult::Folder(found) => match folder_answer(found) {
                Ok(answer) => EngineResult::Folder(answer),
                Err(detail) => {
                    return Some(EngineEvent::Failed(FailureReason::EngineFailed { detail }));
                }
            },
            OpResult::Backup(r) => EngineResult::Backup(BackupSummary {
                repository: r.repository,
                snapshot: r.snapshot,
                parent: r.parent,
                files_new: r.files_new,
                files_changed: r.files_changed,
                files_unmodified: r.files_unmodified,
                bytes_processed: Some(r.bytes_processed),
                data_added: r.data_added,
                data_added_packed: r.data_added_packed,
                error_count: r.error_count,
            }),
            OpResult::Rewrite(r) => EngineResult::Rewrite(RewriteSummary {
                repository: r.repository,
                created: r.created,
            }),
            OpResult::Forget(r) => EngineResult::Forget(ForgetSummary {
                repository: r.repository,
                removed: r.removed,
            }),
            OpResult::Prune(r) => EngineResult::Prune(PruneSummary {
                repository: r.repository,
                packs_marked: r.packs_marked,
                packs_deleted: r.packs_deleted,
                packs_recovered: r.packs_recovered,
            }),
            OpResult::PrunePlan(r) => EngineResult::PrunePlan(PrunePlanSummary {
                repository: r.repository,
                packs_marked: r.packs_marked,
                packs_deleted: r.packs_deleted,
                packs_recovered: r.packs_recovered,
                bytes_deleted: r.bytes_deleted,
            }),
            OpResult::Check(r) => EngineResult::Check(CheckSummary {
                repository: r.repository,
                errors: r.errors,
                warnings: r.warnings,
            }),
            OpResult::Stats(r) => EngineResult::Stats(RepositoryStats {
                snapshots: r.snapshots,
                total_size: r.total_size,
                stored_size: r.stored_size,
                raw_size: r.raw_size,
            }),
            OpResult::Restore(r) => EngineResult::Restore(RestoreSummary {
                files_restored: r.files_restored,
                files_unchanged: r.files_unchanged,
                folders_restored: r.folders_restored,
                bytes_restored: r.bytes_restored,
            }),
            OpResult::RestorePlan(r) => EngineResult::RestorePlan(RestorePlanSummary {
                files_added: r.files_added,
                files_overwritten: r.files_overwritten,
                files_unchanged: r.files_unchanged,
                files_kept: r.files_kept,
                bytes_to_write: r.bytes_to_write,
            }),
            OpResult::Dump(r) => EngineResult::Dump(match r {
                DumpResult::Done { bytes } => DumpAnswer::Done { bytes },
                DumpResult::SnapshotUnknown => DumpAnswer::SnapshotUnknown,
                DumpResult::EntryUnknown => DumpAnswer::EntryUnknown,
            }),
            // The listing's end, which `Listing` turns into the list before
            // an event reaches here.
            OpResult::Snapshots { .. } => return None,
        }),
        WorkerEvent::Snapshots { .. } => return None,
        WorkerEvent::Dumping { found, name, host } => EngineEvent::Dumping(DumpHeader {
            shape: match found {
                DumpFound::File { size } => DownloadShape::File { size },
                DumpFound::Archive { archive } => DownloadShape::Archive(match archive {
                    ArchiveSpec::Tar => ArchiveFormat::Tar,
                    ArchiveSpec::Zip => ArchiveFormat::Zip,
                }),
            },
            name: name.map(|name| DisplayName::new(&name)),
            host,
        }),
        // `Reading` decodes a piece's bytes before an event reaches here.
        WorkerEvent::Piece { .. } => return None,
        WorkerEvent::Failed { code, args, detail } => {
            let detail = redactor.redact_clipped(&detail.unwrap_or_default(), TRUNCATED);
            EngineEvent::Failed(match code {
                ErrorCode::InvalidJobSpec => FailureReason::JobRejected { detail },
                ErrorCode::DeadlinePassed => FailureReason::DeadlinePassed,
                ErrorCode::RestoreTargetUnwritable => FailureReason::RestoreTargetUnwritable,
                // A repository that already exists where one was to be
                // initialised, and a location with no repository at all
                // (decision 53), are the failures the agent reports as codes
                // of their own: `InitRepository` refuses with the first, and
                // a saved repository's row offers Create on the second.
                // Nothing was written. The job's spec already says where.
                ErrorCode::RepositoryExists => FailureReason::RepositoryExists,
                // The location holds another repository than the one the
                // spec named (§7.3): nothing there was read or written. As
                // with `RepositoryExists`, the code says it all.
                ErrorCode::RepositoryReplaced => FailureReason::RepositoryReplaced,
                ErrorCode::RepositoryNotFound => FailureReason::RepositoryNotFound,
                // A server refused the credentials (decision 85): nothing was
                // read or written. The worker names the half whose server
                // refused, for a hot/cold repository; a name that is no
                // half's names none.
                ErrorCode::CredentialsRefused => FailureReason::CredentialsRefused {
                    half: args.iter().find_map(|(name, arg)| match arg {
                        api::MsgArg::Text(half) if name.as_str() == "half" => {
                            RepositoryHalf::parse(half)
                        }
                        _ => None,
                    }),
                },
                // No config file where the engine looked, and nothing tells
                // an empty place from one out of reach (#525): the engine's
                // words, masked, say where it looked.
                ErrorCode::RepositoryConfigNotFound => {
                    FailureReason::RepositoryConfigNotFound { detail }
                }
                // No key opened with the repository's password (#481): the
                // code a repository of several locations fails with too.
                ErrorCode::PasswordNotAccepted => FailureReason::PasswordNotAccepted,
                // A worker only ever says one of the codes above or
                // `operation-failed`. The daemon's own codes are the log's
                // words for what the daemon saw, the request-check codes
                // belong to the HTTP adapter, and the configuration codes to
                // the config store: a worker that sent one has failed in a way
                // this side doesn't know, so it is reported as the failure it
                // claims, in its own words.
                ErrorCode::OperationFailed
                | ErrorCode::CreateNotOffered
                | ErrorCode::EngineUnavailable
                | ErrorCode::RepositoryAliasTaken
                | ErrorCode::RepositoryAlreadyConfigured
                | ErrorCode::RepositoryAliasInvalid
                | ErrorCode::SecretMissing
                | ErrorCode::SecretUnreadable
                | ErrorCode::SecretKeyringUnavailable
                | ErrorCode::SecretKeyringLocked
                | ErrorCode::SecretNotText
                | ErrorCode::SecretTooLarge
                | ErrorCode::SecretCommandFailed
                | ErrorCode::SecretCommandOutputOpen
                | ErrorCode::SecretCommandTimedOut
                | ErrorCode::SecretKeyringTimedOut
                | ErrorCode::SecretFileTimedOut
                | ErrorCode::SecretInProcessArguments
                | ErrorCode::SecretExpansionTooLong
                | ErrorCode::SecretsTooLargeToMask
                | ErrorCode::TemplateInvalid
                | ErrorCode::LocationTooManyCredentials
                | ErrorCode::RepositoryPathRelative
                | ErrorCode::KeyringUnavailable
                | ErrorCode::KeyringLocked
                | ErrorCode::KeyringUnwritable
                | ErrorCode::KeyringEntryTaken
                | ErrorCode::EngineExited
                | ErrorCode::ProtocolViolated
                | ErrorCode::EngineStartTimedOut
                | ErrorCode::EngineExitTimedOut
                | ErrorCode::RepositoryTimedOut
                | ErrorCode::DaemonStopped
                | ErrorCode::DaemonShutDown
                | ErrorCode::NoLocationUsable
                | ErrorCode::NoLocationRuleHolds
                | ErrorCode::HostNotAllowed
                | ErrorCode::OriginNotAllowed
                | ErrorCode::JsonRequired
                | ErrorCode::SignInRequired
                | ErrorCode::SignInFailed
                | ErrorCode::SignInWait
                | ErrorCode::CsrfTokenInvalid
                | ErrorCode::SignInUnavailable
                | ErrorCode::ApiTokenInvalid
                | ErrorCode::ApiTokenScope
                | ErrorCode::SignInOff
                | ErrorCode::RestartUnavailable
                | ErrorCode::RestartPending
                | ErrorCode::ApiTokenLabel
                | ErrorCode::ApiTokenNoScope
                | ErrorCode::ApiTokenLimit
                | ErrorCode::ApiTokenUnknown
                | ErrorCode::ApiTokensUnavailable
                | ErrorCode::AdminPasswordTooShort
                | ErrorCode::AdminPasswordTooLong
                | ErrorCode::CurrentPasswordWrong
                | ErrorCode::AdminPasswordUnavailable
                | ErrorCode::SessionsUnavailable
                | ErrorCode::DaemonUnreachable
                | ErrorCode::DaemonAnswerLost
                | ErrorCode::DaemonAnswerTooLarge
                | ErrorCode::ConfigUnavailable
                | ErrorCode::ConfigTooLarge
                | ErrorCode::ConfigInvalid
                | ErrorCode::ConfigSchemaMissing
                | ErrorCode::ConfigSchemaNewer
                | ErrorCode::PlanUnknown
                | ErrorCode::PlanInvalid
                | ErrorCode::PlanInUse
                | ErrorCode::KeyFilesNotLeftOut
                | ErrorCode::ProposalEmpty
                | ErrorCode::ProposalInvalid
                | ErrorCode::ProposalTooLarge
                | ErrorCode::ProposalLimit
                | ErrorCode::ProposalUnknown
                | ErrorCode::ProposalNotPending
                | ErrorCode::ProposalChanged
                | ErrorCode::ProposalNeedsForgetPreview
                | ErrorCode::ProposalForgetPreviewChanged
                | ErrorCode::ProposalRate
                | ErrorCode::ProposalAfterRejection
                | ErrorCode::ProposalReasons
                | ErrorCode::ProposalsUnavailable
                | ErrorCode::DryRunBusy
                | ErrorCode::DryRunGivesWay
                | ErrorCode::DryRunRate
                | ErrorCode::DryRunNotAllowed
                | ErrorCode::DryRunReadsAbove
                | ErrorCode::DryRunFilter
                | ErrorCode::DryRunFailed
                | ErrorCode::DryRunFolderInvalid
                | ErrorCode::DryRunAllowlistTooLong
                | ErrorCode::RepositoryUnknown
                | ErrorCode::SnapshotUnknown
                | ErrorCode::FolderUnknown
                | ErrorCode::MaintenanceBusy
                | ErrorCode::MaintenanceNotOwner
                | ErrorCode::RetentionKeepsNothing
                | ErrorCode::MaintenanceKindUnknown
                | ErrorCode::AdoptionNoMatch
                | ErrorCode::AdoptionAmbiguous
                | ErrorCode::ProfileUnreadable
                | ErrorCode::ProfileTooLarge
                | ErrorCode::ProfileInvalid
                | ErrorCode::ProfileUnsupported
                | ErrorCode::PlanIdTaken
                | ErrorCode::InlineSecret
                | ErrorCode::ProfileExportReference
                | ErrorCode::ProfileExportUrlPassword
                | ErrorCode::ProfileExportTagComma
                | ErrorCode::MaintenanceSelectionChanged
                | ErrorCode::RepositoryReadTimedOut
                | ErrorCode::OperationUnknown
                | ErrorCode::OperationNotRunning
                | ErrorCode::InvalidCursor
                | ErrorCode::InvalidArguments
                | ErrorCode::InvalidBody
                | ErrorCode::BodyTooLarge
                | ErrorCode::ConfigFolderUnknown
                | ErrorCode::RuntimeUnavailable
                | ErrorCode::StepUpFailed
                | ErrorCode::RestoreRunning
                | ErrorCode::RestoreInPlaceNotHere
                | ErrorCode::RestoreHubNotAllowed
                | ErrorCode::RestoreFolderUnset
                | ErrorCode::FullDiskAccessNotLocal
                | ErrorCode::FullDiskAccessSettingsUnopened
                | ErrorCode::DownloadsBusy
                | ErrorCode::DownloadsUnavailable
                | ErrorCode::EntryUnknown
                | ErrorCode::DownloadUnknown
                | ErrorCode::RouteUnknown
                | ErrorCode::TokenUsed
                | ErrorCode::TokenExpired
                | ErrorCode::TokenRevoked
                | ErrorCode::TokenMalformed
                | ErrorCode::TokenUnsupported
                | ErrorCode::HostnameTaken
                | ErrorCode::AgentKeyMismatch
                | ErrorCode::HubKeyMismatch
                | ErrorCode::HubProofInvalid
                | ErrorCode::ProofRefused
                | ErrorCode::PendingLimit
                | ErrorCode::BindingUnavailable
                | ErrorCode::HubUnreachable
                | ErrorCode::HubPinMismatch
                | ErrorCode::HubUntrusted
                | ErrorCode::HubTlsRefused
                | ErrorCode::PlainAddress
                | ErrorCode::HubConnectionLost
                | ErrorCode::Unknown => FailureReason::EngineFailed { detail },
            })
        }
    })
}

/// A browse's answer as the daemon takes it: each name parsed as an entry
/// name, and a page of no more than a page's entries (an input-derived
/// size, §13.5). Anything else is the worker's fault, said in the detail.
fn folder_answer(found: FolderResult) -> Result<FolderAnswer, String> {
    let (entries, total, next) = match found {
        FolderResult::SnapshotUnknown => return Ok(FolderAnswer::SnapshotUnknown),
        FolderResult::FolderUnknown => return Ok(FolderAnswer::FolderUnknown),
        FolderResult::Page {
            entries,
            total,
            next,
        } => (entries, total, next),
    };
    if entries.len() > PAGE_ENTRIES {
        return Err(format!(
            "the worker sent a page of {} entries, more than {PAGE_ENTRIES}",
            entries.len()
        ));
    }
    let entries = entries
        .into_iter()
        .map(|entry| {
            Ok(Entry {
                name: EntryName::parse(&entry.name)
                    .map_err(|error| format!("the worker sent a page in which {error}"))?,
                display: entry.display,
                kind: entry.kind,
                size: entry.size,
                modified: entry.modified,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(FolderAnswer::Page(FolderPage {
        entries,
        total,
        next,
    }))
}

#[cfg(test)]
mod tests {
    use std::assert_matches;
    use std::time::Duration;

    use api::MsgArg;
    use app::ports::{FakeEnd, FakeJob, Job, OperationStep};
    use domain::execution::RepositoryHalf;

    use super::*;
    use crate::client::job_kind::job_kind;
    use crate::client::test_support::*;
    use crate::job::JobKind;

    fn entry_named(name: &str) -> crate::job::EntryResult {
        crate::job::EntryResult {
            name: name.to_owned(),
            display: domain::browse::DisplayName::new(name),
            kind: domain::browse::EntryKind::File,
            size: Some(1),
            modified: None,
        }
    }

    #[test]
    fn a_folder_page_is_taken_with_its_entries_and_where_the_next_starts() {
        let answer = folder_answer(FolderResult::Page {
            entries: vec![entry_named("readme.txt")],
            total: 1_001,
            next: Some(1_000),
        });
        let Ok(FolderAnswer::Page(page)) = answer else {
            panic!("a page: {answer:?}");
        };
        assert_eq!(page.entries[0].name.as_str(), "readme.txt");
        assert_eq!(page.entries[0].size, Some(1));
        assert_eq!((page.total, page.next), (1_001, Some(1_000)));
    }

    #[test]
    fn a_snapshot_or_folder_that_is_not_there_is_taken_as_such() {
        assert_eq!(
            folder_answer(FolderResult::SnapshotUnknown),
            Ok(FolderAnswer::SnapshotUnknown)
        );
        assert_eq!(
            folder_answer(FolderResult::FolderUnknown),
            Ok(FolderAnswer::FolderUnknown)
        );
    }

    #[test]
    fn a_page_with_a_name_no_entry_has_is_refused() {
        for name in ["", "a/b"] {
            let answer = folder_answer(FolderResult::Page {
                entries: vec![entry_named(name)],
                total: 1,
                next: None,
            });
            assert!(answer.is_err(), "{name:?}: {answer:?}");
        }
    }

    #[test]
    fn a_page_longer_than_a_page_holds_is_refused() {
        let most = domain::browse::PAGE_ENTRIES;
        let entries = |n: usize| (0..n).map(|at| entry_named(&format!("f{at}"))).collect();
        let fits = folder_answer(FolderResult::Page {
            entries: entries(most),
            total: most as u64,
            next: None,
        });
        assert!(fits.is_ok(), "{fits:?}");
        let over = folder_answer(FolderResult::Page {
            entries: entries(most + 1),
            total: most as u64 + 1,
            next: None,
        });
        assert!(over.is_err());
    }

    fn failed(code: ErrorCode) -> WorkerEvent {
        WorkerEvent::Failed {
            code,
            args: Vec::new(),
            detail: Some("bad spec".to_owned()),
        }
    }

    #[test]
    fn a_worker_that_rejects_its_job_spec_rejects_the_job() {
        assert_eq!(
            translate(failed(ErrorCode::InvalidJobSpec), &Redactor::default()),
            Some(EngineEvent::Failed(FailureReason::JobRejected {
                detail: "bad spec".to_owned()
            }))
        );
    }

    #[test]
    fn other_worker_failures_carry_the_engine_message_verbatim() {
        for code in [
            ErrorCode::OperationFailed,
            ErrorCode::EngineUnavailable,
            ErrorCode::EngineExited,
            ErrorCode::ProtocolViolated,
            ErrorCode::EngineStartTimedOut,
            ErrorCode::EngineExitTimedOut,
            ErrorCode::DaemonStopped,
            ErrorCode::DaemonShutDown,
            ErrorCode::Unknown,
        ] {
            assert_eq!(
                translate(failed(code), &Redactor::default()),
                Some(EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "bad spec".to_owned()
                }))
            );
        }
    }

    #[test]
    fn a_deadline_failure_from_the_worker_is_a_passed_deadline() {
        assert_eq!(
            translate(failed(ErrorCode::DeadlinePassed), &Redactor::default()),
            Some(EngineEvent::Failed(FailureReason::DeadlinePassed))
        );
    }

    /// A refusal of the credentials names the half its worker named, and
    /// none when it named none or a half there is no such name for.
    #[test]
    fn a_refusal_of_the_credentials_keeps_the_half_the_worker_named() {
        for (said, half) in [
            (
                Some(MsgArg::Text("hot".to_owned())),
                Some(RepositoryHalf::Hot),
            ),
            (
                Some(MsgArg::Text("cold".to_owned())),
                Some(RepositoryHalf::Cold),
            ),
            (None, None),
            (Some(MsgArg::Text("warm".to_owned())), None),
            (Some(MsgArg::Count(1)), None),
        ] {
            let event = WorkerEvent::Failed {
                code: ErrorCode::CredentialsRefused,
                args: said
                    .clone()
                    .map(|arg| ("half".to_owned(), arg))
                    .into_iter()
                    .collect(),
                detail: Some("the server answered 401".to_owned()),
            };
            assert_eq!(
                translate(event, &Redactor::default()),
                Some(EngineEvent::Failed(FailureReason::CredentialsRefused {
                    half
                })),
                "{said:?}"
            );
        }
    }

    /// No repository config found keeps the engine's words, masked of the
    /// job's secrets: they name where it looked (#525).
    #[test]
    fn no_repository_config_found_keeps_the_engines_masked_words() {
        let redactor = redactor_of(&["hunter2-demo"]);
        assert_eq!(
            translate(
                failed_saying(
                    ErrorCode::RepositoryConfigNotFound,
                    "No repository config file found for `rest:http://ann:hunter2-demo@nas/`."
                ),
                &redactor
            ),
            Some(EngineEvent::Failed(
                FailureReason::RepositoryConfigNotFound {
                    detail: "No repository config file found for `rest:http://ann:***@nas/`."
                        .to_owned()
                }
            ))
        );
    }

    /// The worker rejects a spec before it has the job's secrets to mask
    /// with, and a deserialization error can quote the value it choked on.
    #[test]
    fn what_a_rejected_job_spec_quotes_of_the_jobs_secrets_is_masked() {
        let redactor = redactor_of(&["hunter2-demo"]);
        assert_eq!(
            translate(
                failed_saying(
                    ErrorCode::InvalidJobSpec,
                    "invalid type: string \"hunter2-demo\", expected u64 at line 1 column 90"
                ),
                &redactor
            ),
            Some(EngineEvent::Failed(FailureReason::JobRejected {
                detail: "invalid type: string \"***\", expected u64 at line 1 column 90".to_owned()
            }))
        );
    }

    #[test]
    fn what_any_other_failure_says_of_the_jobs_secrets_is_masked_too() {
        let redactor = redactor_of(&["hunter2-demo"]);
        for code in [
            ErrorCode::OperationFailed,
            ErrorCode::EngineUnavailable,
            ErrorCode::EngineExited,
            ErrorCode::ProtocolViolated,
            ErrorCode::EngineStartTimedOut,
            ErrorCode::EngineExitTimedOut,
            ErrorCode::DaemonStopped,
            ErrorCode::DaemonShutDown,
            ErrorCode::Unknown,
        ] {
            assert_eq!(
                translate(
                    failed_saying(code, "could not open with hunter2-demo"),
                    &redactor
                ),
                Some(EngineEvent::Failed(FailureReason::EngineFailed {
                    detail: "could not open with ***".to_owned()
                })),
                "{code:?}"
            );
        }
    }

    #[test]
    fn a_warning_hides_the_jobs_secrets_in_every_text_it_carries() {
        let redactor = redactor_of(&["hunter2-demo"]);
        let warning = WorkerEvent::Warning {
            item: Some("/home/demo/hunter2-demo.txt".to_owned()),
            during: "reading hunter2-demo".to_owned(),
            code: WarningCode::SourceUnreadable,
            detail: "denied for hunter2-demo".to_owned(),
        };
        assert_eq!(
            translate(warning, &redactor),
            Some(EngineEvent::Warning(Warning {
                kind: WarningKind::SourceUnreadable,
                item: Some("/home/demo/***.txt".to_owned()),
                during: "reading ***".to_owned(),
                detail: "denied for ***".to_owned(),
            }))
        );
    }

    #[test]
    fn a_warning_without_an_item_stays_without_one() {
        let warning = WorkerEvent::Warning {
            item: None,
            during: "scan".to_owned(),
            code: WarningCode::Unknown,
            detail: "odd".to_owned(),
        };
        assert_eq!(
            translate(warning, &redactor_of(&["hunter2-demo"])),
            Some(EngineEvent::Warning(Warning {
                kind: WarningKind::Other,
                item: None,
                during: "scan".to_owned(),
                detail: "odd".to_owned(),
            }))
        );
    }

    /// What a check found comes back as the kind of its level, and a fake
    /// job that plays one sends the code it came as.
    #[test]
    fn a_checks_findings_keep_their_level_both_ways() {
        for (code, kind) in [
            (WarningCode::CheckWarning, WarningKind::CheckWarning),
            (WarningCode::CheckError, WarningKind::CheckError),
        ] {
            let found = WorkerEvent::Warning {
                item: None,
                during: "check".to_owned(),
                code,
                detail: "pack 5ca1ab1e not referenced in index".to_owned(),
            };
            assert_eq!(
                translate(found, &redactor_of(&["hunter2-demo"])),
                Some(EngineEvent::Warning(Warning {
                    kind,
                    item: None,
                    during: "check".to_owned(),
                    detail: "pack 5ca1ab1e not referenced in index".to_owned(),
                }))
            );
            let played = job_kind(Job::Fake(FakeJob {
                steps: 0,
                step_delay: Duration::ZERO,
                warnings: vec![Warning {
                    kind,
                    item: None,
                    during: "check".to_owned(),
                    detail: String::new(),
                }],
                end: FakeEnd::Succeed,
                deadline_budget: None,
            }));
            let Ok(JobKind::Fake(spec)) = played else {
                panic!("not a fake job: {played:?}");
            };
            assert_eq!(
                spec.warnings.iter().map(|w| w.code).collect::<Vec<_>>(),
                [code]
            );
        }
    }

    /// A progress bar's text is rustic's, and what leaves the adapter is the
    /// step it names, never the text: a text that carries a secret names no
    /// step, so nothing of it goes on (I4). The masked text is what names
    /// the step, so one of rustic's own texts a mask changed names none.
    #[test]
    fn a_progress_text_carrying_a_secret_is_never_forwarded_as_text() {
        let redactor = redactor_of(&["CANARY-step-secret", "unindexed"]);
        for (text, step) in [
            ("reading index...", Some(OperationStep::ReadingIndex)),
            ("reading index... CANARY-step-secret", None),
            ("CANARY-step-secret", None),
            ("removing unindexed packs...", None),
            (
                "removing old data packs...",
                Some(OperationStep::DeletingUnusedData),
            ),
        ] {
            let begun = translate(
                WorkerEvent::ProgressStart {
                    key: 1,
                    kind: ProgressKind::Bytes,
                    prefix: text.to_owned(),
                },
                &redactor,
            );
            assert_eq!(
                begun,
                Some(EngineEvent::Progress(ProgressEvent::Begin {
                    key: 1,
                    kind: AppProgressKind::Bytes,
                    step,
                })),
                "{text:?}"
            );
            let titled = translate(
                WorkerEvent::ProgressTitle {
                    key: 1,
                    title: text.to_owned(),
                },
                &redactor,
            );
            assert_eq!(
                titled,
                Some(EngineEvent::Progress(ProgressEvent::Title { key: 1, step })),
                "{text:?}"
            );
            let told = format!("{begun:?} {titled:?}");
            assert!(!told.contains("CANARY"), "{told}");
        }
    }

    /// The worker can refuse a spec before it has the job's secrets to mask
    /// with, and what it says then can quote one.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_rejected_job_spec_that_quotes_the_jobs_password_is_masked_in_the_failure() {
        let rejection = failed_saying(
            ErrorCode::InvalidJobSpec,
            "invalid type: string \"hunter2-demo\", expected a map",
        );
        let events = answered(
            identify("hunter2-demo", &[]),
            None,
            "rejecting",
            &[line_of(&rejection)],
        )
        .await;
        assert_eq!(
            events,
            [EngineEvent::Failed(FailureReason::JobRejected {
                detail: "invalid type: string \"***\", expected a map".to_owned()
            })]
        );
    }

    /// What a worker logs goes to the daemon's log as it says it.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_log_line_from_the_worker_hides_the_jobs_secrets() {
        capture_the_worker_log();
        let log_line = WorkerEvent::Log {
            level: Level::Info,
            message: "log one: hunter2-demo".to_owned(),
        };
        let events = answered(
            identify("hunter2-demo", &[]),
            None,
            "logging",
            &[line_of(&log_line), result_line()],
        )
        .await;
        assert_matches!(events.as_slice(), [EngineEvent::Succeeded(_)]);
        let line = logged_soon(log::Level::Info, |text| text.starts_with("log one:"))
            .await
            .expect("the worker's log line never reached the log");
        assert_eq!(line, "log one: ***");
    }
}
