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
    let mut window = StartWindow::new([(agent(1), repository('a')), (agent(2), repository('b'))]);
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
    let mut window = StartWindow::new([
        (agent(1), repository('a')),
        (agent(2), repository('a')),
        (agent(2), repository('b')),
    ]);
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
    let mut window = StartWindow::new([(agent(1), repository('a'))]);
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
    let mut window = StartWindow::new([(agent(1), repository('a'))]);
    window.pass(START_WINDOW);
    window.pass(SignedDuration::ZERO);
    assert!(!window.holds(&repository('a')));
}
