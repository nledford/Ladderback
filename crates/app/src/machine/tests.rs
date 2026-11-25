//! Where a machine stands as its hub refuses it, or takes it, as a second
//! computer under its machine ID (decision 150; #1462).

use super::*;

/// Where a machine that stood at `standing` stands after `change`.
fn after(standing: Standing, change: fn(&MachineStanding)) -> Standing {
    let held = MachineStanding::new(standing);
    change(&held);
    held.get()
}

#[test]
fn only_a_machine_standing_as_usual_takes_its_hubs_refusal() {
    let refused = MachineStanding::hub_refused;
    assert_eq!(after(Standing::Same, refused), Standing::RefusedByHub);
    assert_eq!(
        after(Standing::RefusedByHub, refused),
        Standing::RefusedByHub
    );
    // A copy stays one, and says so; a machine that took a new ID uses it
    // from its next start; one whose identifier couldn't be read sends no
    // fingerprint for its hub to refuse.
    assert_eq!(after(Standing::Copied, refused), Standing::Copied);
    assert_eq!(after(Standing::NewIdTaken, refused), Standing::NewIdTaken);
    assert_eq!(after(Standing::Unread, refused), Standing::Unread);
}

#[test]
fn its_hub_taking_the_machine_again_clears_only_the_refusal() {
    let took = MachineStanding::hub_took;
    assert_eq!(after(Standing::RefusedByHub, took), Standing::Same);
    assert_eq!(after(Standing::Same, took), Standing::Same);
    // A copied data folder stays stopped whatever its hub says (I1), and a
    // new ID taken stays taken.
    assert_eq!(after(Standing::Copied, took), Standing::Copied);
    assert_eq!(after(Standing::NewIdTaken, took), Standing::NewIdTaken);
    assert_eq!(after(Standing::Unread, took), Standing::Unread);
}

#[test]
fn a_machine_its_hub_refused_maintains_as_before() {
    // It may be the original computer: its hub's lease, which a hub that
    // refuses it never grants, is what stops its maintenance.
    assert!(Standing::RefusedByHub.maintains());
    assert!(!Standing::Copied.maintains());
    assert!(!Standing::NewIdTaken.maintains());
}

/// A machine its hub removed, or rejected while it waited (#1489): the hub
/// never takes its ID again, so it may take a new one, whether it stood as
/// usual or refused as a second computer before.
#[test]
fn a_machine_its_hub_removed_stands_removed() {
    let removed = MachineStanding::hub_removed;
    assert_eq!(after(Standing::Same, removed), Standing::RemovedByHub);
    assert_eq!(
        after(Standing::RefusedByHub, removed),
        Standing::RemovedByHub
    );
    assert_eq!(
        after(Standing::RemovedByHub, removed),
        Standing::RemovedByHub
    );
    // A copy stays one, a new ID taken stays taken, and a machine whose
    // identifier couldn't be read can't take a new ID without one.
    assert_eq!(after(Standing::Copied, removed), Standing::Copied);
    assert_eq!(after(Standing::NewIdTaken, removed), Standing::NewIdTaken);
    assert_eq!(after(Standing::Unread, removed), Standing::Unread);
}

/// A hub that takes the machine, or refuses it only as a second computer,
/// got past its proof: it no longer holds the machine removed.
#[test]
fn its_hub_taking_or_refusing_the_machine_clears_the_removal() {
    assert_eq!(
        after(Standing::RemovedByHub, MachineStanding::hub_took),
        Standing::Same
    );
    assert_eq!(
        after(Standing::RemovedByHub, MachineStanding::hub_refused),
        Standing::RefusedByHub
    );
}

#[test]
fn a_machine_its_hub_removed_may_take_a_new_id_and_maintains_as_before() {
    // Its hub grants it no lease, which is what stops its maintenance, as
    // for a machine its hub refused.
    assert!(Standing::RemovedByHub.offers_new_id());
    assert!(Standing::RemovedByHub.maintains());
    assert!(Standing::RefusedByHub.offers_new_id());
    assert!(!Standing::Same.offers_new_id());
    assert!(!Standing::NewIdTaken.offers_new_id());
    assert!(!Standing::Unread.offers_new_id());
}

/// Taking a new ID answers where the machine it was given stands now, as
/// its hub moves it.
#[test]
fn taking_a_new_id_reads_where_the_machine_stands_now() {
    use crate::fakes::{FakeThisMachine, InMemoryConfigStore, InMemoryOperationStore};

    let standing = Arc::new(MachineStanding::new(Standing::Same));
    let take = TakeNewId::new(
        Arc::new(FakeThisMachine::new(Some("this-computer"))),
        Arc::clone(&standing),
        Arc::new(InMemoryConfigStore::default()),
        Arc::new(InMemoryOperationStore::default()),
        Arc::default(),
    );
    standing.hub_refused();
    assert_eq!(take.standing(), Standing::RefusedByHub);
}
