//! A repository's stats (§3.1, §16 decision 104): four figures read from a
//! repository, and a reading of them, the figures with the time they were
//! taken.

use serde::{Deserialize, Serialize};

use crate::{RepositoryAlias, RepositoryId, SignedDuration, Timestamp};

/// How long a reading is kept: the year the operations are kept (§9). A
/// reading taken this long before a newer one of its repository is dropped
/// when the newer one is saved.
pub const READINGS_KEPT_FOR: SignedDuration = SignedDuration::from_hours(365 * 24);

/// The figures of one read of a repository.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryStats {
    /// How many snapshots the repository holds.
    pub snapshots: u64,
    /// The size of the sources the snapshots hold, added up: a file two
    /// snapshots hold counts twice.
    pub total_size: u64,
    /// What the repository's packs hold for them: every indexed blob's
    /// length in its pack, after deduplication, compression and encryption.
    pub stored_size: u64,
    /// The raw size of those blobs, before compression.
    pub raw_size: u64,
}

impl RepositoryStats {
    /// The share of the raw size compression saved, a whole percent rounded
    /// down: 0 when the stored size is the raw size or more (encryption adds
    /// a little to data that doesn't compress), and none when the repository
    /// stores no data.
    pub fn compression_saved_percent(&self) -> Option<u8> {
        if self.raw_size == 0 {
            return None;
        }
        let saved = u128::from(self.raw_size.saturating_sub(self.stored_size));
        // At most 100, for the saved bytes are at most the raw size.
        u8::try_from(saved * 100 / u128::from(self.raw_size)).ok()
    }
}

/// One reading of a repository's stats: the figures, when they were taken,
/// and the repository they were taken of, by its alias and the ID its entry
/// recorded then, if it recorded one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatsReading {
    pub alias: RepositoryAlias,
    pub repository: Option<RepositoryId>,
    pub taken_at: Timestamp,
    pub stats: RepositoryStats,
}

impl StatsReading {
    /// Whether a reading taken at `taken_at` is still kept once this one is.
    pub fn keeps(&self, taken_at: Timestamp) -> bool {
        // A reading taken so early that a year before it can't be written
        // keeps everything before it.
        self.taken_at
            .checked_sub(READINGS_KEPT_FOR)
            .ok()
            .is_none_or(|oldest| taken_at >= oldest)
    }

    /// Whether this reading was taken of the repository whose ID is `id`,
    /// as far as can be told: a reading of another repository saved under
    /// the same alias, once that alias names this one, is not. A reading or
    /// an entry that recorded no ID can't be told apart, so it is.
    pub fn is_of(&self, id: Option<&RepositoryId>) -> bool {
        match (&self.repository, id) {
            (Some(taken_of), Some(id)) => taken_of == id,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(stored_size: u64, raw_size: u64) -> RepositoryStats {
        RepositoryStats {
            snapshots: 1,
            total_size: raw_size,
            stored_size,
            raw_size,
        }
    }

    #[test]
    fn compression_is_the_share_of_the_raw_size_saved() {
        assert_eq!(stats(1200, 2400).compression_saved_percent(), Some(50));
        assert_eq!(stats(1, 4).compression_saved_percent(), Some(75));
    }

    #[test]
    fn compression_rounds_down_to_a_whole_percent() {
        // 1 - 333/1000 is 66.7 percent.
        assert_eq!(stats(333, 1000).compression_saved_percent(), Some(66));
        // 1 - 1/1000 is 99.9 percent: not yet all of it.
        assert_eq!(stats(1, 1000).compression_saved_percent(), Some(99));
        assert_eq!(stats(0, 1000).compression_saved_percent(), Some(100));
    }

    #[test]
    fn data_that_did_not_compress_saved_nothing() {
        assert_eq!(stats(1000, 1000).compression_saved_percent(), Some(0));
        // Encryption adds a little to data that doesn't compress.
        assert_eq!(stats(1032, 1000).compression_saved_percent(), Some(0));
    }

    #[test]
    fn a_repository_that_stores_no_data_has_no_compression() {
        assert_eq!(stats(0, 0).compression_saved_percent(), None);
    }

    #[test]
    fn the_largest_sizes_do_not_overflow() {
        assert_eq!(
            stats(u64::MAX / 2, u64::MAX).compression_saved_percent(),
            Some(50)
        );
        assert_eq!(stats(1, u64::MAX).compression_saved_percent(), Some(99));
    }

    fn reading_at(seconds: i64) -> StatsReading {
        StatsReading {
            alias: RepositoryAlias::parse("demo").expect("an alias"),
            repository: None,
            taken_at: Timestamp::from_second(seconds).expect("an instant"),
            stats: RepositoryStats::default(),
        }
    }

    #[test]
    fn a_reading_keeps_those_taken_up_to_a_year_before_it() {
        let day = 24 * 60 * 60;
        let newest = reading_at(1_800_000_000);
        let at = |days_before: i64, seconds: i64| {
            Timestamp::from_second(1_800_000_000 - days_before * day + seconds).expect("an instant")
        };
        assert!(newest.keeps(at(364, 0)));
        assert!(newest.keeps(at(365, 0)), "exactly a year before is kept");
        assert!(!newest.keeps(at(365, -1)), "a second more is not");
        assert!(!newest.keeps(at(366, 0)));
        assert!(newest.keeps(at(0, 60)), "a later reading is kept");
    }

    #[test]
    fn a_reading_is_of_the_repository_whose_id_it_recorded() {
        let id = |hex: &str| RepositoryId::parse(&hex.repeat(32)).expect("a repository ID");
        let taken_of = |repository| StatsReading {
            repository,
            ..reading_at(1_800_000_000)
        };
        assert!(taken_of(Some(id("5a"))).is_of(Some(&id("5a"))));
        assert!(
            !taken_of(Some(id("5a"))).is_of(Some(&id("6b"))),
            "another repository's reading under the same alias"
        );
        assert!(
            taken_of(Some(id("5a"))).is_of(None),
            "an entry with no ID can't tell"
        );
        assert!(
            taken_of(None).is_of(Some(&id("5a"))),
            "a reading with no ID can't tell"
        );
    }
}
