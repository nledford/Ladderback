use super::*;
use crate::Uuid;

fn agent(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

fn repository(digit: char) -> RepositoryId {
    RepositoryId::parse(&digit.to_string().repeat(64)).unwrap()
}

#[test]
fn a_repository_no_agent_used_at_the_start_is_held_until_every_agent_that_used_any_has_reported() {
    let mut window = StartWindow::new(
        [(agent(1), repository('a')), (agent(2), repository('b'))],
        [],
    );
    assert!(
        window.holds(&repository('c')),
        "agents 1 and 2 may have begun using it after a backup"
    );
    window.heard(agent(1));
    assert!(window.holds(&repository('c')), "agent 2 hasn't reported");
    assert!(!window.holds(&repository('a')));
    window.heard(agent(2));
    assert!(!window.holds(&repository('c')));
    assert!(
        !StartWindow::default().holds(&repository('a')),
        "a hub nobody reported to holds nothing"
    );
}

#[test]
fn a_repository_is_held_until_every_agent_that_used_it_has_reported() {
    let mut window = StartWindow::new(
        [
            (agent(1), repository('a')),
            (agent(2), repository('a')),
            (agent(2), repository('b')),
        ],
        [],
    );
    window.heard(agent(1));
    assert!(window.holds(&repository('a')), "agent 2 hasn't reported");
    window.heard(agent(3));
    assert!(window.holds(&repository('a')), "agent 3 never used it");
    window.heard(agent(2));
    assert!(!window.holds(&repository('a')));
    assert!(!window.holds(&repository('b')));
}

#[test]
fn the_window_holds_nothing_once_its_time_has_passed() {
    let mut window = StartWindow::new([(agent(1), repository('a'))], [agent(2)]);
    window.pass(
        START_WINDOW
            .checked_sub(SignedDuration::from_millis(1))
            .unwrap(),
    );
    assert!(window.holds(&repository('a')), "a moment before the end");
    window.pass(START_WINDOW);
    assert!(!window.holds(&repository('a')), "at the end");
    assert!(
        !window.holds(&repository('b')),
        "nor one no agent used at the start"
    );
}

#[test]
fn time_that_ran_back_doesnt_reopen_the_window() {
    let mut window = StartWindow::new([(agent(1), repository('a'))], [agent(2)]);
    window.pass(START_WINDOW);
    window.pass(SignedDuration::ZERO);
    assert!(!window.holds(&repository('a')));
}

/// The NAS's first report, and the grant it won, were lost with the hub's
/// last commits (a power cut, or a restore from a backup taken before
/// them): no stored report of it names a repository, yet it holds a lease
/// the hub no longer knows of (decisions 164 and 178).
#[test]
fn an_approved_machine_with_no_stored_report_holds_a_repository_others_listed() {
    let (laptop, nas) = (agent(1), agent(2));
    let mut window = StartWindow::new([(laptop, repository('a'))], [laptop, nas]);
    window.heard(laptop);
    assert!(
        window.holds(&repository('a')),
        "the NAS may hold a lease on it that the hub lost"
    );
    assert!(
        window.holds(&repository('b')),
        "and one nobody listed, which the NAS may hold too"
    );
    window.heard(nas);
    assert!(!window.holds(&repository('a')));
    assert!(!window.holds(&repository('b')));
}
