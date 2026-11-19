use super::*;

/// The bytes of hex digits, two a byte.
fn message(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    message(hex).try_into().unwrap()
}

/// RFC 8032 §7.1, tests 1 to 3: seed, public key, message, signature.
const RFC_8032: [(&str, &str, &str, &str); 3] = [
    (
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "",
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    ),
    (
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        "72",
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    ),
    (
        "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        "af82",
        "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    ),
];

#[test]
fn the_rfc_8032_vectors_hold() {
    for (seed, public, text, signature) in RFC_8032 {
        let seed = SigningSeed::from_bytes(bytes(seed));
        let key = AwsLcSigning.public_key(&seed).unwrap();
        assert_eq!(key.as_bytes(), &bytes::<KEY_BYTES>(public));
        let signed = sign_bytes(&seed, &message(text)).unwrap();
        assert_eq!(signed, bytes::<SIGNATURE_BYTES>(signature));
        assert!(verify_bytes(key.as_bytes(), &message(text), &signed));
    }
}

/// `S + L` is the malleated copy RFC 8032 §5.1.7 refuses; the library
/// refuses it too, and `Signature` never holds it.
#[test]
fn a_malleated_signature_is_refused_by_the_library() {
    let (_, public, _, signature) = RFC_8032[0];
    let mut malleated = bytes::<SIGNATURE_BYTES>(signature);
    let order = message("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
    let mut carry = 0u16;
    for (s, l) in malleated[KEY_BYTES..].iter_mut().zip(order) {
        let sum = u16::from(*s) + u16::from(l) + carry;
        *s = u8::try_from(sum & 0xff).unwrap();
        carry = sum >> 8;
    }
    assert!(!verify_bytes(&bytes(public), b"", &malleated));
    assert!(Signature::from_bytes(malleated).is_err());
}

/// The forgery decision 120 names: under the identity key, `R = identity`
/// with `S = 0` verifies for every message in the library alone, so the key
/// table in `domain` is what stops it, and `PublicKey` refuses the key.
#[test]
fn the_library_alone_would_accept_the_small_order_forgery() {
    let mut identity = [0; KEY_BYTES];
    identity[0] = 1;
    let mut forged = [0; SIGNATURE_BYTES];
    forged[0] = 1;
    for text in [&b""[..], b"any message", b"another"] {
        assert!(verify_bytes(&identity, text, &forged));
    }
    assert!(PublicKey::from_bytes(identity).is_err());
}
