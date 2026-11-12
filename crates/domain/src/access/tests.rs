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
    assert!(Token::parse(&text[..63]).is_none());
    assert!(Token::parse(&format!("{text}0")).is_none());
    assert!(Token::parse(&text.to_uppercase()).is_none());
    assert!(Token::parse(&text.replacen('a', "g", 1)).is_none());
    assert!(Token::parse(&format!("é{}", &text[2..])).is_none());
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
    sessions.start(token(1), token(2), PASSWORD, at(0));
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
    sessions.start(token(1), token(2), PASSWORD, at(0));
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
    let session = sessions.start(token(1), token(2), PASSWORD, at(0));
    assert!(session.live_at(at(SESSION_IDLE.as_secs() - 1)));
    assert!(!session.live_at(at(SESSION_IDLE.as_secs())));
}

#[test]
fn a_session_ends_thirty_days_after_it_started_however_it_is_used() {
    let mut sessions = Sessions::default();
    let session = sessions.start(token(1), token(2), PASSWORD, at(0));
    // Thirty days, written out: a lifetime read back from the constant would
    // follow any change to it.
    let lifetime = 30 * 24 * 3600;
    assert_eq!(session.ends_at(), at(lifetime));
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
    sessions.start(token(1), token(2), PASSWORD, at(0));
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
    let session = sessions.start(token(1), token(2), PASSWORD, late);
    assert_eq!(session.ends_at(), Timestamp::MAX);
    assert!(session.live_at(late));
}

#[test]
fn a_session_accepts_its_own_csrf_token_only() {
    let mut sessions = Sessions::default();
    let session = sessions.start(token(1), token(2), PASSWORD, at(0));
    assert!(session.accepts(&token(2)));
    assert!(!session.accepts(&token(1)));
    assert!(session.csrf().matches(&token(2)));
}

#[test]
fn signing_out_ends_one_session_only() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, at(0));
    sessions.start(token(3), token(4), PASSWORD, at(0));
    sessions.end(&token(1));
    assert!(sessions.use_at(&token(1), PASSWORD, at(1)).is_none());
    assert!(sessions.use_at(&token(3), PASSWORD, at(1)).is_some());
    assert_eq!(sessions.len(), 1);
}

#[test]
fn a_new_session_past_the_most_ends_the_one_used_longest_ago() {
    let mut sessions = Sessions::default();
    for index in 0..MAX_SESSIONS {
        let byte = u8::try_from(index).unwrap();
        sessions.start(token(byte), token(200), PASSWORD, at(i64::from(byte)));
    }
    // The first is used again, so the second is the one used longest ago.
    assert!(sessions.use_at(&token(0), PASSWORD, at(100)).is_some());
    sessions.start(token(99), token(200), PASSWORD, at(101));
    assert_eq!(sessions.len(), MAX_SESSIONS);
    assert!(sessions.use_at(&token(1), PASSWORD, at(102)).is_none());
    assert!(sessions.use_at(&token(0), PASSWORD, at(102)).is_some());
    assert!(sessions.use_at(&token(99), PASSWORD, at(102)).is_some());
}

#[test]
fn starting_a_session_forgets_those_that_ended() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, at(0));
    sessions.start(token(3), token(4), PASSWORD, at(SESSION_IDLE.as_secs()));
    assert_eq!(sessions.len(), 1);
}

#[test]
fn a_new_admin_password_ends_every_session_started_under_the_old_one() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, at(0));
    sessions.start(token(3), token(4), PASSWORD, at(0));
    assert!(sessions.use_at(&token(1), NEW_PASSWORD, at(1)).is_none());
    // Ended, so the old password, were it kept again, doesn't bring it back.
    assert!(sessions.use_at(&token(1), PASSWORD, at(2)).is_none());
    // A session that starts under the new password ends the old ones too.
    sessions.start(token(5), token(6), NEW_PASSWORD, at(3));
    assert_eq!(sessions.len(), 1);
    assert!(sessions.use_at(&token(5), NEW_PASSWORD, at(4)).is_some());
}

#[test]
fn peeking_at_a_session_does_not_use_it() {
    let mut sessions = Sessions::default();
    sessions.start(token(1), token(2), PASSWORD, at(0));
    let idle = SESSION_IDLE.as_secs();
    let peeked = sessions.peek(&token(1), PASSWORD, at(idle - 1));
    assert!(peeked.is_some_and(|session| session.accepts(&token(2))));
    assert!(sessions.peek(&token(1), PASSWORD, at(idle)).is_none());
    assert!(sessions.peek(&token(1), NEW_PASSWORD, at(1)).is_none());
    assert!(sessions.peek(&token(3), PASSWORD, at(1)).is_none());
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
    sessions.start(token(1), token(2), PASSWORD, at(0));
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
