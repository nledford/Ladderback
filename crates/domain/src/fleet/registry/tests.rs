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
        zone: None,
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
        zone: None,
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

/// Names that look alike share a skeleton whatever their case, as a
/// Cyrillic а beside a Latin a does, or "rn" beside "m"; names that don't,
/// in any script, have their own.
#[test]
fn lookalike_hostnames_share_a_skeleton() {
    let alike = |a: &str, b: &str| HostnameSkeleton::of(&name(a)) == HostnameSkeleton::of(&name(b));
    assert!(alike("l\u{430}ptop", "laptop"));
    assert!(alike("l\u{430}ptop", "Laptop"));
    assert!(alike("LAPTOP", "laptop"));
    assert!(alike("rnacbook", "macbook"));
    assert!(alike("caf\u{e9}", "cafe\u{301}"));
    assert!(!alike("laptop", "desktop"));
    assert!(!alike(
        "\u{30ce}\u{30fc}\u{30c8}\u{30d1}\u{30bd}\u{30b3}\u{30f3}",
        "laptop"
    ));
    assert!(!alike("caf\u{e9}", "cafe"));
}

/// A capital that looks like another letter is read as that letter before
/// the case fold, so a capital I is an l and a zero beside capitals is an
/// O; a run of spaces looks like one space.
#[test]
fn capitals_and_runs_of_spaces_look_like_what_they_show() {
    let alike = |a: &str, b: &str| HostnameSkeleton::of(&name(a)) == HostnameSkeleton::of(&name(b));
    assert!(alike("Iaptop", "laptop"));
    assert!(alike("I\u{430}ptop", "Laptop"));
    assert!(alike("LAPTOP0", "LAPTOPO"));
    assert!(alike("\u{412}ob", "Bob"));
    assert!(alike("lap  top", "lap top"));
    assert!(alike("lap \u{3000}\u{a0}top", "lap top"));
    assert!(!alike("lap top", "laptop"));
    assert!(!alike("iaptop", "laptop"));
}

fn pending(agent: u128, hostname: &str, last_seen: i64) -> AgentRecord {
    AgentRecord {
        status: AgentStatus::Pending { since: at(0) },
        last_seen: at(last_seen),
        ..record(agent, key(20), hostname)
    }
}

/// A pending agent whose name looks like another's, pending or approved,
/// is flagged with that agent as it is registered; the approved agent
/// isn't, nor is a pending one whose name looks like no other.
#[test]
fn a_pending_lookalike_is_flagged_with_the_agent_it_looks_like() {
    let approved = record(1, key(11), "Laptop");
    let lookalike = pending(2, "l\u{430}ptop", 5);
    let other_script = pending(
        3,
        "\u{30ce}\u{30fc}\u{30c8}\u{30d1}\u{30bd}\u{30b3}\u{30f3}",
        6,
    );
    let listed = list_agents(vec![
        approved.clone(),
        lookalike.clone(),
        other_script.clone(),
    ]);
    assert_eq!(
        listed,
        vec![
            ListedAgent {
                record: approved,
                looks_like: None,
            },
            ListedAgent {
                record: lookalike,
                looks_like: Some(Lookalike {
                    agent_id: id(1),
                    hostname: name("Laptop"),
                    last_seen: at(-50),
                }),
            },
            ListedAgent {
                record: other_script,
                looks_like: None,
            },
        ]
    );
}

/// Two pending lookalikes are each flagged with the other, whichever paired
/// first; of several it looks like, a pending agent is flagged with the
/// first listed.
#[test]
fn pending_lookalikes_flag_each_other_and_name_the_first_listed() {
    let first = pending(1, "laptop", 1);
    let second = pending(2, "l\u{430}ptop", 2);
    let third = pending(3, "LAPT\u{41e}P", 3);
    let flagged = |listed: &ListedAgent| listed.looks_like.as_ref().map(|like| like.agent_id);
    let listed = list_agents(vec![first, second, third]);
    assert_eq!(
        listed.iter().map(flagged).collect::<Vec<_>>(),
        vec![Some(id(2)), Some(id(1)), Some(id(1))]
    );
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
            zone: None,
        }))
    );
}

/// A new agent is kept with the machine zone it reported, and an identity
/// paired again with the one it reports now, none included: the last
/// report wins (#114).
#[test]
fn a_pairing_keeps_the_machine_zone_the_agent_reported_last() {
    let chicago = MachineZone::parse("America/Chicago").unwrap();
    let berlin = MachineZone::parse("Europe/Berlin").unwrap();
    let token = token(TokenUse::Unused);
    let reported = PairRequest {
        zone: Some(chicago.clone()),
        ..request()
    };
    let Ok(Pairing::New(kept)) = decide_pairing(&view(&token), &reported, at(10)) else {
        panic!("a new agent");
    };
    assert_eq!(kept.zone, Some(chicago.clone()));
    let registered = AgentRecord {
        zone: Some(chicago),
        ..record(1, key(4), "laptop")
    };
    let again = PairingView {
        by_id: Some(&registered),
        ..view(&token)
    };
    for zone in [Some(berlin), None] {
        let moved = PairRequest {
            zone: zone.clone(),
            ..request()
        };
        let Ok(Pairing::Again(kept)) = decide_pairing(&again, &moved, at(10)) else {
            panic!("the same identity again");
        };
        assert_eq!(kept.zone, zone);
    }
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
        Ok(Pairing::Repeated(registered.clone()))
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

/// The same pairing asked again under another hostname is answered with the
/// hostname the hub holds the agent under: a pending agent can't rename
/// itself (decision 120), and the agent is told which one it has.
#[test]
fn a_repeated_pairing_names_the_hostname_the_hub_holds() {
    let spent = token(TokenUse::Used {
        agent_id: id(1),
        agent_key: key(4),
    });
    let registered = record(1, key(4), "laptop");
    let again = PairingView {
        by_id: Some(&registered),
        ..view(&spent)
    };
    let under_another_name = PairRequest {
        hostname: name("desktop"),
        ..request()
    };
    assert_eq!(
        decide_pairing(&again, &under_another_name, at(10)),
        Ok(Pairing::Repeated(registered.clone()))
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

fn proof(counter: u64, kind: BindingKind) -> ProofRequest {
    ProofRequest {
        agent_id: id(1),
        agent_key: key(4),
        kind,
        counter,
    }
}

#[test]
fn a_proof_of_a_registered_agent_keeps_the_greater_counter_and_now() {
    let agent = record(1, key(4), "laptop");
    let kinds = [BindingKind::TlsExporter];
    let view = ProofView {
        record: Some(&agent),
        kinds: &kinds,
    };
    let higher = decide_proof(&view, &proof(12, BindingKind::TlsExporter), at(10));
    assert_eq!(
        higher.map(|proved| proved.agent),
        Ok(AgentRecord {
            counter: 12,
            last_seen: at(10),
            ..agent.clone()
        })
    );
    let lower = decide_proof(&view, &proof(3, BindingKind::TlsExporter), at(20));
    assert_eq!(
        lower.map(|proved| (
            proved.agent.counter,
            proved.agent.last_seen,
            proved.agent.status
        )),
        Ok((9, at(20), agent.status)),
        "a lower counter is never kept, and the status stays"
    );
}

/// A proof says whether it is the agent's first connection since a person
/// approved it, from the record it is decided on: it is when the approval
/// came after the hub last heard from the agent, and isn't once the hub has
/// heard from it since, nor for a pending agent (#1342).
#[test]
fn a_proof_says_whether_it_is_the_first_since_the_approval() {
    let kinds = [BindingKind::TlsExporter];
    let request = proof(12, BindingKind::TlsExporter);
    let first = |agent: &AgentRecord| {
        let view = ProofView {
            record: Some(agent),
            kinds: &kinds,
        };
        decide_proof(&view, &request, at(30)).map(|proved| proved.first_since_approval)
    };
    let approved_at = |at_approval: i64, seen: i64| AgentRecord {
        status: AgentStatus::Approved {
            at: at(at_approval),
        },
        last_seen: at(seen),
        ..record(1, key(4), "laptop")
    };
    assert_eq!(first(&approved_at(20, 10)), Ok(true));
    assert_eq!(first(&approved_at(20, 25)), Ok(false));
    assert_eq!(first(&waiting_with(1, key(4), "laptop")), Ok(false));
}

#[test]
fn a_proof_without_a_record_its_key_or_its_kind_is_refused() {
    let agent = record(1, key(4), "laptop");
    let other_key = record(1, key(5), "laptop");
    let kinds = [BindingKind::None];
    let request = proof(12, BindingKind::None);
    for view in [
        ProofView {
            record: None,
            kinds: &kinds,
        },
        ProofView {
            record: Some(&other_key),
            kinds: &kinds,
        },
        ProofView {
            record: Some(&agent),
            kinds: &[BindingKind::TlsExporter],
        },
        ProofView {
            record: Some(&agent),
            kinds: &[],
        },
    ] {
        assert_eq!(
            decide_proof(&view, &request, at(10)),
            Err(ProofRefusal::ProofRefused),
            "{view:?}"
        );
    }
    assert_eq!(
        ProofRefusal::ProofRefused.code(),
        WireName::parse("proof-refused").unwrap()
    );
}

#[test]
fn a_token_is_kept_while_fewer_than_the_limit_are_unused() {
    let limit = UNUSED_TOKEN_LIMIT;
    assert_eq!(admits_unused_token(0, limit), Ok(()));
    assert_eq!(admits_unused_token(limit - 1, limit), Ok(()));
    assert_eq!(admits_unused_token(limit, limit), Err(UnusedTokensFull));
    assert_eq!(admits_unused_token(limit + 1, limit), Err(UnusedTokensFull));
    assert_eq!(admits_unused_token(0, 0), Err(UnusedTokensFull));
}

#[test]
fn a_new_hub_key_voids_the_unused_tokens_only_while_no_agent_is_registered() {
    assert!(voids_tokens_for_new_key(0));
    assert!(!voids_tokens_for_new_key(1));
    assert!(!voids_tokens_for_new_key(2));
}

fn waiting_with(agent: u128, agent_key: PublicKey, hostname: &str) -> AgentRecord {
    AgentRecord {
        status: AgentStatus::Pending { since: at(-100) },
        ..record(agent, agent_key, hostname)
    }
}

/// A pending agent a person approves is approved from then on, and the
/// rest of its record stays as it was.
#[test]
fn an_approved_pending_agent_is_approved_now_and_keeps_its_record() {
    let waiting = waiting_with(1, key(4), "laptop");
    assert_eq!(
        decide_approval(Some(&waiting), &key(4), at(10)),
        Ok(AgentRecord {
            status: AgentStatus::Approved { at: at(10) },
            ..waiting.clone()
        })
    );
}

/// An agent is approved once: approving it again is refused, and so is an
/// approval of an agent the registry doesn't hold, or holds under another
/// key than the one the person compared.
#[test]
fn an_approval_of_an_approved_unknown_or_rekeyed_agent_is_refused() {
    let approved = record(1, key(4), "laptop");
    assert_eq!(
        decide_approval(Some(&approved), &key(4), at(10)),
        Err(ApprovalRefused::AlreadyApproved)
    );
    assert_eq!(
        decide_approval(None, &key(4), at(10)),
        Err(ApprovalRefused::Unknown)
    );
    let waiting = waiting_with(1, key(4), "laptop");
    assert_eq!(
        decide_approval(Some(&waiting), &key(5), at(10)),
        Err(ApprovalRefused::Unknown)
    );
}

/// An approved agent the hub hasn't heard from since its approval is
/// unseen until its first connection after it, which sets `last_seen`
/// after the approval; a pending agent is never.
#[test]
fn an_approved_agent_is_unseen_since_approval_until_the_hub_hears_from_it() {
    let approved_at = |at_approval: i64, seen: i64| AgentRecord {
        status: AgentStatus::Approved {
            at: at(at_approval),
        },
        last_seen: at(seen),
        ..record(1, key(4), "laptop")
    };
    assert!(approved_at(10, 5).unseen_since_approval());
    assert!(
        approved_at(10, 10).unseen_since_approval(),
        "seen at the approval's instant is seen before it"
    );
    assert!(!approved_at(10, 11).unseen_since_approval());
    assert!(!waiting_with(1, key(4), "laptop").unseen_since_approval());
}
