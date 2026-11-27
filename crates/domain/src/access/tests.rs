use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::*;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + second).unwrap()
}

fn token(byte: u8) -> Token {
    Token::from_bytes([byte; TOKEN_BYTES])
}

/// The admin password every session of these tests starts under.
const PASSWORD: PasswordFingerprint = PasswordFingerprint::new(7);

/// The admin password once it is set anew.
const NEW_PASSWORD: PasswordFingerprint = PasswordFingerprint::new(8);

fn v4(last: u8) -> Client {
    Client::of(IpAddr::V4(Ipv4Addr::new(192, 0, 2, last)))
}

#[test]
fn a_session_tag_is_sixteen_hex_digits_that_tell_sessions_apart() {
    let one = SessionTag::of(&token(1).digest());
    let other = SessionTag::of(&token(2).digest());
    assert_eq!(one.as_str().len(), 16);
    assert!(
        one.as_str()
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_ne!(one, other);
    assert_eq!(SessionTag::of(&token(1).digest()), one);
    // Neither the token's text nor its digest's leading bytes.
    assert!(!token(1).to_text().starts_with(one.as_str()));
    let digest = token(1).digest();
    let leading: String = digest.as_bytes()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_ne!(one.as_str(), leading);
}

#[test]
fn a_session_tag_reads_back_and_refuses_anything_else() {
    let tag = SessionTag::of(&token(3).digest());
    assert_eq!(SessionTag::parse(tag.as_str()), Ok(tag.clone()));
    for text in [
        "",
        "0123456789abcde",
        "0123456789abcdef0",
        "0123456789ABCDEF",
        "0123456789abcdeg",
    ] {
        assert_eq!(SessionTag::parse(text), Err(InvalidSessionTag), "{text:?}");
    }
}

#[test]
fn a_session_tag_read_from_json_is_checked_as_parse_checks_it() {
    let tag = SessionTag::of(&token(4).digest());
    let json = serde_json::to_string(&tag).unwrap();
    assert_eq!(serde_json::from_str::<SessionTag>(&json).unwrap(), tag);
    for text in ["\"zz\"", "\"0123456789ABCDEF\""] {
        assert!(serde_json::from_str::<SessionTag>(text).is_err(), "{text}");
    }
}

/// A tag refused as JSON is read says why.
#[test]
fn a_session_tag_refused_from_json_says_why() {
    let refused = serde_json::from_str::<SessionTag>("\"zz\"").unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("a session tag isn't 16 lower-case hex digits"),
        "{refused}"
    );
}

#[test]
fn a_password_is_one_byte_to_the_most_bytes() {
    assert_eq!(
        Password::parse(String::new()).unwrap_err(),
        PasswordProblem::Empty
    );
    assert_eq!(Password::parse("x".to_owned()).unwrap().expose(), "x");
    let longest = "x".repeat(PASSWORD_MAX_BYTES);
    assert_eq!(Password::parse(longest.clone()).unwrap().expose(), longest);
    assert_eq!(
        Password::parse("x".repeat(PASSWORD_MAX_BYTES + 1)).unwrap_err(),
        PasswordProblem::TooLong
    );
}

#[test]
fn an_admin_password_has_at_least_eight_characters_not_bytes() {
    assert_eq!(
        Password::new_admin("1234567".to_owned()).unwrap_err(),
        PasswordProblem::TooShort
    );
    assert!(Password::new_admin("12345678".to_owned()).is_ok());
    // Seven characters of two bytes each are fourteen bytes, still seven.
    assert_eq!(
        Password::new_admin("ééééééé".to_owned()).unwrap_err(),
        PasswordProblem::TooShort
    );
    assert_eq!(
        Password::new_admin(String::new()).unwrap_err(),
        PasswordProblem::Empty
    );
    assert_eq!(
        Password::new_admin("x".repeat(PASSWORD_MAX_BYTES + 1)).unwrap_err(),
        PasswordProblem::TooLong
    );
}

#[test]
fn a_password_and_a_token_are_never_shown() {
    let password = Password::parse("demo-admin-pass".to_owned()).unwrap();
    assert!(!format!("{password:?}").contains("demo"));
    assert!(!format!("{:?}", token(0xab)).contains("ab"));
}

/// A password wipes its bytes when it is dropped (it is `ZeroizeOnDrop`);
/// what its drop does is done here to one still in scope.
#[test]
fn a_password_wiped_as_its_drop_wipes_it_keeps_none_of_its_bytes() {
    fn wiped_on_drop(_: &impl zeroize::ZeroizeOnDrop) {}
    let mut password = Password::new_admin("demo-admin-pass".to_owned()).unwrap();
    let mut clone = password.clone();
    wiped_on_drop(&password);

    zeroize::Zeroize::zeroize(&mut password);

    assert_eq!(password.expose(), "");
    assert_eq!(clone.expose(), "demo-admin-pass");
    zeroize::Zeroize::zeroize(&mut clone);
    assert_eq!(clone.expose(), "");
}

#[test]
fn a_token_is_written_as_lower_case_hex_and_read_back() {
    let mut bytes = [0u8; TOKEN_BYTES];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::try_from((index * 9) % 256).unwrap();
    }
    let text = Token::from_bytes(bytes).to_text();
    assert_eq!(text.len(), 64);
    assert!(text.starts_with("0009121b242d363f"), "{text}");
    assert!(
        Token::parse(&text)
            .unwrap()
            .matches(&Token::from_bytes(bytes))
    );
}

#[test]
fn only_sixty_four_lower_case_hex_digits_are_a_token() {
    let text = token(0xab).to_text();
    assert!(Token::parse(text.get(..63).unwrap()).is_none());
    assert!(Token::parse(&format!("{text}0")).is_none());
    assert!(Token::parse(&text.to_uppercase()).is_none());
    assert!(Token::parse(&text.replacen('a', "g", 1)).is_none());
    assert!(Token::parse(&format!("é{}", text.get(2..).unwrap())).is_none());
    assert!(Token::parse("").is_none());
    assert!(Token::parse(&"9".repeat(64)).unwrap().matches(&token(0x99)));
    assert!(Token::parse(&"f".repeat(64)).unwrap().matches(&token(0xff)));
}

#[test]
fn a_token_matches_itself_only() {
    assert!(token(1).matches(&token(1)));
    assert!(!token(1).matches(&token(2)));
}

#[test]
fn tokens_differing_only_in_their_last_byte_dont_match() {
    let mut last = [1u8; TOKEN_BYTES];
    last[TOKEN_BYTES - 1] = 2;
    assert!(!token(1).matches(&Token::from_bytes(last)));
    assert!(!Token::from_bytes(last).matches(&token(1)));
}

#[test]
fn a_session_is_found_by_its_own_token_only() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    let found = sessions.peek(&token(1), PASSWORD, at(1)).unwrap();
    assert!(found.accepts(&token(2)));
    assert!(sessions.use_at(&token(1), PASSWORD, at(1)).is_some());
    let mut last = [1u8; TOKEN_BYTES];
    last[TOKEN_BYTES - 1] = 2;
    let near = Token::from_bytes(last);
    assert!(sessions.peek(&near, PASSWORD, at(1)).is_none());
    assert!(sessions.use_at(&near, PASSWORD, at(1)).is_none());
    // The session of a token that was never handed out is none, not ended.
    sessions.end(&near);
    assert_eq!(sessions.len(), 1);
    sessions.end(&token(1));
    assert!(sessions.is_empty());
}

#[test]
fn a_session_lives_twelve_hours_from_its_last_use() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    let idle = SESSION_IDLE.as_secs();
    assert!(sessions.use_at(&token(1), PASSWORD, at(idle - 1)).is_some());
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(2 * idle - 2))
            .is_some()
    );
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(3 * idle - 2))
            .is_none()
    );
    // Ended is forgotten, not brought back by a later use.
    assert!(sessions.is_empty());
}

#[test]
fn a_session_ends_on_the_twelfth_hour_unused_exactly() {
    let mut sessions = Sessions::default();
    let session = sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    let length = SessionLength::default();
    assert!(session.live_at(at(SESSION_IDLE.as_secs() - 1), length));
    assert!(!session.live_at(at(SESSION_IDLE.as_secs()), length));
}

#[test]
fn a_session_ends_thirty_days_after_it_started_however_it_is_used() {
    let mut sessions = Sessions::default();
    assert_eq!(sessions.length(), SessionLength::ThirtyDays);
    let session = sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    // Thirty days, written out: a lifetime read back from the code would
    // follow any change to it.
    let lifetime = 30 * 24 * 3600;
    assert_eq!(session.ends_at(sessions.length()), Some(at(lifetime)));
    for hour in 1..(lifetime / 3600) {
        assert!(
            sessions
                .use_at(&token(1), PASSWORD, at(hour * 3600))
                .is_some(),
            "hour {hour}"
        );
    }
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(lifetime - 1))
            .is_some()
    );
    assert!(sessions.use_at(&token(1), PASSWORD, at(lifetime)).is_none());
}

#[test]
fn a_clock_that_steps_back_never_shortens_a_session() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    let idle = SESSION_IDLE.as_secs();
    assert!(sessions.use_at(&token(1), PASSWORD, at(idle - 1)).is_some());
    assert!(sessions.use_at(&token(1), PASSWORD, at(10)).is_some());
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(2 * idle - 2))
            .is_some()
    );
}

#[test]
fn a_session_at_the_end_of_time_is_live_until_then() {
    let mut sessions = Sessions::default();
    let late = Timestamp::MAX - SignedDuration::from_hours(1);
    let session = sessions.start(token(1), token(2), PASSWORD, false, false, late);
    assert_eq!(
        session.ends_at(SessionLength::ThirtyDays),
        Some(Timestamp::MAX)
    );
    assert!(session.live_at(late, SessionLength::ThirtyDays));
}

const DAY: i64 = 24 * 3600;

/// Uses the session of `token` every 11 hours from `from` while short of
/// `until`, so its idle limit never ends it; whether it was live at each use.
fn used_every_eleven_hours(sessions: &mut Sessions, token: &Token, from: i64, until: i64) -> bool {
    (0..)
        .map(|step| from + step * 11 * 3600)
        .take_while(|&second| second < until)
        .all(|second| sessions.use_at(token, PASSWORD, at(second)).is_some())
}

#[test]
fn each_session_length_ends_a_session_that_long_after_its_sign_in_however_it_is_used() {
    // In days, written out: a length read back from the code would follow
    // any change to it.
    for (length, days) in [
        (SessionLength::SevenDays, 7),
        (SessionLength::ThirtyDays, 30),
        (SessionLength::NinetyDays, 90),
        (SessionLength::OneYear, 365),
    ] {
        let end = days * DAY;
        assert_eq!(
            length.after_sign_in(),
            Some(SignedDuration::from_secs(end)),
            "{length:?}"
        );
        let mut sessions = Sessions::default();
        sessions.set_length(length, at(0));
        let session = sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
        assert_eq!(session.ends_at(length), Some(at(end)), "{length:?}");
        assert!(
            used_every_eleven_hours(&mut sessions, &token(1), 0, end - 1),
            "{length:?}"
        );
        assert!(
            sessions.use_at(&token(1), PASSWORD, at(end - 1)).is_some(),
            "{length:?}"
        );
        assert!(
            sessions.use_at(&token(1), PASSWORD, at(end)).is_none(),
            "{length:?}"
        );
    }
}

#[test]
fn until_the_browser_closes_and_until_sign_out_set_no_end_after_sign_in_but_the_idle_limit() {
    let two_years = 730 * DAY;
    for length in [
        SessionLength::UntilBrowserCloses,
        SessionLength::UntilSignOut,
    ] {
        assert_eq!(length.after_sign_in(), None, "{length:?}");
        let mut sessions = Sessions::default();
        sessions.set_length(length, at(0));
        let session = sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
        assert_eq!(session.ends_at(length), None, "{length:?}");
        assert!(
            used_every_eleven_hours(&mut sessions, &token(1), 0, two_years),
            "{length:?}"
        );
        assert!(
            sessions
                .use_at(&token(1), PASSWORD, at(two_years))
                .is_some(),
            "{length:?}"
        );
        let idle = SESSION_IDLE.as_secs();
        assert!(
            sessions
                .use_at(&token(1), PASSWORD, at(two_years + idle))
                .is_none(),
            "{length:?}"
        );
    }
}

#[test]
fn a_session_length_is_named_for_the_wire_and_the_store_and_read_back_by_that_name() {
    let names: Vec<&str> = SessionLength::ALL
        .into_iter()
        .map(SessionLength::as_str)
        .collect();
    assert_eq!(
        names,
        [
            "until-browser-closes",
            "7-days",
            "30-days",
            "90-days",
            "1-year",
            "until-sign-out"
        ]
    );
    for length in SessionLength::ALL {
        assert_eq!(SessionLength::parse(length.as_str()), Some(length));
    }
    for other in ["", "30 days", "30-DAYS", "2-days", " 7-days", "forever"] {
        assert_eq!(SessionLength::parse(other), None, "{other:?}");
    }
}

#[test]
fn shortening_the_session_length_ends_the_sessions_older_than_it_at_once_and_no_younger_one() {
    let mut sessions = Sessions::default();
    let now = 8 * DAY;
    assert!(sessions.restore(
        kept(1, 0, now - 3600, PASSWORD),
        token(8),
        PASSWORD,
        at(now)
    ));
    assert!(sessions.restore(
        kept(3, 2 * DAY, now - 3600, PASSWORD),
        token(9),
        PASSWORD,
        at(now)
    ));
    sessions.set_length(SessionLength::SevenDays, at(now));
    assert_eq!(sessions.length(), SessionLength::SevenDays);
    assert_eq!(sessions.take_forgotten(), vec![token(1).digest()]);
    assert_eq!(sessions.len(), 1);
    assert!(sessions.peek(&token(3), PASSWORD, at(now)).is_some());
}

#[test]
fn lengthening_the_session_length_extends_the_sessions_already_open() {
    let mut sessions = Sessions::default();
    sessions.set_length(SessionLength::SevenDays, at(0));
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    assert!(used_every_eleven_hours(
        &mut sessions,
        &token(1),
        0,
        6 * DAY
    ));
    sessions.set_length(SessionLength::ThirtyDays, at(6 * DAY));
    assert!(used_every_eleven_hours(
        &mut sessions,
        &token(1),
        6 * DAY,
        8 * DAY
    ));
    assert!(sessions.take_forgotten().is_empty());
}

#[test]
fn a_restored_session_is_measured_by_the_session_length() {
    let mut sessions = Sessions::default();
    sessions.set_length(SessionLength::SevenDays, at(0));
    assert!(!sessions.restore(
        kept(1, 0, 7 * DAY - 3600, PASSWORD),
        token(9),
        PASSWORD,
        at(7 * DAY)
    ));
    assert_eq!(sessions.take_forgotten(), vec![token(1).digest()]);
}

#[test]
fn a_session_accepts_its_own_csrf_token_only() {
    let mut sessions = Sessions::default();
    let session = sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    assert!(session.accepts(&token(2)));
    assert!(!session.accepts(&token(1)));
    assert!(session.csrf().matches(&token(2)));
}

#[test]
fn signing_out_ends_one_session_only() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    sessions.start(token(3), token(4), PASSWORD, false, false, at(0));
    sessions.end(&token(1));
    assert!(sessions.use_at(&token(1), PASSWORD, at(1)).is_none());
    assert!(sessions.use_at(&token(3), PASSWORD, at(1)).is_some());
    assert_eq!(sessions.len(), 1);
}

#[test]
fn signing_out_everywhere_ends_every_session_for_the_store_to_forget_too() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    sessions.start(token(3), token(4), PASSWORD, true, false, at(0));
    sessions.take_forgotten();
    sessions.end_all();
    assert!(sessions.is_empty());
    assert!(sessions.peek(&token(1), PASSWORD, at(1)).is_none());
    assert!(sessions.peek(&token(3), PASSWORD, at(1)).is_none());
    let mut forgotten = sessions.take_forgotten();
    forgotten.sort_by_key(|digest| *digest.as_bytes());
    let mut expected = vec![token(1).digest(), token(3).digest()];
    expected.sort_by_key(|digest| *digest.as_bytes());
    assert_eq!(forgotten, expected);
}

#[test]
fn a_new_session_past_the_most_ends_the_one_used_longest_ago() {
    let mut sessions = Sessions::default();
    for index in 0..MAX_SESSIONS {
        let byte = u8::try_from(index).unwrap();
        sessions.start(
            token(byte),
            token(200),
            PASSWORD,
            false,
            false,
            at(i64::from(byte)),
        );
    }
    // The first is used again, so the second is the one used longest ago.
    assert!(sessions.use_at(&token(0), PASSWORD, at(100)).is_some());
    sessions.start(token(99), token(200), PASSWORD, false, false, at(101));
    assert_eq!(sessions.len(), MAX_SESSIONS);
    assert!(sessions.use_at(&token(1), PASSWORD, at(102)).is_none());
    assert!(sessions.use_at(&token(0), PASSWORD, at(102)).is_some());
    assert!(sessions.use_at(&token(99), PASSWORD, at(102)).is_some());
}

#[test]
fn starting_a_session_forgets_those_that_ended() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    sessions.start(
        token(3),
        token(4),
        PASSWORD,
        false,
        false,
        at(SESSION_IDLE.as_secs()),
    );
    assert_eq!(sessions.len(), 1);
}

#[test]
fn a_new_admin_password_ends_every_session_started_under_the_old_one() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    sessions.start(token(3), token(4), PASSWORD, false, false, at(0));
    assert!(sessions.use_at(&token(1), NEW_PASSWORD, at(1)).is_none());
    // Ended, so the old password, were it kept again, doesn't bring it back.
    assert!(sessions.use_at(&token(1), PASSWORD, at(2)).is_none());
    // A session that starts under the new password ends the old ones too.
    sessions.start(token(5), token(6), NEW_PASSWORD, false, false, at(3));
    assert_eq!(sessions.len(), 1);
    assert!(sessions.use_at(&token(5), NEW_PASSWORD, at(4)).is_some());
}

#[test]
fn peeking_at_a_session_does_not_use_it() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    let idle = SESSION_IDLE.as_secs();
    let peeked = sessions.peek(&token(1), PASSWORD, at(idle - 1));
    assert!(peeked.is_some_and(|session| session.accepts(&token(2))));
    assert!(sessions.peek(&token(1), PASSWORD, at(idle)).is_none());
    assert!(sessions.peek(&token(1), NEW_PASSWORD, at(1)).is_none());
    assert!(sessions.peek(&token(3), PASSWORD, at(1)).is_none());
}

/// The session `byte` as the store kept it: started at `started`, last used
/// at `last_used`, under `password`.
fn kept(byte: u8, started: i64, last_used: i64, password: PasswordFingerprint) -> KeptSession {
    KeptSession {
        digest: token(byte).digest(),
        password,
        started: at(started),
        last_used: at(last_used),
        keep_signed_in: false,
        secure: false,
    }
}

#[test]
fn a_session_is_kept_as_its_tokens_digest_and_its_times_never_a_token() {
    let mut sessions = Sessions::default();
    let session = sessions.start(token(1), token(2), PASSWORD, false, false, at(5));
    let kept_session = session.kept(token(1).digest());
    assert_eq!(kept_session, kept(1, 5, 5, PASSWORD));
    let shown = format!("{kept_session:?}");
    assert!(!shown.contains(&token(1).to_text()), "{shown}");
    assert!(!shown.contains(&token(2).to_text()), "{shown}");
}

#[test]
fn a_session_s_digest_names_itself_in_debug_output_and_hides_its_bytes() {
    let digest = token(1).digest();
    assert_eq!(format!("{digest:?}"), "SessionDigest(..)");
    assert_eq!(format!("{digest:#?}"), "SessionDigest(..)");
    let mut sessions = Sessions::default();
    let session = sessions.start(token(1), token(2), PASSWORD, false, false, at(5));
    let shown = format!("{:?}", session.kept(digest));
    assert!(shown.contains("digest: SessionDigest(..)"), "{shown}");
}

#[test]
fn a_session_is_kept_by_its_token_s_sha_256_and_found_again_by_those_bytes() {
    // The store keeps a session under its digest's bytes and looks it up by
    // them after a restart: they are the token's SHA-256, one session's never
    // another's.
    let digest = token(1).digest();
    assert_eq!(
        digest.as_bytes(),
        &[
            0x72, 0xcd, 0x6e, 0x84, 0x22, 0xc4, 0x07, 0xfb, 0x6d, 0x09, 0x86, 0x90, 0xf1, 0x13,
            0x0b, 0x7d, 0xed, 0x7e, 0xc2, 0xf7, 0xf5, 0xe1, 0xd3, 0x0b, 0xd9, 0xd5, 0x21, 0xf0,
            0x15, 0x36, 0x37, 0x93,
        ]
    );
    assert_eq!(SessionDigest::from_bytes(*digest.as_bytes()), digest);
    assert_ne!(token(2).digest().as_bytes(), digest.as_bytes());
}

#[test]
fn a_restored_session_keeps_its_times_and_takes_its_new_csrf_token() {
    let mut sessions = Sessions::default();
    let idle = SESSION_IDLE.as_secs();
    assert!(sessions.restore(kept(1, 0, 3600, PASSWORD), token(9), PASSWORD, at(7200)));
    let session = sessions.peek(&token(1), PASSWORD, at(3600 + idle - 1));
    assert!(session.is_some_and(|session| session.accepts(&token(9))));
    assert!(
        sessions
            .peek(&token(1), PASSWORD, at(3600 + idle))
            .is_none()
    );
    let ends = sessions
        .peek(&token(1), PASSWORD, at(7200))
        .and_then(|session| session.ends_at(SessionLength::ThirtyDays));
    assert_eq!(ends, Some(at(30 * 24 * 3600)));
    assert!(sessions.take_forgotten().is_empty());
}

#[test]
fn a_kept_session_that_has_ended_or_is_of_another_password_is_forgotten_not_restored() {
    let mut sessions = Sessions::default();
    let idle = SESSION_IDLE.as_secs();
    assert!(!sessions.restore(kept(1, 0, 0, PASSWORD), token(9), PASSWORD, at(idle)));
    assert!(!sessions.restore(kept(2, 0, 0, NEW_PASSWORD), token(9), PASSWORD, at(1)));
    let lifetime = 30 * 24 * 3600;
    assert!(!sessions.restore(
        kept(3, 0, lifetime - 1, PASSWORD),
        token(9),
        PASSWORD,
        at(lifetime)
    ));
    assert!(sessions.is_empty());
    assert_eq!(
        sessions.take_forgotten(),
        vec![token(1).digest(), token(2).digest(), token(3).digest()]
    );
    assert!(sessions.take_forgotten().is_empty());
}

#[test]
fn restoring_past_the_most_forgets_the_session_used_longest_ago() {
    let mut sessions = Sessions::default();
    for byte in 0..=u8::try_from(MAX_SESSIONS).unwrap() {
        let used = i64::from(byte) + 100;
        assert!(sessions.restore(kept(byte, 0, used, PASSWORD), token(200), PASSWORD, at(200)));
    }
    assert_eq!(sessions.len(), MAX_SESSIONS);
    assert_eq!(sessions.take_forgotten(), vec![token(0).digest()]);
    assert!(sessions.peek(&token(1), PASSWORD, at(200)).is_some());
}

#[test]
fn a_restored_session_used_before_every_one_of_the_most_is_forgotten_itself() {
    let mut sessions = Sessions::default();
    for byte in 1..=u8::try_from(MAX_SESSIONS).unwrap() {
        assert!(sessions.restore(kept(byte, 0, 100, PASSWORD), token(200), PASSWORD, at(200)));
    }
    assert!(!sessions.restore(kept(0, 0, 99, PASSWORD), token(200), PASSWORD, at(200)));
    assert_eq!(sessions.len(), MAX_SESSIONS);
    assert_eq!(sessions.take_forgotten(), vec![token(0).digest()]);
    assert!(sessions.peek(&token(0), PASSWORD, at(200)).is_none());
    // One used as recently as the oldest kept takes that one's place.
    assert!(sessions.restore(kept(99, 0, 100, PASSWORD), token(200), PASSWORD, at(200)));
    assert_eq!(sessions.len(), MAX_SESSIONS);
    assert_eq!(sessions.take_forgotten().len(), 1);
}

#[test]
fn every_session_that_ends_is_forgotten_for_the_store_once() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    sessions.start(token(3), token(4), PASSWORD, false, false, at(0));
    sessions.end(&token(3));
    assert_eq!(sessions.take_forgotten(), vec![token(3).digest()]);
    // Ended unused: forgotten when it is next looked for.
    let idle = SESSION_IDLE.as_secs();
    assert!(sessions.use_at(&token(1), PASSWORD, at(idle)).is_none());
    assert_eq!(sessions.take_forgotten(), vec![token(1).digest()]);
    // Ended under an old password: forgotten when a session starts.
    sessions.start(token(5), token(6), PASSWORD, false, false, at(idle));
    sessions.start(token(7), token(8), NEW_PASSWORD, false, false, at(idle));
    assert_eq!(sessions.take_forgotten(), vec![token(5).digest()]);
    // A session that was never there is nothing to forget.
    sessions.end(&token(42));
    assert!(sessions.take_forgotten().is_empty());
}

#[test]
fn the_session_used_longest_ago_is_forgotten_when_a_new_one_needs_its_place() {
    let mut sessions = Sessions::default();
    for byte in 0..u8::try_from(MAX_SESSIONS).unwrap() {
        sessions.start(
            token(byte),
            token(200),
            PASSWORD,
            false,
            false,
            at(i64::from(byte)),
        );
    }
    sessions.start(token(99), token(200), PASSWORD, false, false, at(100));
    assert_eq!(sessions.take_forgotten(), vec![token(0).digest()]);
}

#[test]
fn a_sessions_use_is_kept_at_most_once_a_minute() {
    let mut sessions = Sessions::default();
    let minute = SESSION_USE_KEPT_EVERY.as_secs();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(minute - 1))
            .is_some()
    );
    assert_eq!(sessions.use_to_keep(&token(1)), None);
    assert!(sessions.use_at(&token(1), PASSWORD, at(minute)).is_some());
    assert_eq!(
        sessions.use_to_keep(&token(1)),
        Some((token(1).digest(), at(minute)))
    );
    assert_eq!(sessions.use_to_keep(&token(1)), None);
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(2 * minute - 1))
            .is_some()
    );
    assert_eq!(sessions.use_to_keep(&token(1)), None);
    assert_eq!(sessions.use_to_keep(&token(3)), None);
}

#[test]
fn an_ipv4_address_is_its_own_client_also_written_as_ipv6() {
    assert_ne!(v4(1), v4(2));
    let mapped = Ipv4Addr::new(192, 0, 2, 1).to_ipv6_mapped();
    assert_eq!(Client::of(IpAddr::V6(mapped)), v4(1));
}

#[test]
fn ipv6_addresses_of_one_slash_64_network_are_one_client() {
    let one: Ipv6Addr = "2001:db8:0:1::1".parse().unwrap();
    let same: Ipv6Addr = "2001:db8:0:1:ffff::2".parse().unwrap();
    let other: Ipv6Addr = "2001:db8:0:2::1".parse().unwrap();
    assert_eq!(Client::of(IpAddr::V6(one)), Client::of(IpAddr::V6(same)));
    assert_ne!(Client::of(IpAddr::V6(one)), Client::of(IpAddr::V6(other)));
    assert_eq!(
        Client::of(IpAddr::V6(one)),
        Client::V6Network(0x2001_0db8_0000_0001)
    );
}

#[test]
fn each_failed_sign_in_doubles_the_wait_up_to_five_minutes() {
    let mut waits = SignInWaits::default();
    let mut now = at(0);
    for expected in [1, 2, 4, 8, 16, 32, 64, 128, 256, 300, 300] {
        waits.attempt(v4(1), now).unwrap();
        assert_eq!(
            waits.attempt(v4(1), now),
            Err(SignedDuration::from_secs(expected))
        );
        now += SignedDuration::from_secs(expected);
    }
}

#[test]
fn the_wait_never_overflows_however_many_sign_ins_fail() {
    assert_eq!(wait_after(u32::MAX), MAX_WAIT);
    assert_eq!(wait_after(0), SignedDuration::from_secs(1));
}

#[test]
fn a_sign_in_during_the_wait_says_what_is_left_and_counts_nothing() {
    let mut waits = SignInWaits::default();
    waits.attempt(v4(1), at(0)).unwrap();
    waits.attempt(v4(1), at(1)).unwrap();
    // Two failed: the next waits 2 s, from 1 s.
    assert_eq!(
        waits.attempt(v4(1), at(2)),
        Err(SignedDuration::from_secs(1))
    );
    assert_eq!(
        waits.attempt(v4(1), at(2)),
        Err(SignedDuration::from_secs(1))
    );
    waits.attempt(v4(1), at(3)).unwrap();
    assert_eq!(
        waits.attempt(v4(1), at(3)),
        Err(SignedDuration::from_secs(4))
    );
}

#[test]
fn a_success_starts_the_client_over_and_no_other() {
    let mut waits = SignInWaits::default();
    waits.attempt(v4(1), at(0)).unwrap();
    waits.attempt(v4(2), at(0)).unwrap();
    waits.succeeded(v4(1));
    assert_eq!(waits.attempt(v4(1), at(0)), Ok(()));
    assert_eq!(
        waits.attempt(v4(2), at(0)),
        Err(SignedDuration::from_secs(1))
    );
}

#[test]
fn a_client_quiet_for_a_day_starts_over() {
    let mut waits = SignInWaits::default();
    for second in [0, 1, 3, 7] {
        waits.attempt(v4(1), at(second)).unwrap();
    }
    let day = FORGET_AFTER.as_secs();
    waits.attempt(v4(1), at(7 + day - 1)).unwrap();
    // Five in a row: 16 s.
    assert_eq!(
        waits.attempt(v4(1), at(7 + day - 1)),
        Err(SignedDuration::from_secs(16))
    );
    let mut fresh = SignInWaits::default();
    fresh.attempt(v4(1), at(0)).unwrap();
    fresh.attempt(v4(1), at(day)).unwrap();
    assert_eq!(
        fresh.attempt(v4(1), at(day)),
        Err(SignedDuration::from_secs(1))
    );
}

#[test]
fn the_table_holds_the_most_clients_and_keeps_the_one_that_waits_longest() {
    let mut waits = SignInWaits::default();
    let mut now = at(0);
    for wait in [1, 2, 4, 8, 16, 32, 64, 128, 256] {
        waits.attempt(v4(1), now).unwrap();
        now += SignedDuration::from_secs(wait);
    }
    waits.attempt(v4(1), now).unwrap();
    for index in 0..3000u32 {
        let address = Ipv4Addr::from(0x0a00_0000 + index);
        waits.attempt(Client::of(IpAddr::V4(address)), now).unwrap();
        assert!(waits.len() <= MAX_CLIENTS);
    }
    assert_eq!(waits.len(), MAX_CLIENTS);
    assert_eq!(waits.attempt(v4(1), now), Err(MAX_WAIT));
}

#[test]
fn a_full_table_forgets_quiet_clients_before_any_that_waits() {
    let mut waits = SignInWaits::default();
    let day = FORGET_AFTER.as_secs();
    waits.attempt(v4(1), at(0)).unwrap();
    for index in 1..u32::try_from(MAX_CLIENTS).unwrap() {
        let address = Ipv4Addr::from(0x0a00_0000 + index);
        waits
            .attempt(Client::of(IpAddr::V4(address)), at(day))
            .unwrap();
    }
    assert_eq!(waits.len(), MAX_CLIENTS);
    waits.attempt(v4(2), at(day)).unwrap();
    assert_eq!(waits.len(), MAX_CLIENTS);
    // Every client that waits stays, and the quiet one went.
    assert_eq!(
        waits.attempt(Client::of(IpAddr::V4(Ipv4Addr::from(0x0a00_0001))), at(day)),
        Err(SignedDuration::from_secs(1))
    );
    assert_eq!(
        waits.attempt(v4(2), at(day)),
        Err(SignedDuration::from_secs(1))
    );
    assert_eq!(waits.attempt(v4(1), at(day)), Ok(()));
}

#[test]
fn making_room_forgets_every_client_quiet_for_a_whole_day() {
    let mut waits = SignInWaits::default();
    let day = FORGET_AFTER.as_secs();
    waits.attempt(v4(1), at(0)).unwrap();
    waits.attempt(v4(2), at(0)).unwrap();
    for index in 2..u32::try_from(MAX_CLIENTS).unwrap() {
        let address = Ipv4Addr::from(0x0a00_0000 + index);
        waits
            .attempt(Client::of(IpAddr::V4(address)), at(day))
            .unwrap();
    }
    assert_eq!(waits.len(), MAX_CLIENTS);
    // Both went quiet exactly a day ago: both are forgotten, not only the one
    // a full table would have to let go.
    waits.attempt(v4(3), at(day)).unwrap();
    assert_eq!(waits.len(), MAX_CLIENTS - 1);
}

#[test]
fn a_table_is_empty_until_it_holds_something() {
    let mut waits = SignInWaits::default();
    assert!(waits.is_empty());
    waits.attempt(v4(1), at(0)).unwrap();
    assert!(!waits.is_empty());
    let mut sessions = Sessions::default();
    assert!(sessions.is_empty());
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    assert!(!sessions.is_empty());
}

#[test]
fn debug_names_a_password_or_a_token_but_never_shows_it() {
    let password = format!(
        "{:?}",
        Password::parse("demo-admin-pass".to_owned()).unwrap()
    );
    assert!(!password.contains("demo-admin-pass"), "{password}");
    assert!(password.contains("Password"), "the type helps: {password}");
    let token = token(0xab);
    let shown = format!("{token:?}");
    assert!(!shown.contains(&token.to_text()), "{shown}");
    assert!(!shown.contains("abab"), "{shown}");
    assert!(shown.contains("Token"), "the type helps: {shown}");
}

#[test]
fn waiting_tells_what_is_left_of_a_wait_and_counts_nothing() {
    let mut waits = SignInWaits::default();
    assert_eq!(waits.waiting(v4(1), at(0)), None);
    assert_eq!(waits.attempt(v4(1), at(0)), Ok(()));
    assert_eq!(
        waits.waiting(v4(1), at(0)),
        Some(SignedDuration::from_secs(1))
    );
    assert_eq!(
        waits.waiting(v4(1), Timestamp::constant(1_800_000_000, 400_000_000)),
        Some(SignedDuration::from_millis(600))
    );
    assert_eq!(waits.waiting(v4(1), at(1)), None);
    assert_eq!(waits.waiting(v4(2), at(0)), None);
    // Looking counted nothing: the next failure is the second, 2 s.
    assert_eq!(waits.attempt(v4(1), at(1)), Ok(()));
    assert_eq!(
        waits.waiting(v4(1), at(1)),
        Some(SignedDuration::from_secs(2))
    );
}

#[test]
fn keep_me_signed_in_lifts_the_idle_limit_but_never_the_end_after_sign_in() {
    let mut sessions = Sessions::default();
    let session = sessions.start(token(1), token(2), PASSWORD, true, false, at(0));
    assert_eq!(session.idle_limit(SessionLength::ThirtyDays), None);
    assert!(session.kept(token(1).digest()).keep_signed_in);
    let unkept = sessions.start(token(3), token(4), PASSWORD, false, false, at(0));
    assert_eq!(
        unkept.idle_limit(SessionLength::ThirtyDays),
        Some(SignedDuration::from_hours(12))
    );
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(13 * 3600))
            .is_some()
    );
    assert!(
        sessions
            .use_at(&token(3), PASSWORD, at(13 * 3600))
            .is_none()
    );
    // Unused from the 13th hour until the 30th day, and live all along.
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(30 * DAY - 1))
            .is_some()
    );
    assert!(sessions.use_at(&token(1), PASSWORD, at(30 * DAY)).is_none());
}

#[test]
fn until_the_browser_closes_gives_every_session_the_idle_limit_keep_me_signed_in_or_not() {
    let idle = Some(SignedDuration::from_hours(12));
    let mut sessions = Sessions::default();
    sessions.set_length(SessionLength::UntilBrowserCloses, at(0));
    let asked = sessions.start(token(1), token(2), PASSWORD, true, false, at(0));
    assert_eq!(asked.idle_limit(SessionLength::UntilBrowserCloses), idle);
    // Asked for where it isn't offered, it is never had: not once the
    // length lasts after all.
    assert_eq!(asked.idle_limit(SessionLength::ThirtyDays), idle);
    assert!(!asked.kept(token(1).digest()).keep_signed_in);
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(12 * 3600))
            .is_none()
    );
    // One that has it loses its use while the length is Until I close the
    // browser.
    let mut sessions = Sessions::default();
    let kept = sessions.start(token(1), token(2), PASSWORD, true, false, at(0));
    assert_eq!(kept.idle_limit(SessionLength::UntilBrowserCloses), idle);
    sessions.set_length(SessionLength::UntilBrowserCloses, at(12 * 3600));
    assert!(sessions.is_empty());
}

#[test]
fn the_sign_in_form_offers_keep_me_signed_in_under_every_length_but_until_the_browser_closes() {
    for length in SessionLength::ALL {
        assert_eq!(
            length.offers_keep_signed_in(),
            length != SessionLength::UntilBrowserCloses,
            "{length:?}"
        );
    }
}

#[test]
fn a_cookie_lasts_as_long_as_the_session_has_left_at_most_400_days() {
    // In days, written out: a length read back from the code would follow
    // any change to it.
    for (length, days) in [
        (SessionLength::SevenDays, 7),
        (SessionLength::ThirtyDays, 30),
        (SessionLength::NinetyDays, 90),
        (SessionLength::OneYear, 365),
        (SessionLength::UntilSignOut, 400),
    ] {
        let mut sessions = Sessions::default();
        sessions.set_length(length, at(0));
        let session = sessions.start(token(1), token(2), PASSWORD, true, false, at(0));
        assert_eq!(
            session.cookie_at(at(0), length),
            CookieLife::Lasts(SignedDuration::from_secs(days * DAY)),
            "{length:?}"
        );
        let left = if length == SessionLength::UntilSignOut {
            400 * DAY
        } else {
            days * DAY - DAY - 1
        };
        assert_eq!(
            session.cookie_at(at(DAY + 1), length),
            CookieLife::Lasts(SignedDuration::from_secs(left)),
            "{length:?}"
        );
    }
    let session = Sessions::default().start(token(1), token(2), PASSWORD, false, false, at(0));
    assert_eq!(
        session.cookie_at(at(0), SessionLength::UntilBrowserCloses),
        CookieLife::UntilBrowserCloses
    );
    // Past its end, a session's cookie has nothing left, never a negative
    // age.
    assert_eq!(
        session.cookie_at(at(31 * DAY), SessionLength::ThirtyDays),
        CookieLife::Lasts(SignedDuration::ZERO)
    );
}

#[test]
fn a_use_renews_the_cookie_at_most_once_a_day() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, true, false, at(0));
    assert_eq!(sessions.renew_cookie(&token(1), at(DAY - 1)), None);
    assert_eq!(
        sessions.renew_cookie(&token(1), at(DAY)),
        Some(CookieLife::Lasts(SignedDuration::from_secs(29 * DAY)))
    );
    assert_eq!(sessions.renew_cookie(&token(1), at(DAY + 3600)), None);
    assert_eq!(sessions.renew_cookie(&token(1), at(2 * DAY - 1)), None);
    assert_eq!(
        sessions.renew_cookie(&token(1), at(2 * DAY)),
        Some(CookieLife::Lasts(SignedDuration::from_secs(28 * DAY)))
    );
    assert_eq!(sessions.renew_cookie(&token(3), at(3 * DAY)), None);
}

#[test]
fn an_until_i_sign_out_cookie_is_renewed_to_400_days_every_day() {
    let mut sessions = Sessions::default();
    sessions.set_length(SessionLength::UntilSignOut, at(0));
    sessions.start(token(1), token(2), PASSWORD, true, false, at(0));
    for day in 1..=401 {
        assert_eq!(
            sessions.renew_cookie(&token(1), at(day * DAY)),
            Some(CookieLife::Lasts(SignedDuration::from_secs(400 * DAY))),
            "day {day}"
        );
    }
}

#[test]
fn a_new_session_length_renews_the_cookie_at_the_next_use() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, true, false, at(0));
    sessions.set_length(SessionLength::SevenDays, at(3600));
    assert_eq!(
        sessions.renew_cookie(&token(1), at(3600)),
        Some(CookieLife::Lasts(SignedDuration::from_secs(7 * DAY - 3600)))
    );
    assert_eq!(sessions.renew_cookie(&token(1), at(7200)), None);
    // A longer one too, so the cookie never ends before the session does.
    sessions.set_length(SessionLength::NinetyDays, at(7200));
    assert_eq!(
        sessions.renew_cookie(&token(1), at(7200)),
        Some(CookieLife::Lasts(SignedDuration::from_secs(
            90 * DAY - 7200
        )))
    );
    assert_eq!(sessions.renew_cookie(&token(1), at(10_800)), None);
    // The same length chosen again changes nothing.
    sessions.set_length(SessionLength::NinetyDays, at(10_800));
    assert_eq!(sessions.renew_cookie(&token(1), at(10_800)), None);
}

#[test]
fn a_change_to_or_from_until_the_browser_closes_renews_the_cookie_at_the_next_use() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, false, false, at(0));
    sessions.set_length(SessionLength::UntilBrowserCloses, at(60));
    assert_eq!(
        sessions.renew_cookie(&token(1), at(60)),
        Some(CookieLife::UntilBrowserCloses)
    );
    // A cookie the browser drops as it closes needs no renewal.
    assert!(sessions.use_at(&token(1), PASSWORD, at(DAY / 3)).is_some());
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(2 * DAY / 3))
            .is_some()
    );
    assert_eq!(sessions.renew_cookie(&token(1), at(2 * DAY / 3)), None);
    assert!(sessions.use_at(&token(1), PASSWORD, at(DAY + 60)).is_some());
    assert_eq!(sessions.renew_cookie(&token(1), at(DAY + 60)), None);
    sessions.set_length(SessionLength::ThirtyDays, at(DAY + 120));
    assert_eq!(
        sessions.renew_cookie(&token(1), at(DAY + 120)),
        Some(CookieLife::Lasts(SignedDuration::from_secs(29 * DAY - 120)))
    );
}

#[test]
fn a_restored_session_keeps_keep_me_signed_in_and_renews_its_cookie_at_its_first_use() {
    let mut sessions = Sessions::default();
    let mut kept_session = kept(1, 0, 0, PASSWORD);
    kept_session.keep_signed_in = true;
    assert!(sessions.restore(kept_session, token(9), PASSWORD, at(13 * 3600)));
    assert!(
        sessions
            .use_at(&token(1), PASSWORD, at(13 * 3600))
            .is_some()
    );
    assert_eq!(
        sessions.renew_cookie(&token(1), at(13 * 3600)),
        Some(CookieLife::Lasts(SignedDuration::from_secs(
            30 * DAY - 13 * 3600
        )))
    );
    assert_eq!(sessions.renew_cookie(&token(1), at(14 * 3600)), None);
}

#[test]
fn a_session_s_cookie_stays_secure_as_its_sign_in_set_it_also_after_a_restart() {
    for secure in [true, false] {
        let mut sessions = Sessions::default();
        let session = sessions.start(token(1), token(2), PASSWORD, false, secure, at(0));
        assert_eq!(session.secure(), secure);
        let kept_session = session.kept(token(1).digest());
        assert_eq!(kept_session.secure, secure);
        let mut restarted = Sessions::default();
        assert!(restarted.restore(kept_session, token(9), PASSWORD, at(60)));
        let found = restarted.use_at(&token(1), PASSWORD, at(60));
        assert_eq!(found.map(Session::secure), Some(secure));
    }
}

#[test]
fn a_kept_session_started_after_now_is_forgotten_not_restored() {
    // As a clock set back a year leaves it: started and used a year ahead.
    // Counted from now at every start, it would outlive its length.
    let year = 365 * DAY;
    let mut sessions = Sessions::default();
    assert!(!sessions.restore(kept(1, year, year, PASSWORD), token(9), PASSWORD, at(0)));
    assert!(sessions.peek(&token(1), PASSWORD, at(0)).is_none());
    assert_eq!(sessions.take_forgotten(), vec![token(1).digest()]);
    assert!(sessions.restore(kept(2, 0, 0, PASSWORD), token(9), PASSWORD, at(0)));
}

#[test]
fn a_kept_session_used_after_now_is_forgotten_not_restored() {
    // As a clock set back leaves it: signed in before now, last used a year
    // ahead. Counted as now, its last use would move forward at every start,
    // as if each start were a use.
    let mut sessions = Sessions::default();
    let restored = kept(1, 0, 365 * DAY, PASSWORD);
    assert!(!sessions.restore(restored, token(9), PASSWORD, at(DAY)));
    assert!(sessions.peek(&token(1), PASSWORD, at(DAY)).is_none());
    assert_eq!(sessions.take_forgotten(), vec![token(1).digest()]);
    assert!(sessions.restore(kept(2, 0, DAY, PASSWORD), token(9), PASSWORD, at(DAY)));
}
