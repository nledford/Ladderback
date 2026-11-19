use super::*;

/// The bytes of hex digits, two a byte.
fn decode(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn bytes32(hex: &str) -> [u8; KEY_BYTES] {
    decode(hex).try_into().unwrap()
}

/// RFC 8032 §7.1, test 1: an honest key.
const RFC_KEY: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

#[test]
fn an_honest_key_is_accepted_as_it_is() {
    let key = PublicKey::from_bytes(bytes32(RFC_KEY)).unwrap();
    assert_eq!(key.as_bytes(), &bytes32(RFC_KEY));
}

/// The eight small-order encodings, written out in full (each point's order,
/// 1, 2, 4 or 8, was computed independently of this code).
#[test]
fn each_small_order_key_is_refused() {
    for hex in [
        "0100000000000000000000000000000000000000000000000000000000000000",
        "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000080",
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
        "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
        "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
    ] {
        assert_eq!(
            PublicKey::from_bytes(bytes32(hex)),
            Err(KeyRefused::SmallOrder),
            "{hex}"
        );
    }
}

/// `y` at `p` and above (`p` itself is `y = 0`, `p + 1` the identity) is
/// another encoding of a point, so it's refused before the table is read.
#[test]
fn a_key_whose_y_is_not_below_p_is_refused() {
    for hex in [
        // p, p + 1, p + 18 (the largest 255-bit value), each with and
        // without the sign bit.
        "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    ] {
        assert_eq!(
            PublicKey::from_bytes(bytes32(hex)),
            Err(KeyRefused::NotCanonical),
            "{hex}"
        );
    }
}

/// `p - 2`, just below `p - 1`, is a canonical `y`; whether it lies on the
/// curve is the signing library's check, which refuses at verification.
#[test]
fn a_key_whose_y_is_just_below_p_is_not_refused_here() {
    let hex = "ebffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f";
    assert!(PublicKey::from_bytes(bytes32(hex)).is_ok());
}

/// The points with `x = 0` encode without a sign bit; with one they are
/// another encoding of the identity or of `(0, -1)`.
#[test]
fn a_key_whose_x_is_zero_with_a_sign_bit_is_refused() {
    for hex in [
        "0100000000000000000000000000000000000000000000000000000000000080",
        "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    ] {
        assert_eq!(
            PublicKey::from_bytes(bytes32(hex)),
            Err(KeyRefused::NotCanonical),
            "{hex}"
        );
    }
}

/// A sign bit on any other `y` is part of an honest encoding.
#[test]
fn a_sign_bit_on_another_key_is_kept() {
    let mut bytes = bytes32(RFC_KEY);
    bytes[31] |= 0x80;
    assert_eq!(PublicKey::from_bytes(bytes).unwrap().as_bytes(), &bytes);
}

/// RFC 8032 §7.1, test 1's signature.
const RFC_SIGNATURE: &str = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";

fn bytes64(hex: &str) -> [u8; SIGNATURE_BYTES] {
    decode(hex).try_into().unwrap()
}

/// Adds `L` to the little-endian `S` of a signature.
fn plus_order(mut bytes: [u8; SIGNATURE_BYTES]) -> [u8; SIGNATURE_BYTES] {
    let mut carry = 0u16;
    for (s, l) in bytes[KEY_BYTES..].iter_mut().zip(GROUP_ORDER) {
        let sum = u16::from(*s) + u16::from(l) + carry;
        *s = (sum & 0xff) as u8;
        carry = sum >> 8;
    }
    bytes
}

#[test]
fn an_honest_signature_is_accepted_as_it_is() {
    let signature = Signature::from_bytes(bytes64(RFC_SIGNATURE)).unwrap();
    assert_eq!(signature.as_bytes(), &bytes64(RFC_SIGNATURE));
}

/// `S + L` verifies under a cofactorless check like `S` does: one signature
/// would have two encodings.
#[test]
fn a_signature_whose_s_is_not_below_the_order_is_refused() {
    let malleated = plus_order(bytes64(RFC_SIGNATURE));
    assert_eq!(
        Signature::from_bytes(malleated),
        Err(SignatureRefused::NotCanonical)
    );
    let mut at_order = [0; SIGNATURE_BYTES];
    at_order[KEY_BYTES..].copy_from_slice(&GROUP_ORDER);
    assert_eq!(
        Signature::from_bytes(at_order),
        Err(SignatureRefused::NotCanonical)
    );
    let mut top_bits = [0; SIGNATURE_BYTES];
    top_bits[63] = 0xe0;
    assert_eq!(
        Signature::from_bytes(top_bits),
        Err(SignatureRefused::NotCanonical)
    );
}

/// `L - 1`, the largest `S`, is canonical; so is the forger's `S = 0`, which
/// only the key table stops.
#[test]
fn a_signature_whose_s_is_just_below_the_order_is_accepted() {
    let mut below = [0; SIGNATURE_BYTES];
    below[KEY_BYTES..].copy_from_slice(&GROUP_ORDER);
    below[KEY_BYTES] -= 1;
    assert!(Signature::from_bytes(below).is_ok());
    assert!(Signature::from_bytes([0; SIGNATURE_BYTES]).is_ok());
}

/// The forgery of decision 120's table, `R = identity, S = 0`, verifies under
/// the identity key for every message; the identity is never a key here.
#[test]
fn the_identity_key_of_the_forgery_is_refused() {
    let mut identity = [0; KEY_BYTES];
    identity[0] = 1;
    assert_eq!(PublicKey::from_bytes(identity), Err(KeyRefused::SmallOrder));
}

#[test]
fn a_seed_never_shows_in_its_debug_form() {
    let seed = SigningSeed::from_bytes([0x5a; KEY_BYTES]);
    assert_eq!(format!("{seed:?}"), "SigningSeed(***)");
    assert_eq!(seed.expose(), &[0x5a; KEY_BYTES]);
}

#[test]
fn keys_and_signatures_show_their_hex() {
    let key = PublicKey::from_bytes(bytes32(RFC_KEY)).unwrap();
    assert_eq!(format!("{key:?}"), format!("PublicKey({RFC_KEY})"));
    let signature = Signature::from_bytes(bytes64(RFC_SIGNATURE)).unwrap();
    assert_eq!(
        format!("{signature:?}"),
        format!("Signature({RFC_SIGNATURE})")
    );
}

#[test]
fn a_key_reads_back_from_its_hex() {
    let key = PublicKey::from_bytes(bytes32(RFC_KEY)).unwrap();
    assert_eq!(key.to_hex(), RFC_KEY);
    assert_eq!(PublicKey::from_hex(RFC_KEY), Ok(key));
}

#[test]
fn a_key_from_hex_is_checked_as_bytes_are() {
    let small = lower_hex(&SMALL_ORDER_KEYS[2]);
    assert_eq!(PublicKey::from_hex(&small), Err(KeyRefused::SmallOrder));
    let prime = lower_hex(&FIELD_PRIME);
    assert_eq!(PublicKey::from_hex(&prime), Err(KeyRefused::NotCanonical));
}

/// Exactly 64 lower-case hex digits, nothing around them: a file's line
/// break is the caller's to take off.
#[test]
fn other_text_is_no_key() {
    let upper = RFC_KEY.to_uppercase();
    let short = RFC_KEY.get(..62).unwrap();
    let long = format!("{RFC_KEY}00");
    let spaced = format!(" {RFC_KEY}");
    let broken = format!("{RFC_KEY}\n");
    let not_hex = format!("{}g", RFC_KEY.get(..63).unwrap());
    let wide = format!("{}é", RFC_KEY.get(..62).unwrap());
    for text in ["", &upper, short, &long, &spaced, &broken, &not_hex, &wide] {
        assert_eq!(
            PublicKey::from_hex(text),
            Err(KeyRefused::NotHex),
            "{text:?}"
        );
        assert!(SigningSeed::from_hex(text).is_none(), "{text:?}");
    }
}

/// Every digit, both nibbles: `0f`, `f0` and each of `0`–`9`, `a`–`f`.
#[test]
fn a_seed_reads_back_from_its_hex() {
    let mut bytes = [0; KEY_BYTES];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::try_from(index).unwrap().wrapping_mul(0x4f) ^ 0x0f;
    }
    let text = lower_hex(&bytes);
    let seed = SigningSeed::from_hex(&text).unwrap();
    assert_eq!(seed.expose(), &bytes);
    assert_eq!(seed.to_hex().as_str(), text);
    assert_eq!(
        SigningSeed::from_hex("0123456789abcdef".repeat(4).as_str())
            .unwrap()
            .expose()[..8],
        [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
    );
}
