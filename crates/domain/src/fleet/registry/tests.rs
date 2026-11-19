use uuid::Uuid;

use super::*;
use crate::fleet::KEY_BYTES;

fn key(byte: u8) -> PublicKey {
    PublicKey::from_bytes([byte; KEY_BYTES]).unwrap()
}

fn id(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

fn name(text: &str) -> Hostname {
    Hostname::parse(text).unwrap()
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + seconds).unwrap()
}

fn token(state: TokenUse) -> TokenRecord {
    TokenRecord {
        pairing_key: key(2),
        hub_key: key(3),
        kind: BindingKind::None,
        made_at: at(0),
        expires_at: at(900),
        state,
    }
}

fn request() -> PairRequest {
    PairRequest {
        pairing_key: key(2),
        agent_id: id(1),
        agent_key: key(4),
        hostname: name("laptop"),
        counter: 5,
        kind: BindingKind::None,
        hub_key: key(3),
    }
}

fn record(agent: u128, agent_key: PublicKey, hostname: &str) -> AgentRecord {
    AgentRecord {
        agent_id: id(agent),
        key: agent_key,
        hostname: name(hostname),
        status: AgentStatus::Approved { at: at(-100) },
        counter: 9,
        last_seen: at(-50),
    }
}

fn view<'a>(token: &'a TokenRecord) -> PairingView<'a> {
    PairingView {
        token: Some(token),
        by_id: None,
        by_hostname: None,
        pending: 0,
    }
}

/// A token lives 15 minutes unless asked otherwise, and from 5 minutes to
/// 24 hours when asked; a minute either side is refused.
#[test]
fn a_token_lives_fifteen_minutes_or_what_was_asked_within_bounds() {
    assert_eq!(token_life(None), Ok(SignedDuration::from_mins(15)));
    for minutes in [5, 60, 24 * 60] {
        let life = SignedDuration::from_mins(minutes);
        assert_eq!(token_life(Some(life)), Ok(life), "{minutes} minutes");
    }
    for minutes in [4, 24 * 60 + 1, 0, -15] {
        let life = SignedDuration::from_mins(minutes);
        assert_eq!(
            token_life(Some(life)),
            Err(TokenLifeRefused),
            "{minutes} minutes"
        );
    }
}

/// Names that differ only by case, or only by Unicode normalization, are
/// one name on the hub; different names are not.
#[test]
fn hostnames_compare_by_case_fold_of_their_nfc_form() {
    let one = |a: &str, b: &str| HostnameKey::of(&name(a)) == HostnameKey::of(&name(b));
    assert!(one("Laptop", "laptop"));
    assert!(one("LAPTOP", "laptop"));
    assert!(one("caf\u{e9}", "cafe\u{301}"));
    assert!(one("CAF\u{c9}", "cafe\u{301}"));
    assert!(!one("laptop", "desktop"));
    assert!(
        !one("laptop", "l\u{430}ptop"),
        "a lookalike is another name"
    );
    assert_eq!(HostnameKey::of(&name("Caf\u{e9}")).as_str(), "caf\u{e9}");
}

/// A token is unused until it is spent or revoked, and only before it
/// lapses; its ID is its pairing key's.
#[test]
fn a_token_is_unused_only_before_it_lapses() {
    let unused = token(TokenUse::Unused);
    assert!(unused.is_unused_at(at(899)));
    assert!(!unused.is_unused_at(at(900)));
    assert!(!token(TokenUse::Revoked).is_unused_at(at(1)));
    let used = TokenUse::Used {
        agent_id: id(1),
        agent_key: key(4),
    };
    assert!(!token(used).is_unused_at(at(1)));
    assert_eq!(unused.id(), TokenId::of_pairing_key(&key(2)));
}

/// A new agent on an unused token is kept pending from now, under the
/// hostname and with the counter it sent.
#[test]
fn a_new_agent_is_kept_pending() {
    let token = token(TokenUse::Unused);
    let decided = decide_pairing(&view(&token), &request(), at(10));
    assert_eq!(
        decided,
        Ok(Pairing::New(AgentRecord {
            agent_id: id(1),
            key: key(4),
            hostname: name("laptop"),
            status: AgentStatus::Pending { since: at(10) },
            counter: 5,
            last_seen: at(10),
        }))
    );
}

/// A pairing key no token has, or a binding kind other than the token's,
/// is refused as a proof would be.
#[test]
fn an_unknown_token_or_another_kind_is_refused() {
    let token = token(TokenUse::Unused);
    let none = PairingView {
        token: None,
        ..view(&token)
    };
    assert_eq!(
        decide_pairing(&none, &request(), at(10)),
        Err(PairRefusal::ProofRefused)
    );
    let other_kind = PairRequest {
        kind: BindingKind::TlsExporter,
        ..request()
    };
    assert_eq!(
        decide_pairing(&view(&token), &other_kind, at(10)),
        Err(PairRefusal::ProofRefused)
    );
}

/// A token whose hub key isn't the hub's is refused whatever its state,
/// before anything else about it is weighed: a new hub key voids every
/// token made under the old one (decision 120, "Pairing, identities").
#[test]
fn a_token_of_another_hub_key_is_refused() {
    let registered = record(1, key(4), "laptop");
    let other_hub = PairRequest {
        hub_key: key(9),
        ..request()
    };
    for state in [
        TokenUse::Unused,
        TokenUse::Revoked,
        TokenUse::Used {
            agent_id: id(1),
            agent_key: key(4),
        },
    ] {
        let token = token(state);
        let again = PairingView {
            by_id: Some(&registered),
            ..view(&token)
        };
        assert_eq!(
            decide_pairing(&again, &other_hub, at(10)),
            Err(PairRefusal::HubKeyMismatch),
            "{state:?}"
        );
    }
}

/// A revoked token is refused, and a token is expired from the second its
/// life ends.
#[test]
fn a_revoked_or_lapsed_token_is_refused() {
    let revoked = token(TokenUse::Revoked);
    assert_eq!(
        decide_pairing(&view(&revoked), &request(), at(10)),
        Err(PairRefusal::TokenRevoked)
    );
    let unused = token(TokenUse::Unused);
    assert_eq!(
        decide_pairing(&view(&unused), &request(), at(900)),
        Err(PairRefusal::TokenExpired)
    );
    assert!(decide_pairing(&view(&unused), &request(), at(899)).is_ok());
}

/// A spent token pairs no one else; the same agent ID and key on it, still
/// registered with that key, is answered again, even after the token's
/// life, and changes nothing.
#[test]
fn a_spent_token_answers_only_its_own_pairing_again() {
    let spent = token(TokenUse::Used {
        agent_id: id(1),
        agent_key: key(4),
    });
    let registered = record(1, key(4), "laptop");
    let again = PairingView {
        by_id: Some(&registered),
        ..view(&spent)
    };
    assert_eq!(
        decide_pairing(&again, &request(), at(5000)),
        Ok(Pairing::Repeated)
    );
    let other_key = PairRequest {
        agent_key: key(5),
        ..request()
    };
    assert_eq!(
        decide_pairing(&again, &other_key, at(10)),
        Err(PairRefusal::TokenUsed)
    );
    let other_agent = PairRequest {
        agent_id: id(2),
        ..request()
    };
    assert_eq!(
        decide_pairing(&view(&spent), &other_agent, at(10)),
        Err(PairRefusal::TokenUsed)
    );
    assert_eq!(
        decide_pairing(&view(&spent), &request(), at(10)),
        Err(PairRefusal::TokenUsed),
        "an agent no longer registered with the key is no repeat"
    );
}

/// The same identity on a new token keeps its record: its status and
/// hostname, the greater of the two counters, and the time it was seen.
#[test]
fn the_same_identity_paired_again_keeps_its_record() {
    let token = token(TokenUse::Unused);
    let registered = record(1, key(4), "laptop");
    let again = PairingView {
        by_id: Some(&registered),
        ..view(&token)
    };
    let renamed = PairRequest {
        hostname: name("desktop"),
        counter: 12,
        ..request()
    };
    let expected = AgentRecord {
        counter: 12,
        last_seen: at(10),
        ..registered.clone()
    };
    assert_eq!(
        decide_pairing(&again, &renamed, at(10)),
        Ok(Pairing::Again(expected))
    );
    let lower = PairRequest {
        counter: 2,
        ..request()
    };
    let kept = AgentRecord {
        counter: 9,
        last_seen: at(10),
        ..registered.clone()
    };
    assert_eq!(
        decide_pairing(&again, &lower, at(10)),
        Ok(Pairing::Again(kept))
    );
}

/// An agent ID registered with another key is refused, whatever hostname
/// the pairing asks for.
#[test]
fn a_registered_id_with_another_key_is_refused() {
    let token = token(TokenUse::Unused);
    let registered = record(1, key(6), "laptop");
    let taken_over = PairingView {
        by_id: Some(&registered),
        ..view(&token)
    };
    assert_eq!(
        decide_pairing(&taken_over, &request(), at(10)),
        Err(PairRefusal::AgentKeyMismatch)
    );
}

/// A hostname another agent holds is refused, naming the holder as it is
/// registered and when it was last seen.
#[test]
fn a_taken_hostname_names_its_holder() {
    let token = token(TokenUse::Unused);
    let holder = record(2, key(6), "Laptop");
    let taken = PairingView {
        by_hostname: Some(&holder),
        ..view(&token)
    };
    assert_eq!(
        decide_pairing(&taken, &request(), at(10)),
        Err(PairRefusal::HostnameTaken {
            holder: name("Laptop"),
            last_seen: at(-50),
        })
    );
}

/// With 20 agents waiting, a new one is refused; with 19, it is kept.
#[test]
fn a_twenty_first_pending_agent_is_refused() {
    let token = token(TokenUse::Unused);
    let full = PairingView {
        pending: PENDING_LIMIT,
        ..view(&token)
    };
    assert_eq!(
        decide_pairing(&full, &request(), at(10)),
        Err(PairRefusal::PendingLimit)
    );
    let one_left = PairingView {
        pending: PENDING_LIMIT - 1,
        ..view(&token)
    };
    assert!(matches!(
        decide_pairing(&one_left, &request(), at(10)),
        Ok(Pairing::New(_))
    ));
    assert_eq!(PENDING_LIMIT, 20);
}

/// Each refusal's code is the one decision 121's table names, and a valid
/// wire name.
#[test]
fn each_refusal_has_its_code() {
    let holder = PairRefusal::HostnameTaken {
        holder: name("laptop"),
        last_seen: at(0),
    };
    for (refusal, code) in [
        (PairRefusal::ProofRefused, "proof-refused"),
        (PairRefusal::TokenUsed, "token-used"),
        (PairRefusal::TokenExpired, "token-expired"),
        (PairRefusal::TokenRevoked, "token-revoked"),
        (holder, "hostname-taken"),
        (PairRefusal::AgentKeyMismatch, "agent-key-mismatch"),
        (PairRefusal::HubKeyMismatch, "hub-key-mismatch"),
        (PairRefusal::PendingLimit, "pending-limit"),
    ] {
        assert_eq!(refusal.code(), WireName::parse(code).unwrap());
    }
}
