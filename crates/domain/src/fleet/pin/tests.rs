use super::*;

const HUB: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const RECOVERY: &str = "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c";
const CERT: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn key(hex: &str) -> PublicKey {
    PublicKey::from_hex(hex).unwrap()
}

fn pin() -> HubPin {
    HubPin::new(
        key(HUB),
        TokenTls::Pinned(Fingerprint::from_digest([0x5a; 32])),
        key(RECOVERY),
        HubAddress::parse("https://hub.example:9898").unwrap(),
    )
}

fn text(lines: &[&str]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

#[test]
fn a_new_pin_starts_at_the_first_generation_and_no_recovery() {
    let pin = pin();
    assert_eq!(pin.recovery_number, 0);
    assert_eq!(pin.generation, 1);
    assert_eq!(pin.hub_key, key(HUB));
    assert_eq!(pin.recovery_key, key(RECOVERY));
    assert_eq!(pin.address.as_str(), "https://hub.example:9898");
}

#[test]
fn a_pin_is_written_as_name_value_lines() {
    let mut pin = pin();
    pin.tls = TokenTls::Pinned(Fingerprint::from_digest(
        <[u8; 32]>::try_from((0..32u8).map(|i| (i % 16) * 0x11).collect::<Vec<_>>()).unwrap(),
    ));
    pin.recovery_number = 3;
    pin.generation = 7;
    assert_eq!(
        pin.to_text(),
        text(&[
            &format!("hub-key = {HUB}"),
            "tls = 00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
            &format!("recovery-key = {RECOVERY}"),
            "recovery-number = 3",
            "generation = 7",
            "address = https://hub.example:9898",
        ])
    );
}

#[test]
fn a_pin_reads_back_what_it_wrote() {
    let mut behind_a_proxy = pin();
    behind_a_proxy.tls = TokenTls::Ca;
    behind_a_proxy.recovery_number = u64::MAX;
    behind_a_proxy.generation = 0;
    for pin in [pin(), behind_a_proxy] {
        assert_eq!(HubPin::parse(&pin.to_text()), Ok(pin.clone()));
    }
    assert!(pin().to_text().contains("\ntls = 5a5a"));
    let mut ca = pin();
    ca.tls = TokenTls::Ca;
    assert!(ca.to_text().contains("\ntls = ca\n"));
}

/// Decision 120: a reader ignores names it doesn't know and keeps them when
/// it rewrites the file, so a newer agent's pin survives an older agent.
#[test]
fn names_this_build_doesnt_know_are_kept_in_their_order() {
    let newer = text(&[
        "hub-release = 0.9.0",
        &format!("hub-key = {HUB}"),
        &format!("tls = {CERT}"),
        "statement-address = https://hub.example:443 pinned",
        &format!("recovery-key = {RECOVERY}"),
        "recovery-number = 2",
        "",
        "  generation   =   5  ",
        "address = https://hub.example:9898",
        "hub-release = 0.9.1",
        "empty =",
    ]);
    let pin = HubPin::parse(&newer).unwrap();
    assert_eq!(pin.generation, 5);
    assert_eq!(pin.recovery_number, 2);
    assert_eq!(
        pin.tls,
        TokenTls::Pinned(Fingerprint::from_digest([
            0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
            0xff, 0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff
        ]))
    );
    let written = pin.to_text();
    assert!(
        written.ends_with(&text(&[
            "address = https://hub.example:9898",
            "hub-release = 0.9.0",
            "statement-address = https://hub.example:443 pinned",
            "hub-release = 0.9.1",
            "empty = ",
        ])),
        "{written}"
    );
    assert_eq!(HubPin::parse(&written), Ok(pin));
}

/// Decision 120: a newer agent's pin may hold a few dozen addresses (the
/// person's own and the latest transport statement's) under names this
/// build doesn't know; it is read and kept whole.
#[test]
fn a_newer_pin_of_a_few_dozen_addresses_is_read_and_kept() {
    let mut newer = pin().to_text();
    for n in 0..48 {
        newer.push_str(&format!(
            "statement-address = https://hub-{n:02}.example:9898 pinned\n"
        ));
    }
    assert!(newer.len() > 2 * 1024, "{}", newer.len());
    let read = HubPin::parse(&newer).unwrap();
    assert_eq!(read.to_text(), newer);
}

#[test]
fn a_pin_with_windows_line_breaks_and_no_last_one_is_read() {
    let crlf = pin().to_text().replace('\n', "\r\n");
    assert_eq!(HubPin::parse(&crlf), Ok(pin()));
    let unended = pin().to_text();
    assert_eq!(HubPin::parse(unended.trim_end()), Ok(pin()));
}

#[test]
fn a_line_that_isnt_a_name_and_value_is_refused_by_its_number() {
    let good = pin().to_text();
    for bad in [
        "no equals sign",
        "= value",
        "Hub-Key = value",
        "hub key = value",
        "hub_key = value",
        "名前 = value",
    ] {
        let text = format!("{good}\n{bad}\n");
        assert_eq!(
            HubPin::parse(&text),
            Err(PinRefused::Malformed { line: 8 }),
            "{bad:?}"
        );
    }
}

#[test]
fn a_known_name_given_twice_is_refused() {
    let text = format!("{}generation = 9\n", pin().to_text());
    assert_eq!(
        HubPin::parse(&text),
        Err(PinRefused::Repeated { name: "generation" })
    );
}

#[test]
fn every_known_name_but_none_is_needed() {
    let whole = pin().to_text();
    for name in KNOWN {
        let without: String = whole
            .lines()
            .filter(|line| !line.starts_with(&format!("{name} =")))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(
            HubPin::parse(&without),
            Err(PinRefused::Missing { name }),
            "{name}"
        );
    }
}

#[test]
fn a_value_that_cant_be_read_is_refused_by_its_name() {
    let small_order = "0000000000000000000000000000000000000000000000000000000000000000";
    for (name, value) in [
        ("hub-key", small_order),
        ("hub-key", "not hex"),
        ("tls", "CA"),
        ("tls", CERT.get(1..).unwrap()),
        ("recovery-key", HUB.get(2..).unwrap()),
        ("recovery-number", "-1"),
        ("recovery-number", "+1"),
        ("recovery-number", "18446744073709551616"),
        ("generation", "one"),
        ("generation", ""),
        ("address", "ftp://hub.example"),
    ] {
        let text: String = pin()
            .to_text()
            .lines()
            .map(|line| {
                if line.starts_with(&format!("{name} =")) {
                    format!("{name} = {value}\n")
                } else {
                    format!("{line}\n")
                }
            })
            .collect();
        let name = KNOWN.into_iter().find(|known| *known == name).unwrap();
        assert_eq!(
            HubPin::parse(&text),
            Err(PinRefused::Unreadable { name }),
            "{name} = {value}"
        );
    }
}

#[test]
fn a_pin_past_the_most_bytes_is_refused_unread() {
    let mut text = pin().to_text();
    text.push_str(&"x".repeat(MAX_PIN_BYTES - text.len() - 4));
    text.push_str(" = \n");
    assert_eq!(text.len(), MAX_PIN_BYTES);
    assert!(HubPin::parse(&text).is_ok());
    text.push('\n');
    assert_eq!(HubPin::parse(&text), Err(PinRefused::TooLong));
}

/// A pin is written back as `name = value` lines, which a pin of compact
/// `name=value` lines outgrows: one that would be written past the most
/// bytes is refused when it is read, so every pin read can be written and
/// read again.
#[test]
fn a_pin_that_would_be_written_past_the_most_bytes_is_refused() {
    let mut text = pin().to_text();
    let mut n = 0;
    while text.len() + 12 <= MAX_PIN_BYTES {
        text.push_str(&format!("x{n:07}=1\n"));
        n += 1;
    }
    assert!(text.len() <= MAX_PIN_BYTES, "{}", text.len());
    assert_eq!(HubPin::parse(&text), Err(PinRefused::TooLong));
    let mut fits = pin().to_text();
    while fits.len() + 14 <= MAX_PIN_BYTES {
        fits.push_str(&format!("x{n:07} = 1\n"));
        n += 1;
    }
    let read = HubPin::parse(&fits).unwrap();
    assert_eq!(read.to_text(), fits);
}

fn address(text: &str) -> HubAddress {
    HubAddress::parse(text).unwrap()
}

/// Decision 120: the address the person gives a paired machine is kept in
/// `hub-pin` beside the token's, written after it and read back.
#[test]
fn an_address_the_person_gave_is_written_after_the_tokens_and_read_back() {
    let mut pin = pin();
    assert!(
        pin.give_address(address("https://hub-new.example:9898"))
            .unwrap()
    );
    let written = pin.to_text();
    assert!(
        written.ends_with(&text(&[
            "address = https://hub.example:9898",
            "given-address = https://hub-new.example:9898",
        ])),
        "{written}"
    );
    assert_eq!(HubPin::parse(&written), Ok(pin.clone()));
    assert_eq!(
        pin.given_address,
        Some(address("https://hub-new.example:9898"))
    );
}

/// Decision 120: the agent tries the address the person gave, then the
/// token's, which no later address removes.
#[test]
fn the_address_the_person_gave_is_tried_before_the_tokens() {
    let mut pin = pin();
    let tried = |pin: &HubPin| -> Vec<String> {
        pin.addresses()
            .map(|address| address.as_str().to_owned())
            .collect()
    };
    assert_eq!(tried(&pin), ["https://hub.example:9898"]);
    assert!(
        pin.give_address(address("https://hub-new.example:9898"))
            .unwrap()
    );
    assert_eq!(
        tried(&pin),
        ["https://hub-new.example:9898", "https://hub.example:9898"]
    );
    assert!(
        pin.give_address(address("https://hub-newer.example:9898"))
            .unwrap()
    );
    assert_eq!(
        tried(&pin),
        ["https://hub-newer.example:9898", "https://hub.example:9898"]
    );
}

/// Giving the address already kept changes nothing, and giving the
/// token's own address drops the other, so no address is tried twice.
#[test]
fn giving_an_address_already_kept_changes_nothing() {
    let mut pin = pin();
    assert!(
        !pin.give_address(address("https://hub.example:9898"))
            .unwrap()
    );
    assert_eq!(pin, self::pin());
    assert!(
        pin.give_address(address("https://hub-new.example:9898"))
            .unwrap()
    );
    assert!(
        !pin.give_address(address("https://hub-new.example:9898"))
            .unwrap()
    );
    assert!(
        pin.give_address(address("https://hub.example:9898"))
            .unwrap()
    );
    assert_eq!(pin, self::pin());
    assert!(!pin.to_text().contains("given-address"));
}

#[test]
fn a_given_address_that_cant_be_read_or_is_given_twice_is_refused() {
    let good = pin().to_text();
    assert_eq!(
        HubPin::parse(&format!("{good}given-address = ftp://hub.example\n")),
        Err(PinRefused::Unreadable {
            name: "given-address"
        })
    );
    assert_eq!(
        HubPin::parse(&format!(
            "{good}given-address = https://a.example\ngiven-address = https://b.example\n"
        )),
        Err(PinRefused::Repeated {
            name: "given-address"
        })
    );
}

/// A pin a given address would make too long to read again keeps what it
/// had, so a machine never writes a pin it can't read.
#[test]
fn an_address_that_would_make_the_pin_too_long_is_refused() {
    let mut text = pin().to_text();
    let mut n = 0;
    while text.len() + 14 <= MAX_PIN_BYTES {
        text.push_str(&format!("x{n:07} = 1\n"));
        n += 1;
    }
    let mut full = HubPin::parse(&text).unwrap();
    assert_eq!(
        full.give_address(address("https://hub-new.example:9898")),
        Err(PinRefused::TooLong)
    );
    assert_eq!(full.given_address, None);
    assert_eq!(full.to_text(), text);
}

/// A given address that brings the pin to exactly [`MAX_PIN_BYTES`] fits.
#[test]
fn an_address_that_makes_the_pin_exactly_the_most_bytes_is_kept() {
    let given = address("https://hub-new.example:9898");
    let mut with_given = pin();
    with_given.give_address(given.clone()).unwrap();
    let given_bytes = with_given.to_text().len() - pin().to_text().len();
    let mut text = pin().to_text();
    let mut n = 0;
    while MAX_PIN_BYTES - given_bytes - text.len() > 40 {
        text.push_str(&format!("x{n:07} = 1\n"));
        n += 1;
    }
    let rest = MAX_PIN_BYTES - given_bytes - text.len();
    text.push_str(&format!("y = {}\n", "1".repeat(rest - 5)));
    let mut full = HubPin::parse(&text).unwrap();
    assert_eq!(full.give_address(given), Ok(true));
    assert_eq!(full.to_text().len(), MAX_PIN_BYTES);
}
