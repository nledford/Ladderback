//! What a row of the Operations page says (technical design §4.4): the name
//! its row header gives the operation, the label of its Cancel button, the
//! facts of what a run that succeeded produced, and how the page announces
//! the run's end. Which message, and with which values, is decided here, on
//! the host; `routes.rs` renders it.

use api::OperationSummaryDto;

use crate::announcer::Politeness;
use crate::wording::Words;

/// What a row's header names an operation by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Name {
    /// A backup, by its plan's ID as `config.toml` writes it, shown as it came.
    Plan(String),
    /// Anything else, by what ran and the repository it ran on.
    Words(Words),
}

/// The name of `operation`'s row: a backup by its plan, and a maintenance run
/// by what ran and the alias of its repository, or by what ran alone when the
/// log has no alias for it (a run an earlier version logged).
pub(crate) fn name(operation: &OperationSummaryDto) -> Name {
    let _ = operation;
    todo!()
}

/// The label of the button that cancels `operation`, named as its row is, so
/// each Cancel on the page says which run it stops (WCAG 2.4.6).
pub(crate) fn cancel_label(operation: &OperationSummaryDto) -> Words {
    let _ = operation;
    todo!()
}

/// The facts of what `operation` produced, in the order the Result cell
/// lists them; none for a run that has not succeeded. A fact the log did not
/// keep, such as the bytes a backup an earlier version ran went through, is
/// left out, never shown as zero.
pub(crate) fn result(operation: &OperationSummaryDto) -> Vec<Words> {
    let _ = operation;
    todo!()
}

/// How the end of `operation`, which the page saw still going, is announced:
/// as what ran, so the end of a check is not told as a backup's. A failure is
/// said assertively; a cancellation and a finish (warnings included: they are
/// the row's own story) politely.
pub(crate) fn end(operation: &OperationSummaryDto) -> (Politeness, Words) {
    let _ = operation;
    todo!()
}

#[cfg(test)]
mod tests {
    use api::{MsgArg, OperationResultDto};
    use l10n::{Localizer, MessageId, msg};

    use super::*;
    use crate::routes::text_with;

    fn summary(
        kind: &str,
        plan: &str,
        repository: Option<&str>,
        result: Option<OperationResultDto>,
    ) -> OperationSummaryDto {
        OperationSummaryDto {
            id: "018f0000-0000-7000-8000-000000000192".to_owned(),
            plan: plan.to_owned(),
            kind: kind.to_owned(),
            status: if result.is_some() { "succeeded" } else { "running" }.to_owned(),
            queued_at: "2026-11-09T10:00:00Z".to_owned(),
            started_at: Some("2026-11-09T10:00:01Z".to_owned()),
            finished_at: result
                .is_some()
                .then(|| "2026-11-09T10:01:06Z".to_owned()),
            warning_codes: Vec::new(),
            failure_code: None,
            result,
            repository: repository.map(str::to_owned),
        }
    }

    fn said(id: MessageId, args: &[(&'static str, MsgArg)]) -> Words {
        Words {
            id,
            args: args.to_vec(),
        }
    }

    fn text(value: &str) -> MsgArg {
        MsgArg::Text(value.to_owned())
    }

    /// What `words` read in English, without the isolation marks a reader
    /// never hears.
    fn english(words: &Words) -> String {
        text_with(&Localizer::new(&[]), words.id, &words.args)
            .replace(['\u{2068}', '\u{2069}'], "")
    }

    fn backed_up(snapshot: Option<&str>, bytes_processed: Option<u64>) -> OperationResultDto {
        OperationResultDto::Backup {
            files: 3,
            bytes: 4096,
            duration_ms: 65_000,
            snapshot: snapshot.map(str::to_owned),
            bytes_processed,
        }
    }

    #[test]
    fn a_backups_row_is_named_by_its_plan() {
        let backup = summary("backup", "home", None, None);
        assert_eq!(name(&backup), Name::Plan("home".to_owned()));
        assert_eq!(
            cancel_label(&backup),
            said(msg::UI_CANCEL_PLAN, &[("plan", text("home"))])
        );
    }

    #[test]
    fn a_maintenance_row_is_named_by_what_ran_and_the_alias_of_its_repository() {
        for kind in ["check", "prune", "forget", "adopt"] {
            let run = summary(kind, "", Some("demo"), None);
            let args = [("kind", text(kind)), ("repository", text("demo"))];
            assert_eq!(
                name(&run),
                Name::Words(said(msg::UI_OPERATION_OF, &args)),
                "{kind}"
            );
            assert_eq!(
                cancel_label(&run),
                said(msg::UI_CANCEL_OPERATION_OF, &args),
                "{kind}"
            );
        }
        let Name::Words(check) = name(&summary("check", "", Some("demo"), None)) else {
            panic!("a check is named in words");
        };
        assert_eq!(english(&check), "Check of demo");
    }

    #[test]
    fn a_row_the_log_names_no_repository_for_is_named_by_what_ran() {
        // A check an earlier version logged, before the alias was kept.
        let run = summary("check", "", None, None);
        assert_eq!(
            name(&run),
            Name::Words(said(msg::UI_OPERATION, &[("kind", text("check"))]))
        );
        assert_eq!(
            cancel_label(&run),
            said(msg::UI_CANCEL_OPERATION, &[("kind", text("check"))])
        );
        // A backup with no plan to name it by is named as a backup.
        let planless = summary("backup", "", None, None);
        assert_eq!(
            name(&planless),
            Name::Words(said(msg::UI_OPERATION, &[("kind", text("backup"))]))
        );
    }

    #[test]
    fn a_finished_backup_shows_its_snapshot_its_counts_and_how_long_it_took() {
        let snapshot = "1a2b3c4d".repeat(8);
        let backup = summary(
            "backup",
            "home",
            None,
            Some(backed_up(Some(&snapshot), Some(65_536))),
        );
        assert_eq!(
            result(&backup),
            [
                said(msg::UI_RESULT_SNAPSHOT, &[("snapshot", text("1a2b3c4d"))]),
                said(msg::UI_RESULT_FILES, &[("files", MsgArg::Count(3))]),
                said(msg::UI_RESULT_PROCESSED, &[("bytes", MsgArg::Bytes(65_536))]),
                said(msg::UI_RESULT_ADDED, &[("bytes", MsgArg::Bytes(4096))]),
                said(msg::UI_RESULT_TOOK, &[("duration", MsgArg::DurationMs(65_000))]),
            ]
        );
        assert_eq!(
            result(&backup).iter().map(english).collect::<Vec<_>>(),
            [
                "Snapshot 1a2b3c4d",
                "3 files",
                "64 KiB processed",
                "4 KiB added",
                "Took 1 min 5 s",
            ]
        );
    }

    #[test]
    fn a_backup_the_log_kept_less_of_shows_only_what_it_kept() {
        let old = summary("backup", "home", None, Some(backed_up(None, None)));
        assert_eq!(
            result(&old),
            [
                said(msg::UI_RESULT_FILES, &[("files", MsgArg::Count(3))]),
                said(msg::UI_RESULT_ADDED, &[("bytes", MsgArg::Bytes(4096))]),
                said(msg::UI_RESULT_TOOK, &[("duration", MsgArg::DurationMs(65_000))]),
            ]
        );
    }

    #[test]
    fn a_snapshot_id_is_cut_at_a_character_never_inside_one() {
        // Not what the daemon sends, but what the wire could carry: a cut
        // through a character would be a panic in the page.
        let odd = summary(
            "backup",
            "home",
            None,
            Some(backed_up(Some("ééééééééé"), None)),
        );
        assert_eq!(
            result(&odd).first(),
            Some(&said(msg::UI_RESULT_SNAPSHOT, &[("snapshot", text("éééééééé"))]))
        );
        // Shorter than the cut, it is shown whole; empty, not at all.
        let short = summary("backup", "home", None, Some(backed_up(Some("1a2b"), None)));
        assert_eq!(
            result(&short).first(),
            Some(&said(msg::UI_RESULT_SNAPSHOT, &[("snapshot", text("1a2b"))]))
        );
        let empty = summary("backup", "home", None, Some(backed_up(Some(""), None)));
        assert_eq!(
            result(&empty).first(),
            Some(&said(msg::UI_RESULT_FILES, &[("files", MsgArg::Count(3))]))
        );
    }

    #[test]
    fn a_check_that_found_nothing_says_so() {
        let check = summary(
            "check",
            "",
            Some("demo"),
            Some(OperationResultDto::Check {
                errors: 0,
                warnings: 0,
                duration_ms: 1_000,
            }),
        );
        let facts = result(&check);
        assert_eq!(
            facts,
            [
                said(msg::UI_RESULT_CHECK_CLEAN, &[]),
                said(msg::UI_RESULT_TOOK, &[("duration", MsgArg::DurationMs(1_000))]),
            ]
        );
        assert_eq!(english(&facts[0]), "The check found nothing wrong");
    }

    #[test]
    fn a_check_that_found_something_counts_only_what_it_found() {
        let found = |errors, warnings| {
            result(&summary(
                "check",
                "",
                Some("demo"),
                Some(OperationResultDto::Check {
                    errors,
                    warnings,
                    duration_ms: 1_000,
                }),
            ))
        };
        let took = said(msg::UI_RESULT_TOOK, &[("duration", MsgArg::DurationMs(1_000))]);
        let errors = |n| said(msg::UI_RESULT_CHECK_ERRORS, &[("errors", MsgArg::Count(n))]);
        let untidy = |n| said(msg::UI_RESULT_CHECK_UNTIDY, &[("warnings", MsgArg::Count(n))]);
        assert_eq!(found(2, 0), [errors(2), took.clone()]);
        assert_eq!(found(0, 1), [untidy(1), took.clone()]);
        assert_eq!(found(1, 3), [errors(1), untidy(3), took]);
    }

    #[test]
    fn a_prune_a_forget_and_an_adoption_count_what_they_did() {
        let took = said(msg::UI_RESULT_TOOK, &[("duration", MsgArg::DurationMs(2_000))]);
        for (kind, done, fact) in [
            (
                "prune",
                OperationResultDto::Prune {
                    packs_deleted: 4,
                    duration_ms: 2_000,
                },
                said(msg::UI_RESULT_PACKS_DELETED, &[("packs", MsgArg::Count(4))]),
            ),
            (
                "forget",
                OperationResultDto::Forget {
                    removed: 5,
                    duration_ms: 2_000,
                },
                said(msg::UI_RESULT_REMOVED, &[("removed", MsgArg::Count(5))]),
            ),
            (
                "adopt",
                OperationResultDto::Adopt {
                    rewritten: 6,
                    duration_ms: 2_000,
                },
                said(msg::UI_RESULT_ADOPTED, &[("rewritten", MsgArg::Count(6))]),
            ),
        ] {
            let run = summary(kind, "", Some("demo"), Some(done));
            assert_eq!(result(&run), [fact, took.clone()], "{kind}");
        }
    }

    #[test]
    fn a_run_without_a_result_shows_none() {
        assert_eq!(result(&summary("check", "", Some("demo"), None)), []);
    }

    /// A failed run's row says nothing in its Result: why it failed is the
    /// operation's detail, never the table's.
    #[test]
    fn a_failed_run_shows_no_result_and_no_failure() {
        let mut failed = summary("check", "", Some("demo"), None);
        "failed".clone_into(&mut failed.status);
        failed.finished_at = Some("2026-11-09T10:01:06Z".to_owned());
        failed.failure_code = Some(api::ErrorCode::EngineUnavailable);
        assert_eq!(result(&failed), []);
    }

    #[test]
    fn the_end_of_a_run_is_announced_as_what_ran() {
        let ended = |kind: &str, status: &str| {
            let mut run = summary(kind, "", Some("demo"), None);
            status.clone_into(&mut run.status);
            end(&run)
        };
        for kind in ["backup", "check", "prune", "forget", "adopt"] {
            let kind_arg = [("kind", text(kind))];
            for (status, politeness, id) in [
                ("succeeded", Politeness::Polite, msg::UI_OPERATION_FINISHED),
                (
                    "succeeded-with-warnings",
                    Politeness::Polite,
                    msg::UI_OPERATION_FINISHED,
                ),
                ("cancelled", Politeness::Polite, msg::UI_OPERATION_CANCELLED),
                ("failed", Politeness::Assertive, msg::UI_OPERATION_FAILED),
            ] {
                assert_eq!(
                    ended(kind, status),
                    (politeness, said(id, &kind_arg)),
                    "{kind} {status}"
                );
            }
        }
        let english_of = |kind, status| english(&ended(kind, status).1);
        assert_eq!(english_of("check", "succeeded"), "Check finished.");
        assert_eq!(english_of("prune", "failed"), "Prune failed.");
        assert_eq!(english_of("backup", "cancelled"), "Backup cancelled.");
    }
}
