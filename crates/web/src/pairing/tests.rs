use super::*;

#[test]
fn a_loopback_address_is_warned_of_as_one_another_machine_cant_reach() {
    for address in [
        "http://127.0.0.1:9898",
        "https://localhost:9443",
        "HTTP://LOCALHOST:9898/",
        "http://[::1]:8080",
        "https://127.1:9443",
        "https://127.1.2.3:9443/sync",
        " http://127.0.0.1:9898 ",
    ] {
        assert_eq!(
            AddressWarning::of(address),
            Some(AddressWarning::Loopback),
            "{address}"
        );
    }
}

#[test]
fn a_plain_http_address_beyond_loopback_is_warned_of_as_one_an_agent_refuses() {
    for address in ["http://hub.example:8080", "http://192.168.1.10:9898/"] {
        assert_eq!(
            AddressWarning::of(address),
            Some(AddressWarning::Plain),
            "{address}"
        );
    }
}

#[test]
fn an_https_address_beyond_loopback_or_one_the_hub_refuses_is_not_warned_of() {
    for address in [
        "https://hub.example:9443",
        "https://192.168.1.10:9899/",
        "https://localhost.example:9443",
        "https://[2001:db8::1]:9443",
        "ftp://127.0.0.1:21",
        "",
    ] {
        assert_eq!(AddressWarning::of(address), None, "{address}");
    }
}

#[test]
fn an_addresss_scheme_is_read_in_any_case_and_none_for_another() {
    for (address, scheme) in [
        ("http://127.0.0.1:9898", Some(Scheme::Http)),
        (" HTTP://LOCALHOST:9898/ ", Some(Scheme::Http)),
        ("http://hub.example:8080", Some(Scheme::Http)),
        ("https://hub.example:9443", Some(Scheme::Https)),
        ("HTTPS://127.1:9443", Some(Scheme::Https)),
        ("ftp://127.0.0.1:21", None),
        ("hub.example:9443", None),
        ("", None),
    ] {
        assert_eq!(Scheme::of(address), scheme, "{address:?}");
    }
}

#[test]
fn a_token_counts_down_whole_seconds_from_the_hub_s_answer() {
    let at = |millis| Duration::from_millis(millis);
    let countdown = Countdown::new(900, at(10_000));
    assert_eq!(countdown.left(at(10_000)), 900);
    assert_eq!(countdown.left(at(10_999)), 900);
    assert_eq!(countdown.left(at(11_000)), 899);
    assert_eq!(countdown.left(at(909_999)), 1);
    assert_eq!(countdown.left(at(910_000)), 0);
    assert_eq!(countdown.left(at(u64::MAX)), 0);
    // A clock read before the answer came leaves the whole life.
    assert_eq!(countdown.left(at(5_000)), 900);
}

#[test]
fn the_minutes_left_are_rounded_up() {
    for (seconds, said) in [(900, 15), (899, 15), (61, 2), (60, 1), (1, 1), (0, 0)] {
        assert_eq!(minutes(seconds), said, "{seconds}");
    }
}

#[test]
fn a_life_is_read_in_whole_minutes() {
    assert_eq!(life("15"), Some(15));
    assert_eq!(life(" 1440 "), Some(1440));
    for typed in ["", "fifteen", "-5", "+5", "1.5", "15 minutes"] {
        assert_eq!(life(typed), None, "{typed:?}");
    }
}

/// The same table as `crates/domain/src/fleet/route/tests.rs`, so the page
/// warns of the hosts an agent takes for this computer: keep the two
/// identical. A part with a leading `0` or `0x` is read apart by resolvers
/// (octal by glibc, decimal by macOS in four parts), so it is never
/// loopback.
#[test]
fn loopback_is_localhost_or_a_loopback_address_in_a_form_every_resolver_reads_alike() {
    for host in [
        "localhost",
        "LOCALHOST",
        "127.0.0.1",
        "127.255.0.1",
        "::1",
        "127.1",
        "127.0.1",
        "2130706433",
    ] {
        assert!(is_loopback(host), "{host}");
    }
    for host in [
        "hub.example",
        "10.0.0.1",
        "::",
        "fe80::1",
        "localhost.",
        "128.1",
        "0x80.1",
        "2130706431",
        "127.256.0.1",
        "127.0.0.0.1",
        "127.0.0.1.0",
        "127..1",
        "127.",
        "0x",
        "08.1",
        "0x7f.1",
        "0177.0.0.1",
        "0177.0.1",
        "127.0.0.01",
        "0x7f.0.0.1",
        "+127.1",
    ] {
        assert!(!is_loopback(host), "{host}");
    }
}

/// Settings shows its Machines tab once the page has read the hub's key in
/// the standing it is in: a read made signed out says nothing of a page
/// signed in since, which reads again.
#[test]
fn a_hub_is_known_once_its_key_is_read_in_the_standing_the_page_is_in() {
    for (read, now, known) in [
        (None, None, None),
        (None, Some(Standing::SignedIn), None),
        (Some((Standing::Off, true)), None, Some(true)),
        (Some((Standing::Off, false)), None, Some(false)),
        (
            Some((Standing::SignedIn, true)),
            Some(Standing::SignedIn),
            Some(true),
        ),
        (
            Some((Standing::SignedIn, false)),
            Some(Standing::SignedIn),
            Some(false),
        ),
        (
            Some((Standing::SignedOut, true)),
            Some(Standing::SignedIn),
            None,
        ),
    ] {
        assert_eq!(is_hub(read, now), known, "{read:?}, {now:?}");
    }
}

#[test]
fn the_hubs_own_certificate_is_offered_first_only_while_it_serves_one() {
    for (named, serves_own, first) in [
        (Some(TokenCertificateDto::Pinned), true, PINNED),
        (Some(TokenCertificateDto::Pinned), false, CA),
        (Some(TokenCertificateDto::Ca), true, CA),
        (None, true, CA),
        (None, false, CA),
    ] {
        assert_eq!(
            first_choice(named, serves_own),
            first,
            "{named:?}, {serves_own}"
        );
    }
}

#[test]
fn only_a_plain_http_address_is_plain() {
    for (address, plain) in [
        ("http://hub.example:9898", true),
        (" HTTP://127.0.0.1:9898 ", true),
        ("https://hub.example:9443", false),
        ("hub.example:9898", false),
    ] {
        assert_eq!(Scheme::plain(address), plain, "{address:?}");
    }
}

/// The choice is asked for at the address as typed, but a plain
/// address shows no certificate, so its token pins none, even when the
/// hub's own was chosen before it was typed (#1343).
#[test]
fn a_token_is_asked_for_at_the_address_typed_and_a_plain_one_pins_nothing() {
    for (typed, chosen, address, certificate) in [
        (
            " http://hub.example:9898 ",
            PINNED,
            "http://hub.example:9898",
            TokenCertificateDto::Ca,
        ),
        (
            "HTTP://127.0.0.1:9898",
            PINNED,
            "HTTP://127.0.0.1:9898",
            TokenCertificateDto::Ca,
        ),
        (
            "http://hub.example:9898",
            CA,
            "http://hub.example:9898",
            TokenCertificateDto::Ca,
        ),
        (
            "https://hub.example:9443",
            PINNED,
            "https://hub.example:9443",
            TokenCertificateDto::Pinned,
        ),
        (
            "https://proxy.example",
            CA,
            "https://proxy.example",
            TokenCertificateDto::Ca,
        ),
    ] {
        assert_eq!(
            asked(typed, chosen),
            (address.to_owned(), certificate),
            "{typed}, {chosen}"
        );
    }
}

/// Without the viewer's zone the dialog writes the lapse time in UTC,
/// as `instant` writes one without a zone, so the offset it names beside
/// it is UTC's, never the viewer's (#1271). The viewer's zone is a
/// browser's: `formatting.rs` tests it in `just test-wasm`.
#[test]
fn without_the_viewers_zone_a_tokens_lapse_time_and_the_offset_named_are_utcs() {
    let localizer = Localizer::new(&["en-US".parse().unwrap()]);
    let raw = "2026-11-21T17:25:04Z";
    let utc = instant(&localizer, raw, None);
    assert_eq!(
        lapses(&localizer, None, raw, 15 * 60),
        format!("{utc} (+00:00), in 15 minutes")
    );
    assert_eq!(
        lapsed(&localizer, None, raw),
        format!("Lapsed at {utc}. Make a new token to pair a machine.")
    );
}

#[test]
fn a_token_is_canceled_at_its_own_path_on_the_pages_daemon() {
    assert_eq!(
        cancel_url("https://hub.example:9443", "pt_1"),
        "https://hub.example:9443/api/v1/hub/pairing-tokens/pt_1"
    );
}

#[test]
fn two_ids_name_the_same_token_only_when_both_are_held_and_alike() {
    for (held, other, same) in [
        (Some("pt_1"), Some("pt_1"), true),
        (Some("pt_1"), Some("pt_2"), false),
        (Some("pt_1"), None, false),
        (None, Some("pt_1"), false),
        (None, None, false),
    ] {
        assert_eq!(same_token(held, other), same, "{held:?}, {other:?}");
    }
}

#[test]
fn the_dialog_shows_a_token_only_while_open_and_showing_it() {
    for (open, shown, is_shown) in [
        (true, Some("pt_1"), true),
        (false, Some("pt_1"), false),
        (true, Some("pt_2"), false),
        (true, None, false),
    ] {
        assert_eq!(shows(open, shown, "pt_1"), is_shown, "{open}, {shown:?}");
    }
}

/// The count stops once the token ends, or once the dialog shows another
/// or none: a later token's count is its own.
#[test]
fn a_token_is_counted_down_while_the_dialog_shows_it_and_it_hasnt_ended() {
    for (shown, ended, counted) in [
        (Some("pt_1"), false, true),
        (Some("pt_1"), true, false),
        (Some("pt_2"), false, false),
        (None, false, false),
    ] {
        assert_eq!(
            counts_down(shown, "pt_1", ended),
            counted,
            "{shown:?}, {ended}"
        );
    }
}

/// A cancel already on its way answers for the token: no second one goes
/// out, which would find none. A token that ended has nothing to cancel.
#[test]
fn a_token_is_canceled_once_and_only_while_live() {
    for (ended, canceling, cancels) in [
        (false, None, true),
        (false, Some("pt_2"), true),
        (false, Some("pt_1"), false),
        (true, None, false),
        (true, Some("pt_1"), false),
    ] {
        assert_eq!(
            cancelable(ended, canceling, "pt_1"),
            cancels,
            "{ended}, {canceling:?}"
        );
    }
}

#[test]
fn the_dialog_is_left_only_when_it_was_open_and_no_longer_is() {
    for (was_open, open, is_left) in [
        (Some(true), false, true),
        (Some(true), true, false),
        (Some(false), false, false),
        (Some(false), true, false),
        (None, false, false),
        (None, true, false),
    ] {
        assert_eq!(left(was_open, open), is_left, "{was_open:?}, {open}");
    }
}

/// The last token's cancel is said in full; an earlier token's only when it
/// still works, so its answer is never read as the later token's. A token
/// the hub no longer holds was used or lapsed already: nothing is said of
/// it, and the last token's place says nothing more.
#[test]
fn a_closed_dialogs_cancel_is_said_of_the_last_token_in_full_and_of_an_earlier_if_it_still_works() {
    let done = Posted::Done(String::new());
    let unknown = Posted::Refused(ErrorCode::PairingTokenUnknown, Vec::new(), None);
    let refused = Posted::Refused(ErrorCode::StepUpFailed, Vec::new(), None);
    let answers = [
        ("done", Some(&done)),
        ("unknown", Some(&unknown)),
        ("refused", Some(&refused)),
        ("unanswered", None),
    ];
    let canceled = Some(msg::UI_PAIRING_CLOSED_CANCELED);
    let not_canceled = Some(msg::UI_PAIRING_CLOSED_NOT_CANCELED);
    let still_working = Some(ClosedCancel::Earlier(msg::UI_PAIRING_EARLIER_NOT_CANCELED));
    for (last, said) in [
        (
            true,
            [
                Some(ClosedCancel::Last(canceled)),
                Some(ClosedCancel::Last(None)),
                Some(ClosedCancel::Last(not_canceled)),
                Some(ClosedCancel::Last(not_canceled)),
            ],
        ),
        (false, [None, None, still_working, still_working]),
    ] {
        for ((answer, posted), said) in answers.iter().zip(said) {
            assert_eq!(closed_cancel(last, *posted), said, "{last}, {answer}");
        }
    }
}

#[test]
fn a_cancels_answer_ends_the_wait_only_while_that_tokens_cancel_is_on_its_way() {
    assert!(cancel_on_its_way(Some("t1"), "t1"));
    assert!(
        !cancel_on_its_way(Some("t2"), "t1"),
        "a later token's cancel waits on"
    );
    assert!(!cancel_on_its_way(None, "t1"), "no cancel is on its way");
}

#[test]
fn cancel_this_token_is_busy_only_while_the_shown_tokens_cancel_is_on_its_way() {
    assert!(canceling_shown(Some("t1"), Some("t1")));
    assert!(
        !canceling_shown(Some("t1"), Some("t2")),
        "another token's cancel"
    );
    assert!(!canceling_shown(None, Some("t1")), "no cancel on its way");
    assert!(!canceling_shown(None, None), "no token shown");
}
