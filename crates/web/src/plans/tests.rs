use api::{ErrorCode, MsgArg, PlanDto, PlanSummaryDto};
use l10n::msg;
use serde_json::{Value, json};

use super::*;

/// A plan as the daemon sends it: the table of `value`.
fn plan(value: Value) -> PlanDto {
    match value {
        Value::Object(table) => PlanDto(table),
        other => panic!("a plan is a table, not {other}"),
    }
}

/// The plan the form of `base`, changed by `edit`, sends.
fn sent(base: &Value, edit: impl FnOnce(&mut PlanForm)) -> Value {
    let base = plan(base.clone());
    let mut form = PlanForm::of(&base);
    edit(&mut form);
    Value::Object(form.request(&base).0)
}

fn home() -> Value {
    json!({
        "id": "home",
        "repository": "demo",
        "sources": ["/srv/demo/photos", "/srv/demo/notes"],
        "schedule": { "cron": "0 3 * * *", "timezone": "Europe/Berlin" },
        "retention": { "keep-last": 7, "keep-within": "2 days" },
        "hooks": {
            "run-before": [
                "echo starting",
                { "command": "mount-demo", "args": ["--quiet"], "on-failure": "warn" }
            ]
        },
        "host": "demo-host",
    })
}

fn invalid(field: &str, problem: &str) -> Vec<(String, MsgArg)> {
    vec![
        ("field".to_owned(), MsgArg::Text(field.to_owned())),
        ("problem".to_owned(), MsgArg::Text(problem.to_owned())),
    ]
}

fn summary(id: &str, scheduled: bool, next_run: Option<&str>) -> PlanSummaryDto {
    PlanSummaryDto {
        id: id.to_owned(),
        repository: "demo".to_owned(),
        scheduled,
        next_run: next_run.map(str::to_owned),
        warnings: Vec::new(),
    }
}

#[test]
fn the_form_of_a_plan_shows_each_setting_it_has_a_field_for() {
    let form = PlanForm::of(&plan(home()));
    assert_eq!(form.text(Field::Name), "home");
    assert_eq!(form.text(Field::Repository), "demo");
    assert_eq!(
        form.text(Field::Sources),
        "/srv/demo/photos\n/srv/demo/notes"
    );
    assert_eq!(form.text(Field::Cron), "0 3 * * *");
    assert_eq!(form.text(Field::KeepLast), "7");
    assert_eq!(form.text(Field::KeepDaily), "");
    assert!(!form.git_ignore);
}

#[test]
fn a_hook_written_as_a_table_shows_its_command() {
    let form = PlanForm::of(&plan(home()));
    assert_eq!(form.text(Field::RunBefore), "echo starting\nmount-demo");
}

#[test]
fn a_form_sent_unchanged_sends_the_plan_as_it_came() {
    assert_eq!(sent(&home(), |_| {}), home());
}

#[test]
fn a_changed_schedule_keeps_the_schedule_s_other_settings_and_the_keys_the_form_does_not_show() {
    let sent = sent(&home(), |form| {
        form.set_text(Field::Cron, "30 2 * * *".to_owned());
    });
    assert_eq!(
        sent["schedule"],
        json!({ "cron": "30 2 * * *", "timezone": "Europe/Berlin" })
    );
    assert_eq!(sent["host"], "demo-host");
    assert_eq!(sent["retention"]["keep-within"], "2 days");
}

#[test]
fn a_cron_schedule_typed_over_an_interval_replaces_the_interval() {
    let base = json!({ "id": "home", "schedule": { "every": "1h", "jitter": "5m" } });
    let sent = sent(&base, |form| {
        form.set_text(Field::Cron, "0 3 * * *".to_owned())
    });
    assert_eq!(
        sent["schedule"],
        json!({ "cron": "0 3 * * *", "jitter": "5m" })
    );
}

#[test]
fn an_emptied_schedule_removes_the_whole_schedule() {
    let sent = sent(&home(), |form| form.set_text(Field::Cron, "  ".to_owned()));
    assert!(sent.get("schedule").is_none(), "{sent}");
}

#[test]
fn lines_are_sent_each_trimmed_and_blank_ones_left_out() {
    let sent = sent(&home(), |form| {
        form.set_text(Field::Sources, " /srv/a \n\n/srv/b\n".to_owned());
    });
    assert_eq!(sent["sources"], json!(["/srv/a", "/srv/b"]));
}

#[test]
fn an_emptied_list_removes_its_key() {
    let sent = sent(&home(), |form| {
        form.set_text(Field::Sources, "\n".to_owned())
    });
    assert!(sent.get("sources").is_none(), "{sent}");
}

#[test]
fn a_hook_kept_by_its_command_keeps_its_table() {
    let sent = sent(&home(), |form| {
        form.set_text(Field::RunBefore, "mount-demo\necho ready".to_owned());
    });
    assert_eq!(
        sent["hooks"]["run-before"],
        json!([
            { "command": "mount-demo", "args": ["--quiet"], "on-failure": "warn" },
            "echo ready"
        ])
    );
}

#[test]
fn a_count_is_sent_as_a_number() {
    let sent = sent(&home(), |form| {
        form.set_text(Field::KeepDaily, " 14 ".to_owned())
    });
    assert_eq!(sent["retention"]["keep-daily"], json!(14));
}

#[test]
fn a_count_that_is_not_a_whole_number_is_sent_as_typed_for_the_daemon_to_refuse() {
    let sent = sent(&home(), |form| {
        form.set_text(Field::KeepLast, "seven".to_owned())
    });
    assert_eq!(sent["retention"]["keep-last"], json!("seven"));
}

#[test]
fn a_table_left_empty_is_removed() {
    let base = json!({ "id": "home", "retention": { "keep-last": 7 } });
    let sent = sent(&base, |form| form.set_text(Field::KeepLast, String::new()));
    assert!(sent.get("retention").is_none(), "{sent}");
}

#[test]
fn a_setting_typed_into_an_absent_table_creates_it() {
    let sent = sent(&json!({ "id": "home" }), |form| {
        form.set_text(Field::RunAfter, "echo done".to_owned());
    });
    assert_eq!(sent["hooks"], json!({ "run-after": ["echo done"] }));
}

#[test]
fn git_ignore_is_written_only_when_it_is_on() {
    let on = sent(&json!({ "id": "home" }), |form| form.git_ignore = true);
    assert_eq!(on["git-ignore"], json!(true));
    let off = sent(&json!({ "id": "home", "git-ignore": true }), |form| {
        form.git_ignore = false;
    });
    assert!(off.get("git-ignore").is_none(), "{off}");
}

#[test]
fn a_new_plan_sends_what_was_typed_with_the_spaces_around_it_trimmed() {
    let sent = sent(&json!({}), |form| {
        form.set_text(Field::Name, " demo-docs ".to_owned());
        form.set_text(Field::Repository, "demo".to_owned());
        form.set_text(Field::Sources, "/srv/demo/docs".to_owned());
        form.set_text(Field::Cron, "0 3 * * *".to_owned());
        form.set_text(Field::KeepLast, "7".to_owned());
    });
    assert_eq!(
        sent,
        json!({
            "id": "demo-docs",
            "repository": "demo",
            "sources": ["/srv/demo/docs"],
            "schedule": { "cron": "0 3 * * *" },
            "retention": { "keep-last": 7 },
        })
    );
}

#[test]
fn a_refusal_is_about_the_field_whose_setting_it_names() {
    let cases = [
        ("id", Field::Name),
        ("schedule.cron", Field::Cron),
        ("schedule", Field::Cron),
        ("sources", Field::Sources),
        ("sources[1]", Field::Sources),
        ("retention.keep-last", Field::KeepLast),
        ("hooks.run-failed[0]", Field::RunFailed),
        ("exclude-if-present", Field::ExcludeIfPresent),
    ];
    for (setting, field) in cases {
        assert_eq!(
            field_of(ErrorCode::PlanInvalid, &invalid(setting, "invalid")),
            Some(field),
            "{setting}"
        );
    }
}

#[test]
fn a_refusal_of_a_setting_the_form_does_not_show_is_about_no_field() {
    for setting in [
        "schedule.every",
        "host",
        "retention.keep-within",
        "sourcesx",
    ] {
        assert_eq!(
            field_of(ErrorCode::PlanInvalid, &invalid(setting, "invalid")),
            None,
            "{setting}"
        );
    }
}

#[test]
fn a_refusal_of_another_kind_is_about_no_field() {
    let args = vec![("plan".to_owned(), MsgArg::Text("home".to_owned()))];
    assert_eq!(field_of(ErrorCode::PlanUnknown, &args), None);
    assert_eq!(
        field_of(ErrorCode::PlanInUse, &invalid("id", "taken")),
        None
    );
}

#[test]
fn an_invalid_schedule_says_what_a_schedule_is() {
    for problem in ["invalid", "wrong-type"] {
        assert_eq!(
            own_words(ErrorCode::PlanInvalid, &invalid("schedule.cron", problem)),
            Some(msg::UI_PLAN_CRON_INVALID),
            "{problem}"
        );
    }
}

#[test]
fn an_invalid_name_says_what_a_name_is_made_of() {
    for problem in ["invalid", "wrong-type"] {
        assert_eq!(
            own_words(ErrorCode::PlanInvalid, &invalid("id", problem)),
            Some(msg::UI_PLAN_NAME_INVALID),
            "{problem}"
        );
    }
}

#[test]
fn the_form_s_own_words_name_what_to_enter() {
    let cases = [
        ("id", "missing", msg::UI_PLAN_NAME_MISSING),
        ("id", "taken", msg::UI_PLAN_NAME_TAKEN),
        ("repository", "missing", msg::UI_PLAN_REPOSITORY_MISSING),
        (
            "repository",
            "unknown-repository",
            msg::UI_PLAN_REPOSITORY_UNKNOWN,
        ),
        ("sources", "missing", msg::UI_PLAN_SOURCES_MISSING),
        ("sources", "no-sources", msg::UI_PLAN_SOURCES_MISSING),
        (
            "retention.keep-last",
            "wrong-type",
            msg::UI_PLAN_COUNT_INVALID,
        ),
        (
            "retention.keep-yearly",
            "invalid",
            msg::UI_PLAN_COUNT_INVALID,
        ),
    ];
    for (setting, problem, words) in cases {
        assert_eq!(
            own_words(ErrorCode::PlanInvalid, &invalid(setting, problem)),
            Some(words),
            "{setting} {problem}"
        );
    }
}

#[test]
fn a_problem_the_form_has_no_words_for_is_said_in_the_daemon_s() {
    let cases = [
        ("sources", "invalid"),
        ("id", "unknown-key"),
        ("schedule.cron", "missing"),
        ("hooks.run-before", "wrong-type"),
        ("host", "unknown-key"),
    ];
    for (setting, problem) in cases {
        assert_eq!(
            own_words(ErrorCode::PlanInvalid, &invalid(setting, problem)),
            None,
            "{setting} {problem}"
        );
    }
    assert_eq!(
        own_words(ErrorCode::PlanInUse, &invalid("id", "taken")),
        None
    );
}

#[test]
fn each_field_has_its_own_input_id() {
    let mut ids: Vec<_> = Field::ALL.iter().map(|field| field.id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), Field::ALL.len());
}

#[test]
fn a_saved_plan_on_a_schedule_awaits_its_next_run_until_the_list_has_one() {
    let waiting = [summary("home", true, None)];
    let planned = [summary("home", true, Some("2026-11-16T03:00:00Z"))];
    assert!(awaits_next_run(&waiting, "home"));
    assert!(!awaits_next_run(&planned, "home"));
}

#[test]
fn a_plan_run_only_when_started_or_not_listed_awaits_nothing() {
    assert!(!awaits_next_run(&[summary("home", false, None)], "home"));
    assert!(!awaits_next_run(&[summary("other", true, None)], "home"));
}

#[test]
fn a_plan_s_paths_name_it() {
    assert_eq!(plan_path("demo-home"), "/api/v1/plans/demo-home");
}

// Focus returns to a button by its `id`, so each plan's two buttons have
// one of their own, which an HTML `id` can be: not empty, with no space.
#[test]
fn each_plan_s_buttons_have_ids_of_their_own() {
    let ids = [
        change_id("demo-home"),
        delete_id("demo-home"),
        change_id("demo-docs"),
        delete_id("demo-docs"),
    ];
    let distinct: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(distinct.len(), ids.len(), "{ids:?}");
    for id in &ids {
        assert!(
            !id.is_empty() && !id.contains(char::is_whitespace),
            "{id:?}"
        );
    }
}

#[test]
fn the_words_about_a_plan_name_it() {
    let named = |(id, args): Said| {
        (
            id,
            args.into_iter()
                .map(|(name, value)| (name, format!("{value:?}")))
                .collect::<Vec<_>>(),
        )
    };
    let base = plan(home());
    let home = vec![("plan", format!("{:?}", MsgArg::Text("home".to_owned())))];
    assert_eq!(
        named(saved("home", &base, &base)),
        (msg::UI_PLAN_SAVED, home.clone())
    );
    assert_eq!(named(deleted("home")), (msg::UI_PLAN_DELETED, home.clone()));
    assert_eq!(
        named(change_label("home")),
        (msg::UI_PLAN_CHANGE, home.clone())
    );
    assert_eq!(
        named(delete_label("home")),
        (msg::UI_PLAN_DELETE, home.clone())
    );
    assert_eq!(
        named(delete_title("home")),
        (msg::UI_PLAN_DELETE_TITLE, home.clone())
    );
    assert_eq!(
        named(delete_successor("home")),
        (msg::UI_PLAN_DELETE_SUCCESSOR, home.clone())
    );
    assert_eq!(
        named(form_change_heading("home")),
        (msg::UI_PLAN_FORM_CHANGE, home)
    );
}

#[test]
fn each_field_s_label_and_hint_select_the_field_by_its_setting() {
    let selects = |(id, args): Said| (id, format!("{args:?}"));
    let field = |code: &str| format!("{:?}", vec![("field", MsgArg::Text(code.to_owned()))]);
    let cases = [
        (Field::Name, "name"),
        (Field::Cron, "cron"),
        (Field::KeepLast, "keep-last"),
        (Field::ExcludeIfPresent, "exclude-if-present"),
        (Field::RunFinally, "run-finally"),
    ];
    for (form_field, code) in cases {
        assert_eq!(
            selects(form_field.label()),
            (msg::UI_PLAN_FIELD_LABEL, field(code))
        );
        assert_eq!(
            selects(form_field.hint()),
            (msg::UI_PLAN_FIELD_HINT, field(code))
        );
    }
}

#[test]
fn a_scheduled_plan_saved_without_its_schedule_is_said_to_run_only_when_started() {
    let base = plan(home());
    let mut form = PlanForm::of(&base);
    form.set_text(Field::Cron, String::new());
    let sent = form.request(&base);
    assert_eq!(
        saved("home", &base, &sent).0,
        msg::UI_PLAN_SAVED_UNSCHEDULED
    );
}

#[test]
fn a_plan_that_keeps_its_schedule_or_never_had_one_is_said_only_to_be_saved() {
    let scheduled = plan(home());
    let mut form = PlanForm::of(&scheduled);
    form.set_text(Field::Cron, "30 2 * * *".to_owned());
    let changed = form.request(&scheduled);
    assert_eq!(saved("home", &scheduled, &changed).0, msg::UI_PLAN_SAVED);
    let unscheduled = plan(json!({ "id": "home", "repository": "demo", "sources": ["/srv"] }));
    assert_eq!(
        saved("home", &unscheduled, &unscheduled).0,
        msg::UI_PLAN_SAVED
    );
    let new = PlanDto::default();
    assert_eq!(saved("home", &new, &unscheduled).0, msg::UI_PLAN_SAVED);
}

#[test]
fn a_refusal_the_form_has_words_for_is_said_in_them_at_its_field() {
    let daemon = Sentence::new("The setting schedule.cron is invalid.", None);
    let item = refusal_of(
        ErrorCode::PlanInvalid,
        &invalid("schedule.cron", "invalid"),
        |id| format!("{id:?}"),
        daemon,
    );
    assert_eq!(item.field, Some(Field::Cron.id()));
    assert_eq!(
        item.sentence,
        Sentence::new(&format!("{:?}", msg::UI_PLAN_CRON_INVALID), None)
    );
}

#[test]
fn a_refusal_the_form_has_no_words_for_is_said_in_the_daemon_s_at_its_field() {
    let daemon = Sentence::new("The setting hooks.run-before is invalid.", None);
    let item = refusal_of(
        ErrorCode::PlanInvalid,
        &invalid("hooks.run-before[0]", "invalid"),
        |id| format!("{id:?}"),
        daemon.clone(),
    );
    assert_eq!(item.field, Some(Field::RunBefore.id()));
    assert_eq!(item.sentence, daemon);
    let elsewhere = refusal_of(
        ErrorCode::PlanInUse,
        &[],
        |id| format!("{id:?}"),
        daemon.clone(),
    );
    assert_eq!(elsewhere.field, None);
    assert_eq!(elsewhere.sentence, daemon);
}

#[test]
fn a_field_shows_the_problem_the_summary_has_about_it() {
    let items = [
        SummaryItem {
            field: None,
            sentence: Sentence::new("Elsewhere.", None),
        },
        SummaryItem {
            field: Some(Field::Cron.id()),
            sentence: Sentence::new("Enter a cron schedule.", None),
        },
    ];
    assert_eq!(
        error_of(&items, Field::Cron).as_deref(),
        Some("Enter a cron schedule.")
    );
    assert_eq!(error_of(&items, Field::Name), None);
}

#[test]
fn a_save_s_answer_names_the_saved_plan() {
    assert_eq!(saved_id(&home().to_string()).as_deref(), Some("home"));
    assert_eq!(saved_id(r#"{"repository":"demo"}"#), None);
    assert_eq!(saved_id(r#"{"id":7}"#), None);
    assert_eq!(saved_id("not json"), None);
}

#[test]
fn a_new_plan_starts_with_the_first_repository_and_nothing_else() {
    let repositories: api::RepositoriesDto = serde_json::from_value(json!({
        "repositories": [
            { "alias": "demo", "repository": "/srv/demo", "password": { "source": "inline" } },
            { "alias": "other", "repository": "/srv/other", "password": { "source": "inline" } },
        ]
    }))
    .expect("a repositories list");
    assert_eq!(
        Value::Object(new_plan(&repositories.repositories).0),
        json!({ "repository": "demo" })
    );
    assert_eq!(new_plan(&[]), PlanDto::default());
}

#[test]
fn only_the_fields_a_plan_is_refused_without_are_marked_required() {
    let required: Vec<Field> = Field::ALL.into_iter().filter(|field| field.required()).collect();
    assert_eq!(required, [Field::Name, Field::Repository, Field::Sources]);
}
