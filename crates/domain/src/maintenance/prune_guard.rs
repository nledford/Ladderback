//! The owner prune guard (technical design §7.3, layer 2 of I3; §16,
//! decision 166): a repository's prune doesn't start while another machine
//! is backing up to it. The hub decides which prunes it holds from what
//! the heartbeats of the machines it holds a connection of say they run
//! ([`held_prunes`]), and tells each machine in its answer to that
//! machine's heartbeat; the machine keeps the latest answer
//! ([`PruneHolds`]) and starts no prune the answer holds, or when the
//! answer is older than [`HOLDS_STAND_FOR`]. What can't be told holds:
//! a machine whose heartbeat doesn't say what it runs, a backup that
//! doesn't know its repository's ID yet, and an answer that doesn't say.
//! Rules only: every time is passed in.

use std::collections::BTreeSet;

use crate::fleet::SILENCE_LIMIT;
use crate::{OperationId, RepositoryId, SignedDuration, Timestamp};

/// The most backups a heartbeat names, and the most repositories the hub's
/// answer holds: as many as fit one sync frame. A machine that runs more
/// says it can't tell, and the hub that would hold more says the same,
/// which holds every prune.
pub const MAX_RUNNING_BACKUPS: usize = 16;

/// How long the hub's answer stands on the machine that heard it: the 90
/// seconds after which the hub counts a silent machine offline (decision
/// 159), on the machine's wall clock and on its monotonic clock, so neither
/// time asleep nor a clock set back makes an old answer look new.
pub const HOLDS_STAND_FOR: SignedDuration = SILENCE_LIMIT;

/// A backup a machine runs, as its heartbeat says it: the operation, and
/// the repository it backs up to, `None` while the machine doesn't know
/// the repository's ID (a first backup to a repository whose entry records
/// none, §16, decision 57).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningBackup {
    pub op_id: OperationId,
    pub repository: Option<RepositoryId>,
}

/// The repositories whose prune the hub holds for a machine, from what the
/// latest heartbeat of each other machine it holds a connection of said it
/// runs: `None` for a heartbeat that didn't say. `None` when it can't tell
/// which: a machine whose heartbeat didn't say, a backup with no
/// repository, or more repositories than [`MAX_RUNNING_BACKUPS`]. A
/// machine offline is no longer among `others`, so it holds nothing.
pub fn held_prunes<'a>(
    others: impl IntoIterator<Item = Option<&'a [RunningBackup]>>,
) -> Option<Vec<RepositoryId>> {
    let mut held = BTreeSet::new();
    for running in others {
        for backup in running? {
            held.insert(backup.repository.clone()?);
            if held.len() > MAX_RUNNING_BACKUPS {
                return None;
            }
        }
    }
    Some(held.into_iter().collect())
}

/// The latest answer a paired machine heard from its hub of the prunes it
/// holds, and when, on the machine's wall clock and its monotonic clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PruneHolds {
    held: Option<Vec<RepositoryId>>,
    wall: Timestamp,
    monotonic: SignedDuration,
}

impl PruneHolds {
    /// The hub's answer `held`, heard at `wall` and `monotonic`: `None`
    /// when the answer didn't say, which holds every prune.
    pub fn heard(
        held: Option<Vec<RepositoryId>>,
        wall: Timestamp,
        monotonic: SignedDuration,
    ) -> Self {
        Self {
            held,
            wall,
            monotonic,
        }
    }

    /// Whether the prune of `repository` is held at `wall` and
    /// `monotonic`: the answer holds it or didn't say, or it is
    /// [`HOLDS_STAND_FOR`] old or more on either clock, or the wall clock
    /// went back past it.
    pub fn holds(
        &self,
        repository: &RepositoryId,
        wall: Timestamp,
        monotonic: SignedDuration,
    ) -> bool {
        let wall_age = wall.duration_since(self.wall);
        let Some(monotonic_age) = monotonic.checked_sub(self.monotonic) else {
            return true;
        };
        let stale = wall_age.is_negative()
            || monotonic_age.is_negative()
            || wall_age >= HOLDS_STAND_FOR
            || monotonic_age >= HOLDS_STAND_FOR;
        stale
            || self
                .held
                .as_ref()
                .is_none_or(|held| held.contains(repository))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Uuid;

    fn repository(byte: u8) -> RepositoryId {
        RepositoryId::parse(&format!("{byte:02x}").repeat(32)).unwrap()
    }

    fn backup(n: u128, repository: Option<RepositoryId>) -> RunningBackup {
        RunningBackup {
            op_id: OperationId::from_uuid(Uuid::from_u128(n)),
            repository,
        }
    }

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + seconds).unwrap()
    }

    fn mono(seconds: i64) -> SignedDuration {
        SignedDuration::from_secs(seconds)
    }

    #[test]
    fn a_backup_on_another_machine_holds_its_repository_once() {
        let one = [
            backup(1, Some(repository(2))),
            backup(2, Some(repository(1))),
        ];
        let two = [backup(3, Some(repository(2)))];
        assert_eq!(
            held_prunes([Some(&one[..]), Some(&two[..])]),
            Some(vec![repository(1), repository(2)])
        );
    }

    #[test]
    fn machines_that_run_nothing_hold_nothing() {
        assert_eq!(
            held_prunes([Some(&[][..]), Some(&[][..])]),
            Some(Vec::new())
        );
        assert_eq!(held_prunes([]), Some(Vec::new()));
    }

    #[test]
    fn a_machine_that_doesnt_say_what_it_runs_holds_every_prune() {
        let one = [backup(1, Some(repository(1)))];
        assert_eq!(held_prunes([Some(&one[..]), None]), None);
    }

    #[test]
    fn a_backup_that_doesnt_know_its_repository_holds_every_prune() {
        let one = [backup(1, Some(repository(1))), backup(2, None)];
        assert_eq!(held_prunes([Some(&one[..])]), None);
    }

    #[test]
    fn more_repositories_than_an_answer_holds_hold_every_prune() {
        let many: Vec<_> = (0..=MAX_RUNNING_BACKUPS)
            .map(|n| backup(n as u128, Some(repository(n as u8))))
            .collect();
        assert_eq!(held_prunes([Some(&many[..])]), None);
        assert_eq!(
            held_prunes([Some(&many[..MAX_RUNNING_BACKUPS])]).map(|held| held.len()),
            Some(MAX_RUNNING_BACKUPS)
        );
    }

    #[test]
    fn an_answer_holds_the_repositories_it_names_while_it_stands() {
        let holds = PruneHolds::heard(Some(vec![repository(1)]), at(0), mono(100));
        assert!(holds.holds(&repository(1), at(0), mono(100)));
        assert!(!holds.holds(&repository(2), at(0), mono(100)));
        assert!(!holds.holds(&repository(2), at(89), mono(189)));
    }

    #[test]
    fn an_answer_that_doesnt_say_holds_every_prune() {
        let holds = PruneHolds::heard(None, at(0), mono(100));
        assert!(holds.holds(&repository(2), at(0), mono(100)));
    }

    #[test]
    fn an_answer_90_seconds_old_on_either_clock_holds_every_prune() {
        let holds = PruneHolds::heard(Some(Vec::new()), at(0), mono(100));
        assert!(holds.holds(&repository(2), at(90), mono(110)), "asleep");
        assert!(
            holds.holds(&repository(2), at(10), mono(190)),
            "wall clock set back meanwhile"
        );
    }

    #[test]
    fn a_wall_clock_set_back_past_the_answer_holds_every_prune() {
        let holds = PruneHolds::heard(Some(Vec::new()), at(0), mono(100));
        assert!(holds.holds(&repository(2), at(-1), mono(101)));
    }
}
