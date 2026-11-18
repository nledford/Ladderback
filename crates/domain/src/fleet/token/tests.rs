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
}

/// The longest address, 200 bytes, makes the longest token, which reads back.
#[test]
fn a_token_with_the_longest_address_reads_back() {
    let address = format!("https://{}", "h".repeat(MAX_ADDRESS_BYTES - 8));
    let text = token(pinned(), &address).to_text();
    assert_eq!(text.expose().len(), MAX_TOKEN_TEXT);
    let read = PairingToken::parse(text.expose()).unwrap();
    assert_eq!(read.address().as_str(), address);
}

#[test]
fn a_token_never_shows_its_seed_in_its_debug_form() {
    let shown = format!("{:?}", PairingToken::parse(PINNED).unwrap());
    assert!(shown.contains("SigningSeed(***)"), "{shown}");
    assert!(!shown.contains("102, 102"), "{shown}");
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
    assert_eq!(
        PairingToken::parse(&with_checksum(vec![0])).unwrap_err(),
        TokenRefused::Unsupported
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

use super::super::keys::SMALL_ORDER_KEYS;
