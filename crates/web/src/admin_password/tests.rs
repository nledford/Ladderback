use api::{AdminPasswordDto, ErrorCode, ExposureDto, MsgArg, SessionDto, SignInStateDto};
use l10n::{MessageId, msg};

use super::*;

fn form(current: Option<&str>, new: &str, again: &str) -> PasswordForm {
    PasswordForm {
        current: current.map(str::to_owned),
        new: new.to_owned(),
        again: again.to_owned(),
    }
}

/// What `problems_of` asks the catalog for, as text a test can compare.
fn said(id: MessageId, args: &[(&'static str, MsgArg)]) -> String {
    format!("{id:?} {args:?}")
}

#[test]
fn with_sign_in_off_two_new_passwords_alike_lack_nothing() {
    assert!(
        form(None, "demo-admin-pass", "demo-admin-pass")
            .missing()
            .is_empty()
    );
}

#[test]
fn with_sign_in_on_the_current_password_is_needed_too() {
    assert_eq!(
        form(Some(""), "demo-admin-pass", "demo-admin-pass").missing(),
        [Field::Current]
    );
    assert!(
        form(
            Some("demo-admin-pass"),
            "another-admin-pass",
            "another-admin-pass"
        )
        .missing()
        .is_empty()
    );
}

#[test]
fn an_empty_form_lacks_each_field_in_the_page_s_order() {
    assert_eq!(
        form(Some(""), "", "").missing(),
        [Field::Current, Field::New]
    );
}

#[test]
fn new_passwords_that_differ_lack_the_second() {
    assert_eq!(
        form(None, "demo-admin-pass", "demo-admin-pas").missing(),
        [Field::Again]
    );
    assert_eq!(
        form(None, "", "demo-admin-pass").missing(),
        [Field::New, Field::Again]
    );
}

#[test]
fn a_password_is_never_trimmed_so_spaces_make_passwords_differ() {
    assert_eq!(
        form(None, "demo-admin-pass ", "demo-admin-pass").missing(),
        [Field::Again]
    );
    assert!(form(None, "        ", "        ").missing().is_empty());
}

#[test]
fn each_problem_links_to_its_own_field_and_says_what_to_do() {
    let items = problems_of(&form(Some(""), "demo-admin-pass", "other"), said).expect("lacking");
    assert_eq!(
        items.iter().map(|item| item.field).collect::<Vec<_>>(),
        [Some(Field::Current.id()), Some(Field::Again.id())]
    );
    assert_eq!(
        items[0].sentence.plain(),
        said(msg::UI_ADMIN_PASSWORD_CURRENT_REQUIRED, &[])
    );
    assert_eq!(
        items[1].sentence.plain(),
        said(msg::UI_ADMIN_PASSWORD_MISMATCH, &[])
    );
    let new = problems_of(&form(None, "", ""), said).expect("lacking");
    assert_eq!(
        new[0].sentence.plain(),
        said(msg::UI_ADMIN_PASSWORD_NEW_REQUIRED, &[])
    );
    assert!(problems_of(&form(None, "demo-admin-pass", "demo-admin-pass"), said).is_none());
}

#[test]
fn the_error_a_field_shows_is_its_item_s_sentence() {
    let items = problems_of(&form(Some(""), "demo-admin-pass", "other"), said).expect("lacking");
    assert_eq!(
        error_of(&items, Field::Current),
        Some(said(msg::UI_ADMIN_PASSWORD_CURRENT_REQUIRED, &[]))
    );
    assert_eq!(error_of(&items, Field::New), None);
}

#[test]
fn the_request_carries_the_current_password_only_when_it_was_asked_for() {
    let off = form(None, "demo-admin-pass", "demo-admin-pass").request();
    assert_eq!(off.current_password, None);
    assert_eq!(off.new_password, "demo-admin-pass");
    let on = form(
        Some("demo-admin-pass"),
        "another-admin-pass",
        "another-admin-pass",
    )
    .request();
    assert_eq!(on.current_password.as_deref(), Some("demo-admin-pass"));
    assert_eq!(on.new_password, "another-admin-pass");
}

#[test]
fn a_refusal_names_the_field_it_is_about() {
    assert_eq!(
        field_of(ErrorCode::CurrentPasswordWrong),
        Some(Field::Current)
    );
    assert_eq!(field_of(ErrorCode::AdminPasswordTooShort), Some(Field::New));
    assert_eq!(field_of(ErrorCode::AdminPasswordTooLong), Some(Field::New));
    assert_eq!(field_of(ErrorCode::SignInWait), None);
    assert_eq!(field_of(ErrorCode::AdminPasswordUnavailable), None);
}

#[test]
fn the_form_s_debug_never_shows_a_password() {
    let shown = format!(
        "{:?}",
        form(Some("demo-admin-pass"), "another-admin-pass", "x")
    );
    assert!(!shown.contains("demo-admin-pass"), "{shown}");
    assert!(!shown.contains("another-admin-pass"), "{shown}");
    assert!(shown.contains("PasswordForm"), "{shown}");
}

#[test]
fn the_form_keeps_the_current_password_only_while_sign_in_is_on() {
    let typed = |on| {
        PasswordForm::typed(
            on,
            "demo-admin-pass".to_owned(),
            "another-admin-pass".to_owned(),
            "another-admin-pas".to_owned(),
        )
    };
    assert_eq!(
        typed(true),
        form(
            Some("demo-admin-pass"),
            "another-admin-pass",
            "another-admin-pas"
        )
    );
    assert_eq!(
        typed(false),
        form(None, "another-admin-pass", "another-admin-pas")
    );
}

#[test]
fn with_sign_in_on_the_section_says_a_change_ends_every_session_and_offers_to_change() {
    assert_eq!(
        body(true, Some(msg::UI_ADMIN_PASSWORD_WAITING)),
        Some(msg::UI_ADMIN_PASSWORD_ON_BODY)
    );
    assert_eq!(body(true, None), Some(msg::UI_ADMIN_PASSWORD_ON_BODY));
    assert_eq!(button(true), msg::UI_ADMIN_PASSWORD_CHANGE);
}

#[test]
fn with_sign_in_off_the_section_says_none_is_set_until_one_waits_and_offers_to_set() {
    assert_eq!(body(false, None), Some(msg::UI_ADMIN_PASSWORD_OFF_BODY));
    assert_eq!(body(false, Some(msg::UI_ADMIN_PASSWORD_WAITING)), None);
    assert_eq!(body(false, Some(msg::UI_ADMIN_PASSWORD_DAMAGED)), None);
    assert_eq!(button(false), msg::UI_ADMIN_PASSWORD_SET);
}

/// What `steps` asks the catalog for, as text a test can compare.
fn said_alone(id: MessageId) -> String {
    said(id, &[])
}

fn refusal(code: ErrorCode) -> Answer {
    Answer::Refused(code, Sentence::new(&format!("{code:?}"), None))
}

#[test]
fn a_changed_password_clears_the_form_then_shows_the_sign_in_page() {
    assert_eq!(
        steps(true, Answer::Done, true, said_alone),
        [Step::Clear, Step::SignInWithTheNew]
    );
}

#[test]
fn a_password_changed_after_the_reader_left_settings_still_shows_the_sign_in_page() {
    assert_eq!(
        steps(true, Answer::Done, false, said_alone),
        [Step::SignInWithTheNew]
    );
}

#[test]
fn a_password_set_with_sign_in_off_clears_the_form_and_says_what_waits() {
    assert_eq!(
        steps(false, Answer::Done, true, said_alone),
        [Step::Clear, Step::SayWaiting]
    );
    assert!(steps(false, Answer::Done, false, said_alone).is_empty());
}

#[test]
fn a_refusal_is_shown_beside_its_field_and_keeps_the_session() {
    let shown = |code| match &steps(true, refusal(code), true, said_alone)[..] {
        [Step::Show(items)] => items.clone(),
        other => panic!("{code:?}: {other:?}"),
    };
    assert_eq!(
        shown(ErrorCode::CurrentPasswordWrong),
        [SummaryItem {
            field: Some(Field::Current.id()),
            sentence: Sentence::new("CurrentPasswordWrong", None),
        }]
    );
    assert_eq!(
        shown(ErrorCode::AdminPasswordTooShort)[0].field,
        Some(Field::New.id())
    );
    assert_eq!(shown(ErrorCode::SignInWait)[0].field, None);
    assert!(steps(true, refusal(ErrorCode::SignInWait), false, said_alone).is_empty());
    assert!(
        steps(
            false,
            refusal(ErrorCode::AdminPasswordTooShort),
            false,
            said_alone
        )
        .is_empty()
    );
}

#[test]
fn no_answer_says_the_daemon_is_unreachable_and_keeps_the_session() {
    for on in [true, false] {
        assert_eq!(
            steps(on, Answer::Unreachable, true, said_alone),
            [Step::Show(vec![SummaryItem {
                field: None,
                sentence: Sentence::new(&said_alone(msg::UI_SESSION_UNREACHABLE), None),
            }])],
            "{on}"
        );
        assert!(steps(on, Answer::Unreachable, false, said_alone).is_empty());
    }
}

fn read(admin_password: Option<AdminPasswordDto>) -> SessionDto {
    SessionDto {
        sign_in: SignInStateDto::Off,
        csrf_token: None,
        idle_seconds: None,
        ends_in_seconds: None,
        exposure: ExposureDto::Loopback,
        build: None,
        admin_password,
    }
}

#[test]
fn a_password_set_with_sign_in_off_says_what_the_daemon_says_waits() {
    assert_eq!(
        said_once_set(Some(&read(Some(AdminPasswordDto::Set)))),
        msg::UI_ADMIN_PASSWORD_WAITING
    );
    assert_eq!(
        said_once_set(Some(&read(Some(AdminPasswordDto::Unreadable)))),
        msg::UI_ADMIN_PASSWORD_DAMAGED
    );
}

#[test]
fn a_password_set_whose_session_read_fails_still_says_a_restart_turns_sign_in_on() {
    assert_eq!(said_once_set(None), msg::UI_ADMIN_PASSWORD_WAITING);
    assert_eq!(
        said_once_set(Some(&read(None))),
        msg::UI_ADMIN_PASSWORD_WAITING
    );
}
