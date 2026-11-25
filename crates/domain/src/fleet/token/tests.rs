use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use super::*;

/// Made apart from this code, from the layout in decision 120: hub key
/// `11…11`, pinned to the fingerprint `55…55`, seed `66…66`, address
/// `https://hub.example:9899`.
const PINNED: &str = "ladderback_pair_ARERERERERERERERERERERERERERERERERERERERERERAVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmYYaHR0cHM6Ly9odWIuZXhhbXBsZTo5ODk5k5Jo1w";

/// The same with a CA address: TLS byte 0, fingerprint zeros.
const CA: &str = "ladderback_pair_ARERERERERERERERERERERERERERERERERERERERERERAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmYYaHR0cHM6Ly9odWIuZXhhbXBsZTo5ODk5hJr_pg";

fn token(tls: TokenTls, address: &str) -> PairingToken {
    PairingToken::new(
        PublicKey::from_bytes([0x11; KEY_BYTES]).unwrap(),
        tls,
        SigningSeed::from_bytes([0x66; KEY_BYTES]),
        HubAddress::parse(address).unwrap(),
    )
}

fn pinned() -> TokenTls {
    TokenTls::Pinned(Fingerprint::from_digest([0x55; KEY_BYTES]))
}

#[test]
fn a_token_encodes_to_the_fixed_vector() {
    let address = "https://hub.example:9899";
    assert_eq!(token(pinned(), address).to_text().expose(), PINNED);
    assert_eq!(token(TokenTls::Ca, address).to_text().expose(), CA);
}

#[test]
fn the_fixed_vector_reads_back_field_by_field() {
    let read = PairingToken::parse(PINNED).unwrap();
    assert_eq!(read.hub_key().as_bytes(), &[0x11; KEY_BYTES]);
    assert_eq!(read.tls(), pinned());
    assert_eq!(read.pairing_seed().expose(), &[0x66; KEY_BYTES]);
    assert_eq!(read.address().as_str(), "https://hub.example:9899");
    assert_eq!(PairingToken::parse(CA).unwrap().tls(), TokenTls::Ca);
}

/// 103 bytes plus the address, as decision 120 counts them.
#[test]
fn a_token_is_103_bytes_plus_its_address() {
    let text = token(pinned(), "https://hub.example:9899").to_text();
    let encoded = text.expose().strip_prefix(TOKEN_PREFIX).unwrap();
    let bytes = URL_SAFE_NO_PAD.decode(encoded).unwrap();
    assert_eq!(bytes.len(), 103 + "https://hub.example:9899".len());
    assert_eq!(bytes.len(), FIXED_BYTES + "https://hub.example:9899".len());
}

/// The longest address, 200 bytes, makes the longest token, which reads back.
#[test]
fn a_token_with_the_longest_address_reads_back() {
    let address = format!("https://{}", "h".repeat(MAX_ADDRESS_BYTES - 8));
    let text = token(pinned(), &address).to_text();
    assert_eq!(text.expose().len(), 420);
    assert_eq!(text.expose().len(), MAX_TOKEN_TEXT);
    let read = PairingToken::parse(text.expose()).unwrap();
    assert_eq!(read.address().as_str(), address);
}

#[test]
fn a_token_never_shows_its_seed_in_its_debug_form() {
    let shown = format!("{:?}", PairingToken::parse(PINNED).unwrap());
    assert!(shown.contains("SigningSeed(***)"), "{shown}");
    // The seed is 32 bytes of 0x66: in no form, decimal, hex or base64.
    for seed in ["102, 102", "6666", "66, 66", "ZmZm"] {
        assert!(!shown.contains(seed), "{seed} in {shown}");
    }
}

/// Encodes `bytes` with a checksum that holds, so only the field under test
/// is wrong.
fn with_checksum(mut bytes: Vec<u8>) -> String {
    let sum = checksum(&bytes);
    bytes.extend_from_slice(&sum);
    format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

fn body_of(text: &str) -> Vec<u8> {
    let mut bytes = URL_SAFE_NO_PAD
        .decode(text.strip_prefix(TOKEN_PREFIX).unwrap())
        .unwrap();
    bytes.truncate(bytes.len() - CHECKSUM_BYTES);
    bytes
}

#[test]
fn a_wrong_prefix_is_malformed() {
    for text in [
        PINNED.replacen("ladderback_pair_", "ladderback-pair-", 1),
        PINNED.replacen("ladderback_pair_", "LADDERBACK_PAIR_", 1),
        PINNED.strip_prefix(TOKEN_PREFIX).unwrap().to_owned(),
        String::new(),
        TOKEN_PREFIX.to_owned(),
    ] {
        assert_eq!(
            PairingToken::parse(&text).unwrap_err(),
            TokenRefused::Malformed,
            "{text}"
        );
    }
}

/// One character changed anywhere fails the checksum or the decoding.
#[test]
fn a_changed_character_is_malformed() {
    for at in TOKEN_PREFIX.len()..PINNED.len() {
        let mut text = PINNED.to_owned().into_bytes();
        text[at] = if text[at] == b'A' { b'B' } else { b'A' };
        let text = String::from_utf8(text).unwrap();
        assert_eq!(
            PairingToken::parse(&text).unwrap_err(),
            TokenRefused::Malformed,
            "at {at}"
        );
    }
}

/// The last character's unused bits must be zero, padding is refused, and
/// the standard alphabet's `+` and `/` aren't base64url: one token, one text.
#[test]
fn a_non_canonical_encoding_is_malformed() {
    let last = PINNED.len() - 1;
    let mut bits = PINNED.to_owned();
    // `w` (48) and `x` (49) differ only in a bit past the token's last byte.
    assert!(bits.ends_with('w'));
    bits.replace_range(last.., "x");
    let padded = format!("{PINNED}==");
    let standard = CA.replace('_', "/");
    for text in [bits, padded, standard, format!("{PINNED} ")] {
        assert_eq!(
            PairingToken::parse(&text).unwrap_err(),
            TokenRefused::Malformed,
            "{text}"
        );
    }
}

#[test]
fn a_later_version_is_unsupported() {
    let mut body = body_of(PINNED);
    body[0] = 2;
    assert_eq!(
        PairingToken::parse(&with_checksum(body)).unwrap_err(),
        TokenRefused::Unsupported
    );
    // A later version may have another layout, so its length isn't checked.
    assert_eq!(
        PairingToken::parse(&with_checksum(vec![2])).unwrap_err(),
        TokenRefused::Unsupported
    );
}

/// No token had version 0, so one that says so is a mistake, not a token
/// from a later Ladderback.
#[test]
fn version_0_is_malformed() {
    let mut body = body_of(PINNED);
    body[0] = 0;
    assert_eq!(
        PairingToken::parse(&with_checksum(body)).unwrap_err(),
        TokenRefused::Malformed
    );
    assert_eq!(
        PairingToken::parse(&with_checksum(vec![0])).unwrap_err(),
        TokenRefused::Malformed
    );
}

#[test]
fn a_short_or_long_token_is_malformed() {
    let body = body_of(PINNED);
    for cut in [body.len() - 1, 1 + 32, 1, 0] {
        assert_eq!(
            PairingToken::parse(&with_checksum(body[..cut].to_vec())).unwrap_err(),
            TokenRefused::Malformed,
            "cut at {cut}"
        );
    }
    let mut long = body.clone();
    long.push(b'x');
    assert_eq!(
        PairingToken::parse(&with_checksum(long)).unwrap_err(),
        TokenRefused::Malformed
    );
    // Fewer than the 4 bytes of a checksum.
    let short = format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode([1, 2, 3]));
    assert_eq!(
        PairingToken::parse(&short).unwrap_err(),
        TokenRefused::Malformed
    );
}

/// Text past the longest token is refused before it is decoded.
#[test]
fn text_longer_than_any_token_is_malformed() {
    let text = format!("{TOKEN_PREFIX}{}", "A".repeat(MAX_TOKEN_TEXT));
    assert_eq!(
        PairingToken::parse(&text).unwrap_err(),
        TokenRefused::Malformed
    );
    // A later version longer than the longest token of this one, checksum
    // and all, is refused by its length before its version is read: this
    // build can't tell it from text that isn't a token.
    let mut later = vec![2; MAX_TOKEN_TEXT];
    later.truncate((MAX_TOKEN_TEXT - TOKEN_PREFIX.len()) / 4 * 3);
    let later = with_checksum(later);
    assert!(later.len() > MAX_TOKEN_TEXT);
    assert_eq!(
        PairingToken::parse(&later).unwrap_err(),
        TokenRefused::Malformed
    );
    let mut body = body_of(PINNED);
    let at = 1 + 32 + 1 + 32 + 32;
    body.truncate(at);
    body.push(201);
    body.extend_from_slice(format!("https://{}", "h".repeat(193)).as_bytes());
    assert_eq!(
        PairingToken::parse(&with_checksum(body)).unwrap_err(),
        TokenRefused::Malformed
    );
}

#[test]
fn a_small_order_hub_key_is_malformed() {
    let mut body = body_of(PINNED);
    body[1..33].copy_from_slice(&SMALL_ORDER_KEYS[0]);
    assert_eq!(
        PairingToken::parse(&with_checksum(body)).unwrap_err(),
        TokenRefused::Malformed
    );
}

#[test]
fn a_tls_byte_other_than_0_or_1_is_malformed() {
    let mut body = body_of(PINNED);
    body[33] = 2;
    assert_eq!(
        PairingToken::parse(&with_checksum(body)).unwrap_err(),
        TokenRefused::Malformed
    );
}

/// A CA token carries zeros where a pinned one has the fingerprint, so a CA
/// token has one text.
#[test]
fn a_ca_token_with_a_fingerprint_is_malformed() {
    let mut body = body_of(PINNED);
    body[33] = 0;
    assert_eq!(
        PairingToken::parse(&with_checksum(body)).unwrap_err(),
        TokenRefused::Malformed
    );
}

#[test]
fn an_address_that_isnt_one_is_malformed() {
    let at = 1 + 32 + 1 + 32 + 32;
    for address in [&b"ftp://hub"[..], b"https://", b"https://a b", b"\xff\xfe"] {
        let mut body = body_of(PINNED)[..at].to_vec();
        body.push(u8::try_from(address.len()).unwrap());
        body.extend_from_slice(address);
        assert_eq!(
            PairingToken::parse(&with_checksum(body)).unwrap_err(),
            TokenRefused::Malformed,
            "{address:?}"
        );
    }
}

#[test]
fn a_hub_address_is_https_or_http_and_a_host() {
    for address in [
        "https://hub.example:9899",
        "http://127.0.0.1:9898",
        "https://[::1]:9899",
    ] {
        assert_eq!(HubAddress::parse(address).unwrap().as_str(), address);
    }
    let longest = format!("https://{}", "h".repeat(MAX_ADDRESS_BYTES - 8));
    assert!(HubAddress::parse(&longest).is_ok());
    for address in [
        "",
        "hub.example:9899",
        "https://",
        "http://",
        "https://hub example",
        "https://hub\u{7}",
        "https://hub\u{a0}x",
        &format!("{longest}h"),
    ] {
        assert_eq!(
            HubAddress::parse(address),
            Err(InvalidHubAddress),
            "{address:?}"
        );
    }
}

/// An `http://` address is plain and shows no certificate; an `https://`
/// one is reached over TLS (#1343).
#[test]
fn a_hub_addresss_scheme_is_https_or_http() {
    for (address, scheme) in [
        ("https://hub.example:9899", HubScheme::Https),
        ("https://[::1]:9899", HubScheme::Https),
        ("http://127.0.0.1:9898", HubScheme::Http),
        ("http://localhost", HubScheme::Http),
    ] {
        assert_eq!(
            HubAddress::parse(address).unwrap().scheme(),
            scheme,
            "{address}"
        );
    }
}

/// A token wrapped across lines, quoted, with a trailing newline, with
/// zero-width characters a chat app added, or with its prefix's case and
/// underscores changed is read as the token it is (decision 121, "`pair`").
#[test]
fn a_pasted_token_is_read_through_what_a_paste_adds() {
    let (head, tail) = PINNED.split_at(60);
    let (prefix, body) = PINNED.split_at(TOKEN_PREFIX.len());
    for pasted in [
        format!("{PINNED}\n"),
        format!("  {head}\n{tail}\r\n"),
        format!("\"{PINNED}\""),
        format!("'{PINNED}'"),
        format!("<{PINNED}>"),
        format!("\u{201c}{PINNED}\u{201d}"),
        format!("`\"{PINNED}\"`"),
        format!("{head}\u{200b}{tail}\u{feff}"),
        format!("{}{body}", prefix.to_uppercase()),
        format!("{}{body}", prefix.replace('_', "-")),
    ] {
        let read = PairingToken::parse_pasted(&pasted);
        assert_eq!(
            read.map(|token| token.to_text().expose().to_owned()),
            Ok(PINNED.to_owned()),
            "{pasted:?}"
        );
    }
}

/// Base64 is case-sensitive: a letter of the body whose case a chat app
/// changed fails the checksum, and so does a body cut short. Text far
/// longer than any paste of a token is refused unread.
#[test]
fn a_pasted_token_with_its_body_changed_is_malformed() {
    let (prefix, body) = PINNED.split_at(TOKEN_PREFIX.len());
    for pasted in [
        format!("{prefix}{}", body.to_lowercase()),
        PINNED.get(..PINNED.len() - 3).unwrap().to_owned(),
        format!("\"{PINNED}"),
        format!("ladderback-pair{body}"),
        String::new(),
        " ".repeat(MAX_PASTED_TEXT + 1),
    ] {
        assert_eq!(
            PairingToken::parse_pasted(&pasted).map(|_| ()),
            Err(TokenRefused::Malformed),
            "{pasted:?}"
        );
    }
    let spaced = format!("{PINNED}{}", " ".repeat(MAX_PASTED_TEXT - PINNED.len()));
    assert!(PairingToken::parse_pasted(&spaced).is_ok());
}

/// `pair <address>` keeps the token's hub key, certificate and seed, and
/// connects to the address given.
#[test]
fn a_token_with_another_address_keeps_its_keys() {
    let address = HubAddress::parse("https://192.0.2.7:9899").unwrap();
    let moved = PairingToken::parse(PINNED)
        .unwrap()
        .with_address(address.clone());
    assert_eq!(moved.address(), &address);
    assert_eq!(moved.hub_key().as_bytes(), &[0x11; KEY_BYTES]);
    assert_eq!(moved.tls(), pinned());
    assert_eq!(moved.pairing_seed().expose(), &[0x66; KEY_BYTES]);
}

use super::super::keys::SMALL_ORDER_KEYS;

/// The longest token, wrapped after every character with a Windows line
/// break and a zero-width character, is still read.
#[test]
fn the_longest_token_wrapped_on_every_character_is_read() {
    let address = format!("https://{}", "h".repeat(MAX_ADDRESS_BYTES - 8));
    let text = token(pinned(), &address).to_text();
    let wrapped: String = text
        .expose()
        .chars()
        .flat_map(|c| [c, '\r', '\n', '\u{200b}'])
        .collect();
    let read = PairingToken::parse_pasted(&wrapped).unwrap();
    assert_eq!(read.address().as_str(), address);
}

/// Only the token's own prefix, in any case and with `-` for `_`, is read
/// as it: other text of its length in its place is malformed.
#[test]
fn a_pasted_token_with_another_prefix_is_malformed() {
    let (_, body) = PINNED.split_at(TOKEN_PREFIX.len());
    let other = "x".repeat(TOKEN_PREFIX.len());
    assert_eq!(
        PairingToken::parse_pasted(&format!("{other}{body}")).map(|_| ()),
        Err(TokenRefused::Malformed)
    );
}
