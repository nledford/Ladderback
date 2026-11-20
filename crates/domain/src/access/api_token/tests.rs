use super::*;

fn at(seconds: i64) -> Timestamp {
    Timestamp::constant(1_800_000_000 + seconds, 0)
}

fn bytes(seed: u8) -> [u8; TOKEN_BYTES] {
    std::array::from_fn(|index| {
        seed.wrapping_mul(31)
            .wrapping_add(u8::try_from(index).unwrap())
    })
}

fn label(text: &str) -> ApiTokenLabel {
    ApiTokenLabel::new(text.to_owned()).unwrap()
}

const UNDER: PasswordFingerprint = PasswordFingerprint::new(7);

/// A token created from `seed`, with the text a program presents for it.
fn issued(seed: u8) -> (ApiToken, ApiTokenText) {
    let id = ApiTokenId::from_bytes(&bytes(seed));
    let secret = ApiTokenSecret::from_bytes(bytes(seed.wrapping_add(100)));
    let text = ApiTokenText::new(&id, &secret);
    let permissions = TokenPermissions::new(Permissions::default()).unwrap();
    let token = ApiToken::issue(id, label("nightly-mcp"), permissions, &secret, UNDER, at(0));
    (token, text)
}

fn presented(text: &str) -> PresentedApiToken {
    PresentedApiToken::parse(text).unwrap()
}

#[test]
fn a_tokens_text_is_the_prefix_a_base32_id_and_a_base64url_secret() {
    let (token, text) = issued(1);
    let text = text.expose();
    let rest = text.strip_prefix("ladderback_").unwrap();
    let (id, secret) = rest.split_at(12);
    assert_eq!(id, token.id.as_str());
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    );
    let secret = secret.strip_prefix('_').unwrap();
    assert_eq!(secret.len(), 43);
    assert!(
        secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
}

#[test]
fn the_secret_is_written_as_base64url() {
    let id = ApiTokenId::parse("abcdefghijkl").unwrap();
    let text = ApiTokenText::new(&id, &ApiTokenSecret::from_bytes([0xfb; TOKEN_BYTES]));
    // 0xfb 0xfb 0xfb is 111110 111111 101111 111011: "-_v7" in base64url.
    assert_eq!(
        text.expose(),
        "ladderback_abcdefghijkl_-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_v7-_s"
    );
}

fn wiped_on_drop(_: &impl zeroize::ZeroizeOnDrop) {}

/// A token's secret wipes its bytes when it is dropped (it is
/// `ZeroizeOnDrop`); what its drop does is done here to one still in scope.
#[test]
fn a_tokens_secret_wiped_as_its_drop_wipes_it_keeps_none_of_its_bytes() {
    let mut secret = ApiTokenSecret::from_bytes(bytes(7));
    let mut clone = secret.clone();
    wiped_on_drop(&secret);

    zeroize::Zeroize::zeroize(&mut secret);

    assert_eq!(secret.0[..], [0; TOKEN_BYTES]);
    assert_eq!(clone.0[..], bytes(7));
    zeroize::Zeroize::zeroize(&mut clone);
    assert_eq!(clone.0[..], [0; TOKEN_BYTES]);
}

/// A token's text, the secret written out, wipes itself the same way.
#[test]
fn a_tokens_text_wiped_as_its_drop_wipes_it_keeps_none_of_its_bytes() {
    let (_, mut text) = issued(3);
    wiped_on_drop(&text);

    zeroize::Zeroize::zeroize(&mut text);

    assert_eq!(text.expose(), "");
}

#[test]
fn a_token_accepts_the_text_it_was_shown_as() {
    for seed in [0, 1, 2, 77, 255] {
        let (token, text) = issued(seed);
        let presented = presented(text.expose());
        assert_eq!(presented.id(), &token.id);
        assert!(token.accepts(&presented, UNDER), "seed {seed}");
    }
}

#[test]
fn a_token_refuses_another_secret_another_id_and_another_password() {
    let (token, _) = issued(1);
    let (_, other_secret) = issued(2);
    let other = presented(other_secret.expose());
    // The other token's secret under this token's ID.
    let mixed = format!(
        "ladderback_{}_{}",
        token.id.as_str(),
        other_secret.expose().get(24..).unwrap()
    );
    assert!(!token.accepts(&other, UNDER));
    assert!(!token.accepts(&presented(&mixed), UNDER));
    let (_, own) = issued(1);
    assert!(!token.accepts(&presented(own.expose()), PasswordFingerprint::new(8)));
}

#[test]
fn a_token_with_this_secret_under_another_id_is_refused() {
    let (token, own) = issued(1);
    let other_id = format!("ladderback_bbbbbbbbbbbb{}", own.expose().get(23..).unwrap());
    assert!(!token.accepts(&presented(&other_id), UNDER));
}

#[test]
fn only_the_exact_text_is_a_token() {
    let (_, text) = issued(3);
    let good = text.expose().to_owned();
    let secret = good.get(24..).unwrap();
    for bad in [
        String::new(),
        good.replacen("ladderback_", "stdg_", 1),
        good.replacen("ladderback_", "Ladderback_", 1),
        format!("{good}A"),
        good.get(..good.len() - 1).unwrap().to_owned(),
        format!("ladderback_ABCDEFGHIJKL_{secret}"),
        format!("ladderback_abcdefghijk1_{secret}"),
        format!("ladderback_abcdefghijkl-{secret}"),
        format!(
            "ladderback_abcdefghijkl_{}",
            secret.replacen(|_| true, "=", 1)
        ),
        format!(
            "ladderback_abcdefghijkl_{}",
            secret.replacen(|_| true, "é", 1)
        ),
        format!("ladderback_abcdefghijk_{secret}A"),
    ] {
        assert!(PresentedApiToken::parse(&bad).is_none(), "{bad:?}");
    }
}

#[test]
fn a_secret_whose_unused_last_bits_are_set_is_no_token() {
    let canonical = format!("ladderback_abcdefghijkl_{}A", "A".repeat(42));
    assert!(PresentedApiToken::parse(&canonical).is_some());
    for last in ["B", "C", "D"] {
        let other = format!("ladderback_abcdefghijkl_{}{last}", "A".repeat(42));
        assert!(PresentedApiToken::parse(&other).is_none(), "{last}");
    }
    let next = format!("ladderback_abcdefghijkl_{}E", "A".repeat(42));
    assert!(PresentedApiToken::parse(&next).is_some());
}

#[test]
fn ids_are_twelve_base32_characters_of_the_first_sixty_bits() {
    let id = ApiTokenId::from_bytes(&[0; TOKEN_BYTES]);
    assert_eq!(id.as_str(), "aaaaaaaaaaaa");
    // 00000 00001 00010 … 01011, then four bits no character uses.
    let mut counting = [0; TOKEN_BYTES];
    counting[..8].copy_from_slice(&[0x00, 0x44, 0x32, 0x14, 0xc7, 0x42, 0x54, 0xbf]);
    assert_eq!(ApiTokenId::from_bytes(&counting).as_str(), "abcdefghijkl");
    assert_eq!(
        ApiTokenId::from_bytes(&[0xff; TOKEN_BYTES]).as_str(),
        "777777777777"
    );
}

#[test]
fn bytes_that_differ_in_their_first_sixty_bits_give_different_ids() {
    // Every bit counts: bytes that differ only above their low five bits
    // (1 and 33) are two IDs, not one.
    let mut first = [0; TOKEN_BYTES];
    let mut second = [0; TOKEN_BYTES];
    first[0] = 1;
    second[0] = 33;
    assert_ne!(
        ApiTokenId::from_bytes(&first),
        ApiTokenId::from_bytes(&second)
    );
    first[0] = 0;
    second[0] = 0;
    second[7] = 0x10;
    assert_ne!(
        ApiTokenId::from_bytes(&first),
        ApiTokenId::from_bytes(&second)
    );
}

#[test]
fn an_id_is_parsed_only_from_twelve_base32_characters() {
    assert!(ApiTokenId::parse("abcdefghijkl").is_some());
    assert!(ApiTokenId::parse("abcdefghijk").is_none());
    assert!(ApiTokenId::parse("abcdefghijkl1").is_none());
    assert!(ApiTokenId::parse("abcdefghijk8").is_none());
}

#[test]
fn a_label_is_one_to_sixty_four_characters_not_blank_without_control_characters() {
    assert!(ApiTokenLabel::new("a".to_owned()).is_ok());
    assert!(ApiTokenLabel::new("é".repeat(64)).is_ok());
    for bad in [
        String::new(),
        "   ".to_owned(),
        "a".repeat(65),
        "é".repeat(65),
        "line\nbreak".to_owned(),
        "tab\tbed".to_owned(),
    ] {
        assert_eq!(
            ApiTokenLabel::new(bad.clone()),
            Err(ApiTokenProblem::Label),
            "{bad:?}"
        );
    }
}

#[test]
fn a_token_needs_one_permission_at_least() {
    assert_eq!(
        TokenPermissions::new(Permissions::NONE),
        Err(ApiTokenProblem::NoPermission)
    );
    let log_only = Permissions::NONE
        .with(
            crate::access::permissions::Permission::DaemonLog,
            Some(crate::access::permissions::Level::Read),
        )
        .unwrap();
    assert_eq!(
        TokenPermissions::new(log_only).map(TokenPermissions::get),
        Ok(log_only)
    );
}

#[test]
fn a_use_is_recorded_first_and_then_once_a_minute_at_most() {
    let (mut token, _) = issued(1);
    assert!(token.use_to_record(at(0)));
    token.last_used_at = Some(at(10));
    assert!(!token.use_to_record(at(10)));
    assert!(!token.use_to_record(at(69)));
    assert!(token.use_to_record(at(70)));
    assert!(token.use_to_record(at(10_000)));
}

#[test]
fn a_new_token_was_created_now_and_never_used() {
    let (token, _) = issued(1);
    assert_eq!(token.created_at, at(0));
    assert_eq!(token.last_used_at, None);
    assert_eq!(token.password, UNDER);
    assert_eq!(token.label.as_str(), "nightly-mcp");
}

#[test]
fn no_debug_output_shows_the_secret() {
    let (token, text) = issued(9);
    let secret = text.expose().get(24..).unwrap().to_owned();
    let shown = format!(
        "{token:?} {text:?} {:?} {:?}",
        presented(text.expose()),
        ApiTokenSecret::from_bytes(bytes(109))
    );
    assert!(!shown.contains(&secret), "{shown}");
    assert!(!shown.contains(text.expose()), "{shown}");
    assert!(shown.contains(token.id.as_str()), "{shown}");
}

#[test]
fn what_holds_a_secret_names_itself_in_debug_output_and_hides_the_rest() {
    let (token, text) = issued(9);
    assert_eq!(format!("{text:?}"), "ApiTokenText(***)");
    assert_eq!(format!("{:?}", token.digest), "ApiTokenDigest(***)");
    assert_eq!(
        format!("{:?}", ApiTokenSecret::from_bytes(bytes(109))),
        "ApiTokenSecret(***)"
    );
    assert!(
        format!("{:?}", presented(text.expose())).contains("secret: ApiTokenSecret(***)"),
        "a presented token's secret is shown hidden"
    );
}

#[test]
fn a_token_kept_as_its_digest_s_bytes_and_password_s_value_accepts_its_text_again() {
    // A store keeps a token's digest as bytes and its password as a number,
    // and builds the token again from them when a request presents it.
    let (token, text) = issued(4);
    let (other, _) = issued(5);
    let read_back = |token: &ApiToken| ApiToken {
        digest: ApiTokenDigest::from_bytes(*token.digest.as_bytes()),
        password: PasswordFingerprint::new(token.password.value()),
        ..token.clone()
    };
    assert!(read_back(&token).accepts(&presented(text.expose()), UNDER));
    let mixed = ApiToken {
        digest: read_back(&other).digest,
        ..token
    };
    assert!(!mixed.accepts(&presented(text.expose()), UNDER));
}

#[test]
fn a_token_cancels_backups_and_no_other_operation() {
    use crate::execution::OperationKind;
    assert!(cancels(OperationKind::Backup));
    for kind in [
        OperationKind::Fake,
        OperationKind::Identify,
        OperationKind::Init,
        OperationKind::Forget,
        OperationKind::Adopt,
        OperationKind::Prune,
        OperationKind::Check,
        OperationKind::Ls,
    ] {
        assert!(!cancels(kind), "{kind:?}");
    }
}

#[test]
fn a_token_is_kept_while_fewer_than_the_limit_are_kept() {
    let limit = MAX_API_TOKENS;
    assert_eq!(admits_api_token(0, limit), Ok(()));
    assert_eq!(admits_api_token(limit - 1, limit), Ok(()));
    assert_eq!(admits_api_token(limit, limit), Err(ApiTokensFull));
    assert_eq!(admits_api_token(limit + 1, limit), Err(ApiTokensFull));
    assert_eq!(admits_api_token(0, 0), Err(ApiTokensFull));
}
