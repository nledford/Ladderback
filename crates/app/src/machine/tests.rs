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
