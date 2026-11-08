//! Codes and their messages (technical design §8.4, "Errors cross the hexagon
//! as codes"): every code has one message, every code message has a code, a
//! message reads the arguments its code declares, and the codes render.

use std::collections::BTreeSet;

use api::{ArgKind, ArgSpec, ErrorCode, MsgArg, WarningCode};
use l10n::Localizer;
use l10n::catalog::{Catalog, Source};
use l10n::code::{ERRORS, WARNINGS, error_message, warning_message};

use crate::support::catalog::{
    argument_mismatches, codes_without_message, messages_without_code, translation_problems,
};
use crate::support::{english, locale, sample, source_locale, tag};

fn error_wire(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|wire| wire.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn warning_wire(code: WarningCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|wire| wire.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Every code's message ID, errors then warnings.
fn mapped() -> Vec<&'static str> {
    let errors = ERRORS.iter().map(|(_, id)| id.as_str());
    let warnings = WARNINGS.iter().map(|(_, id)| id.as_str());
    errors.chain(warnings).collect()
}

/// A code's arguments, each with a representative value.
fn args_for(declared: &[ArgSpec]) -> Vec<(&'static str, MsgArg)> {
    declared
        .iter()
        .map(|spec| (spec.name, sample(spec.kind)))
        .collect()
}

#[test]
fn a_codes_message_is_named_after_its_wire_name_and_no_message_serves_two_codes() {
    assert!(!ERRORS.is_empty() && !WARNINGS.is_empty());
    for (code, id) in ERRORS {
        assert_eq!(id.as_str(), format!("error-{}", error_wire(*code)));
        assert_eq!(error_message(*code), *id);
    }
    for (code, id) in WARNINGS {
        assert_eq!(id.as_str(), format!("warning-{}", warning_wire(*code)));
        assert_eq!(warning_message(*code), *id);
    }
    let distinct: BTreeSet<&str> = mapped().into_iter().collect();
    assert_eq!(distinct.len(), ERRORS.len() + WARNINGS.len());
}

#[test]
fn every_code_has_a_message_in_the_source_catalog() {
    assert!(!mapped().is_empty());
    let problems = codes_without_message(&source_locale(), &mapped());
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn every_code_message_of_the_source_catalog_has_a_code() {
    assert!(!mapped().is_empty());
    let problems = messages_without_code(&source_locale(), &mapped());
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn every_message_reads_the_arguments_its_code_declares() {
    let source = source_locale();
    assert!(!ERRORS.is_empty());
    let declared = ERRORS
        .iter()
        .map(|(code, id)| (code.args(), id))
        .chain(WARNINGS.iter().map(|(code, id)| (code.args(), id)));
    for (args, id) in declared {
        let message = source
            .messages
            .get(id.as_str())
            .unwrap_or_else(|| panic!("`{}` has a message", id.as_str()));
        let problems = argument_mismatches(id.as_str(), args, message);
        assert!(problems.is_empty(), "{problems:#?}");
    }
}

#[test]
fn a_code_with_no_message_is_reported() {
    let catalog = locale("error-operation-failed = The operation failed.\n");
    let mapped = ["error-operation-failed", "error-deadline-passed"];
    let problems = codes_without_message(&catalog, &mapped);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(problems[0].contains("error-deadline-passed"));
    assert!(codes_without_message(&catalog, &mapped[..1]).is_empty());
}

#[test]
fn a_message_with_no_code_is_reported() {
    let catalog = locale(
        "error-operation-failed = The operation failed.\n\
         error-forgotten = Nothing renders this.\n\
         warning-forgotten = Nor this.\n\
         plan-run-now = Run now\n",
    );
    let problems = messages_without_code(&catalog, &["error-operation-failed"]);
    assert_eq!(problems.len(), 2, "{problems:#?}");
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("error-forgotten"))
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("warning-forgotten"))
    );
}

#[test]
fn an_argument_mismatch_is_reported() {
    const FILES: ArgSpec = ArgSpec {
        name: "files",
        kind: ArgKind::Count,
        optional: false,
    };
    const PATH: ArgSpec = ArgSpec {
        name: "path",
        kind: ArgKind::Text,
        optional: false,
    };
    const EXIT: ArgSpec = ArgSpec {
        name: "exit_code",
        kind: ArgKind::Text,
        optional: true,
    };
    // (declared, the message, what the report must name; empty for none)
    let cases: [(&[ArgSpec], &str, &str); 14] = [
        (&[FILES], "error-x = { $files } files\n", ""),
        (
            &[FILES],
            "error-x = { NUMBER($files) ->\n    [one] one file\n   *[other] many\n  }\n    .with-detail = { $files }: { $detail }\n",
            "",
        ),
        (&[FILES], "error-x = { $files } of { $total }\n", "total"),
        (&[FILES, PATH], "error-x = { $files } files\n", "path"),
        (&[PATH], "error-x = { NUMBER($path) }\n", "NUMBER"),
        (&[], "error-x = Failed: { $detail }\n", "detail"),
        (
            &[],
            "error-x = Failed.\n    .with-detail = Failed again.\n",
            "detail",
        ),
        (
            &[],
            "error-x = Failed.\n    .with-detail = { $detail } { $other }\n",
            "other",
        ),
        (
            &[EXIT],
            "error-x = { $exit_code ->\n    [none] no exit status\n   *[other] exit status { $exit_code }\n  }\n",
            "",
        ),
        (&[EXIT], "error-x = exit status { $exit_code }\n", "none"),
        // Each text of the message is checked, not only its value.
        (
            &[EXIT],
            "error-x = { $exit_code ->\n    [none] stopped\n   *[other] exit status { $exit_code }\n  }\n    .with-detail = { $exit_code ->\n        [none] stopped\n       *[other] exit status { $exit_code }\n      }: { $detail }\n",
            "",
        ),
        (
            &[EXIT],
            "error-x = { $exit_code ->\n    [none] stopped\n   *[other] exit status { $exit_code }\n  }\n    .with-detail = status { $exit_code }: { $detail }\n",
            "none",
        ),
        // Read outside the select, or in the arm that runs when it is left out.
        (
            &[EXIT],
            "error-x = { $exit_code ->\n    [none] stopped\n   *[other] ended\n  } with { $exit_code }\n",
            "none",
        ),
        (
            &[EXIT],
            "error-x = { $exit_code ->\n    [none] stopped, status { $exit_code }\n   *[other] ended\n  }\n",
            "none",
        ),
    ];
    for (declared, text, culprit) in cases {
        let catalog = locale(text);
        let message = catalog.messages.get("error-x").expect("error-x is defined");
        let problems = argument_mismatches("error-x", declared, message);
        if culprit.is_empty() {
            assert!(problems.is_empty(), "{text:?}: {problems:#?}");
        } else {
            assert_eq!(problems.len(), 1, "{text:?}: {problems:#?}");
            assert!(problems[0].contains(culprit), "{text:?}: {problems:#?}");
        }
    }
    // The detail is the renderer's, so no code may declare it.
    let reserved = ArgSpec {
        name: "detail",
        kind: ArgKind::Text,
        optional: false,
    };
    let catalog = locale("error-x = Failed: { $detail }\n");
    let message = catalog.messages.get("error-x").expect("error-x is defined");
    assert!(!argument_mismatches("error-x", &[reserved], message).is_empty());
}

#[test]
fn every_code_renders_in_english_with_and_without_its_detail() {
    let localizer = english();
    let source = source_locale();
    let detail = "disk full (os error 28)";
    assert!(!ERRORS.is_empty() && !WARNINGS.is_empty());
    let errors = ERRORS.iter().map(|(code, id)| {
        let args = args_for(code.args());
        (
            id,
            localizer.error(*code, &args, None),
            localizer.error(*code, &args, Some(detail)),
        )
    });
    let warnings = WARNINGS.iter().map(|(code, id)| {
        let args = args_for(code.args());
        (
            id,
            localizer.warning(*code, &args, None),
            localizer.warning(*code, &args, Some(detail)),
        )
    });
    for (id, plain, detailed) in errors.chain(warnings) {
        let id = id.as_str();
        for shown in [&plain, &detailed] {
            assert!(shown.problems().is_empty(), "{id}: {:?}", shown.problems());
            assert!(!shown.text().trim().is_empty(), "{id} renders as nothing");
        }
        assert!(
            !plain.text().contains(detail),
            "{id} shows a detail it wasn't given"
        );
        let has_detail_form = source
            .messages
            .get(id)
            .is_some_and(|message| message.attributes.contains_key("with-detail"));
        if has_detail_form {
            assert!(detailed.text().contains(detail), "{id} drops its detail");
        }
    }
}

#[test]
fn an_unknown_code_is_an_unexpected_error_that_keeps_its_detail() {
    // The wire string of an unknown code is discarded when it is read, so the
    // message has no code to show: it says something unexpected happened and
    // leaves the rest to the detail.
    let localizer = english();
    assert!(ErrorCode::Unknown.args().is_empty());
    assert!(WarningCode::Unknown.args().is_empty());
    let detail = "quota exceeded";
    let error = localizer.error(ErrorCode::Unknown, &[], Some(detail));
    let warning = localizer.warning(WarningCode::Unknown, &[], Some(detail));
    for shown in [&error, &warning] {
        assert!(shown.problems().is_empty(), "{:?}", shown.problems());
        assert!(shown.text().contains(detail), "{shown}");
    }
    let bare = localizer.error(ErrorCode::Unknown, &[], None);
    assert!(bare.problems().is_empty());
    assert!(!bare.text().trim().is_empty());
}

const EN: Catalog = Catalog {
    tag: "en-US",
    sources: &[Source {
        name: "codes.ftl",
        text: "\
error-operation-failed = The operation failed.
    .with-detail = The operation failed: { $detail }
error-deadline-passed = The deadline passed.
",
    }],
};

const DE: Catalog = Catalog {
    tag: "de",
    sources: &[Source {
        name: "codes.ftl",
        text: "error-operation-failed = Der Vorgang ist fehlgeschlagen.\n",
    }],
};

#[test]
fn a_detail_goes_where_the_with_detail_form_puts_it() {
    let localizer = Localizer::from_catalogs(&[], &[EN]);
    let failed = |detail| localizer.error(ErrorCode::OperationFailed, &[], detail);
    assert_eq!(
        failed(Some("disk full")).text(),
        "The operation failed: disk full"
    );
    assert_eq!(failed(None).text(), "The operation failed.");
    // A message without the form shows its plain text, and no problem.
    let late = localizer.error(ErrorCode::DeadlinePassed, &[], Some("disk full"));
    assert_eq!(late.text(), "The deadline passed.");
    assert!(late.problems().is_empty());
}

#[test]
fn a_detail_is_the_senders_and_an_argument_cannot_stand_in_for_it() {
    let localizer = Localizer::from_catalogs(&[], &[EN]);
    let forged = [("detail", MsgArg::Text("forged".to_owned()))];
    let shown = localizer.error(ErrorCode::OperationFailed, &forged, Some("real"));
    assert_eq!(shown.text(), "The operation failed: real");
}

#[test]
fn a_long_detail_is_cut_to_the_cap() {
    let localizer = Localizer::from_catalogs(&[], &[EN]);
    let detail = "x".repeat(100_000);
    let shown = localizer.error(ErrorCode::OperationFailed, &[], Some(&detail));
    let frame = "The operation failed: ".len();
    assert!(
        shown.text().len() <= frame + 16 * 1024,
        "{}",
        shown.text().len()
    );
    assert!(shown.text().ends_with('…'));
}

#[test]
fn a_code_with_no_message_anywhere_is_shown_by_its_id_and_reported() {
    let localizer = Localizer::from_catalogs(&[], &[EN]);
    let shown = localizer.error(ErrorCode::HostNotAllowed, &[], None);
    assert_eq!(shown.text(), "error-host-not-allowed");
    assert!(!shown.problems().is_empty());
    let shown = localizer.warning(WarningCode::SourceUnreadable, &[], None);
    assert_eq!(shown.text(), "warning-source-unreadable");
    assert!(!shown.problems().is_empty());
}

#[test]
fn a_translation_without_the_detail_form_falls_back_for_the_detail_only() {
    let localizer = Localizer::from_catalogs(&[tag("de")], &[EN, DE]);
    assert_eq!(localizer.locales(), [tag("de"), tag("en-US")]);
    let failed = |detail| localizer.error(ErrorCode::OperationFailed, &[], detail);
    assert_eq!(failed(None).text(), "Der Vorgang ist fehlgeschlagen.");
    assert_eq!(
        failed(Some("disk full")).text(),
        "The operation failed: disk full"
    );
    // A message the translation lacks falls back whole.
    let late = localizer.error(ErrorCode::DeadlinePassed, &[], None);
    assert_eq!(late.text(), "The deadline passed.");
}

#[test]
fn an_argument_the_sender_may_leave_out_is_the_text_none() {
    const NO_STATUS: Catalog = Catalog {
        tag: "en-US",
        sources: &[Source {
            name: "t.ftl",
            text: "error-engine-exited = { $exit_code ->\n    [none] no exit status\n   *[other] exit status { $exit_code }\n  }\n",
        }],
    };
    let localizer = Localizer::from_catalogs(&[], &[NO_STATUS]);
    let left_out = localizer.error(ErrorCode::EngineExited, &[], None);
    assert_eq!(left_out.text(), "no exit status");
    assert!(left_out.problems().is_empty(), "{:?}", left_out.problems());
    let code = [("exit_code", MsgArg::Text("3".to_owned()))];
    let sent = localizer.error(ErrorCode::EngineExited, &code, None);
    assert_eq!(sent.text(), "exit status 3");
    assert!(sent.problems().is_empty(), "{:?}", sent.problems());
}

#[test]
fn an_argument_a_code_requires_is_shown_by_name_and_reported_when_left_out() {
    let shown = english().error(ErrorCode::ProtocolViolated, &[], None);
    assert!(!shown.problems().is_empty());
    assert!(shown.text().contains("transition"), "{shown}");
}

#[test]
fn a_code_renders_cleanly_without_the_arguments_its_sender_may_leave_out() {
    let localizer = english();
    assert!(!ERRORS.is_empty());
    let mut tested = 0;
    for (code, id) in ERRORS {
        let declared = code.args();
        if !declared.iter().any(|spec| spec.optional) {
            continue;
        }
        let required: Vec<ArgSpec> = declared
            .iter()
            .filter(|spec| !spec.optional)
            .copied()
            .collect();
        let without = localizer.error(*code, &args_for(&required), None);
        let with = localizer.error(*code, &args_for(declared), None);
        let id = id.as_str();
        assert!(
            without.problems().is_empty(),
            "{id}: {:?}",
            without.problems()
        );
        assert_ne!(
            without.text(),
            with.text(),
            "{id} reads the same without it"
        );
        assert!(!without.text().contains("none"), "{id} shows the stand-in");
        tested += 1;
    }
    assert!(tested > 0, "no code has an optional argument");
}

#[test]
fn a_blank_detail_is_no_detail() {
    let localizer = english();
    let error = localizer.error(ErrorCode::OperationFailed, &[], None);
    let warning = localizer.warning(WarningCode::SourceUnreadable, &[], None);
    for blank in ["", " ", "\n\t "] {
        let shown = localizer.error(ErrorCode::OperationFailed, &[], Some(blank));
        assert_eq!(shown.text(), error.text(), "{blank:?}");
        let shown = localizer.warning(WarningCode::SourceUnreadable, &[], Some(blank));
        assert_eq!(shown.text(), warning.text(), "{blank:?}");
    }
}

#[test]
fn a_translation_may_read_an_optional_argument_only_where_it_is_there() {
    let source = source_locale();
    let check = |text: &str| translation_problems(&source, &locale(text));
    let chosen = "error-engine-exited = { $exit_code ->\n    [none] Ohne Status beendet\n   *[other] Beendet mit Status { $exit_code }\n  }\n";
    assert!(check(chosen).is_empty());
    // Read plainly, it would show "none" when the sender leaves it out.
    let problems = check("error-engine-exited = Beendet mit Status { $exit_code }\n");
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].contains("exit_code") && problems[0].contains("none"),
        "{problems:#?}"
    );
}

#[test]
fn an_optional_argument_is_text() {
    // The renderer stands in for a missing one with the text `none`.
    let declared = ERRORS
        .iter()
        .flat_map(|(code, _)| code.args())
        .chain(WARNINGS.iter().flat_map(|(code, _)| code.args()));
    let optional: Vec<&ArgSpec> = declared.filter(|spec| spec.optional).collect();
    assert!(!optional.is_empty());
    for spec in optional {
        assert_eq!(spec.kind, ArgKind::Text, "`{}` is optional", spec.name);
    }
}

#[test]
fn an_engine_that_left_no_exit_status_is_not_said_to_have_been_signalled() {
    let localizer = english();
    let left_out = localizer.error(ErrorCode::EngineExited, &[], None);
    assert!(left_out.problems().is_empty(), "{:?}", left_out.problems());
    assert!(
        !left_out.text().to_lowercase().contains("signal"),
        "{left_out}"
    );
    let code = [("exit_code", MsgArg::Text("3".to_owned()))];
    let sent = localizer.error(ErrorCode::EngineExited, &code, None);
    assert!(sent.text().contains("exit status 3"), "{sent}");
}
