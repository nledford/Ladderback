use jiff::SignedDuration;

use super::*;

fn range(lowest: u64, highest: u64) -> Protocols {
    Protocols::new(lowest, highest).unwrap()
}

fn hello(protocols: Protocols) -> Hello {
    Hello {
        agent_version: "0.5.0+abc1234".to_owned(),
        hostname: Hostname::parse("laptop").unwrap(),
        protocols,
        clock: EpochSeconds::from_seconds(1_800_000_000),
        zone: None,
    }
}

fn welcome() -> Welcome {
    Welcome::answering(
        &hello(PROTOCOLS),
        Approval::Pending,
        "0.5.0+def5678".to_owned(),
        EpochSeconds::from_seconds(1_800_000_000),
    )
}

#[test]
fn a_range_holds_at_least_protocol_one() {
    assert_eq!(Protocols::new(0, 3), None);
    assert_eq!(Protocols::new(3, 2), None);
    let one = range(2, 2);
    assert_eq!((one.lowest(), one.highest()), (2, 2));
}

#[test]
fn the_highest_shared_protocol_is_picked() {
    assert_eq!(range(1, 3).highest_shared(range(2, 5)), Some(3));
    assert_eq!(range(2, 5).highest_shared(range(1, 3)), Some(3));
    assert_eq!(range(1, 1).highest_shared(range(1, 1)), Some(1));
    assert_eq!(range(1, 2).highest_shared(range(3, 4)), None);
    assert_eq!(range(3, 4).highest_shared(range(1, 2)), None);
    assert_eq!(range(1, 2).highest_shared(range(2, 2)), Some(2));
}

#[test]
fn a_welcome_answers_the_hello_it_follows() {
    let asked = range(1, 4);
    let answer = Welcome::answering(
        &hello(asked),
        Approval::Approved,
        "0.5.0+def5678".to_owned(),
        EpochSeconds::from_seconds(1_800_000_000),
    );
    assert_eq!(
        answer,
        Welcome {
            protocol: Some(1),
            hub_protocols: PROTOCOLS,
            agent_protocols: asked,
            generation: GENERATION,
            hub_handshakes: HANDSHAKES,
            hub_version: "0.5.0+def5678".to_owned(),
            signed_at: EpochSeconds::from_seconds(1_800_000_000),
            approval: Approval::Approved,
        }
    );
    assert_eq!(
        answer.expires_at(),
        EpochSeconds::from_seconds(1_800_000_600)
    );
    let none = Welcome::answering(
        &hello(range(2, 3)),
        Approval::Pending,
        String::new(),
        EpochSeconds::from_seconds(0),
    );
    assert_eq!(none.protocol, None);
}

#[test]
fn an_agent_acts_on_a_welcome_inside_its_window() {
    let welcome = welcome();
    for ahead in [1, MESSAGE_LIFE, MAX_EXPIRY_AHEAD] {
        let expires = EpochSeconds::from_seconds(1_800_000_000 + ahead);
        assert_eq!(
            welcome.check(expires, SignedDuration::ZERO, PROTOCOLS, 1),
            Ok(()),
            "{ahead}"
        );
    }
    for expires in [1_800_000_000, 1_799_999_999, 1_800_000_901] {
        assert_eq!(
            welcome.check(
                EpochSeconds::from_seconds(expires),
                SignedDuration::ZERO,
                PROTOCOLS,
                1
            ),
            Err(WelcomeRefused::Expiry),
            "{expires}"
        );
    }
}

/// The agent can't read the hub's clock, but it knows how long it waited
/// since it sent `Hello`, which the hub signed after: a `Welcome` that took
/// longer than its own life to arrive had expired by then (decision 120,
/// "Expiry is judged on the hub's time").
#[test]
fn a_welcome_that_took_longer_than_its_life_to_arrive_is_refused() {
    let welcome = welcome();
    let expires = welcome.expires_at();
    let life = i64::try_from(MESSAGE_LIFE).unwrap();
    for waited in [0, 1, life - 1] {
        assert_eq!(
            welcome.check(expires, SignedDuration::from_secs(waited), PROTOCOLS, 1),
            Ok(()),
            "{waited}"
        );
    }
    for waited in [life, life + 1, i64::MAX] {
        assert_eq!(
            welcome.check(expires, SignedDuration::from_secs(waited), PROTOCOLS, 1),
            Err(WelcomeRefused::Expiry),
            "{waited}"
        );
    }
    assert_eq!(
        welcome.check(
            expires,
            SignedDuration::from_millis(life * 1_000 - 1),
            PROTOCOLS,
            1
        ),
        Ok(())
    );
    assert_eq!(
        welcome.check(expires, SignedDuration::from_secs(-5), PROTOCOLS, 1),
        Ok(())
    );
}

#[test]
fn an_altered_protocol_echo_or_pick_is_refused() {
    let expires = welcome().expires_at();
    let mut stripped = welcome();
    stripped.agent_protocols = range(1, 2);
    assert_eq!(
        stripped.check(expires, SignedDuration::ZERO, PROTOCOLS, 1),
        Err(WelcomeRefused::Protocols)
    );
    let mut lowered = welcome();
    lowered.hub_protocols = range(1, 2);
    lowered.agent_protocols = range(1, 2);
    lowered.protocol = Some(1);
    assert_eq!(
        lowered.check(expires, SignedDuration::ZERO, range(1, 2), 1),
        Err(WelcomeRefused::Protocols)
    );
    lowered.protocol = None;
    assert_eq!(
        lowered.check(expires, SignedDuration::ZERO, range(1, 2), 1),
        Err(WelcomeRefused::Protocols)
    );
}

#[test]
fn a_lower_generation_or_no_common_protocol_is_refused() {
    let welcome = welcome();
    let expires = welcome.expires_at();
    assert_eq!(
        welcome.check(expires, SignedDuration::ZERO, PROTOCOLS, 2),
        Err(WelcomeRefused::GenerationLower)
    );
    let mut higher = welcome.clone();
    higher.generation = 3;
    assert_eq!(
        higher.check(expires, SignedDuration::ZERO, PROTOCOLS, 2),
        Ok(())
    );
    let mut apart = welcome;
    apart.hub_protocols = range(2, 3);
    apart.protocol = None;
    assert_eq!(
        apart.check(expires, SignedDuration::ZERO, PROTOCOLS, 1),
        Err(WelcomeRefused::NoCommonProtocol)
    );
}

#[test]
fn the_message_kinds_are_named_as_the_protocol_names_them() {
    assert_eq!(hello_kind().as_str(), "Hello");
    assert_eq!(welcome_kind().as_str(), "Welcome");
    assert_eq!(
        Approval::of(AgentStatus::Pending {
            since: jiff::Timestamp::UNIX_EPOCH
        }),
        Approval::Pending
    );
    assert_eq!(
        Approval::of(AgentStatus::Approved {
            at: jiff::Timestamp::UNIX_EPOCH
        }),
        Approval::Approved
    );
}
