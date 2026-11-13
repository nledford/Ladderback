//! The steps of an operation, from rustic's names for its progress bars
//! (technical design §4.1, §4.4; §16, decision 82). rustic names a bar in
//! English, as a library's own text: the anti-corruption layer stops it here.
//! A text this table knows becomes its [`OperationStep`]; any other (a text
//! a newer rustic writes, the empty name of a counter it names later, a text
//! a mask changed) is no step, and the text goes no further.

use app::ports::OperationStep;

/// rustic's text for each step, as the pinned `rustic_core` writes it, one
/// entry per text. The places are in `crates/core/src/` at the revision in
/// `Cargo.lock`; decision 82 lists which of Ladderback's jobs reach each.
pub(crate) const STEP_TEXTS: &[(&str, OperationStep)] = &[
    // index.rs:269, commands/check.rs:459, commands/prune.rs:707
    ("reading index...", OperationStep::ReadingIndex),
    // repository.rs:830, 874, 944
    ("getting snapshots...", OperationStep::GettingSnapshots),
    // repository.rs:801, 1518
    ("getting snapshot...", OperationStep::GettingSnapshots),
    // commands/prune.rs:1590
    ("reading snapshots...", OperationStep::GettingSnapshots),
    // repofile/snapshotfile.rs:806, 808
    (
        "getting latest snapshot...",
        OperationStep::GettingSnapshots,
    ),
    (
        "getting latest~N snapshot...",
        OperationStep::GettingSnapshots,
    ),
    // commands/backup.rs:287
    ("backing up...", OperationStep::BackingUp),
    // commands/check.rs:512, 507; commands/prune.rs:729
    ("listing packs...", OperationStep::ListingPacks),
    ("listing packs in hot repo...", OperationStep::ListingPacks),
    (
        "getting packs from repository...",
        OperationStep::ListingPacks,
    ),
    // commands/check.rs:245 (`{file_type:?}` of Snapshot and Index), 261, 276
    (
        "checking Snapshot in cache...",
        OperationStep::CheckingCache,
    ),
    ("checking Index in cache...", OperationStep::CheckingCache),
    (
        "cleaning up packs from cache...",
        OperationStep::CheckingCache,
    ),
    ("checking packs in cache...", OperationStep::CheckingCache),
    // commands/check.rs:253 (Snapshot and Index)
    (
        "checking Snapshot in hot repo...",
        OperationStep::CheckingHotRepository,
    ),
    (
        "checking Index in hot repo...",
        OperationStep::CheckingHotRepository,
    ),
    // commands/check.rs:628
    ("checking trees...", OperationStep::CheckingTrees),
    // commands/check.rs:301
    ("reading pack data...", OperationStep::ReadingPackData),
    // commands/prune.rs:1608
    ("finding used blobs...", OperationStep::FindingUsedData),
    // commands/prune.rs:1238
    (
        "marking unneeded unindexed pack files for deletion...",
        OperationStep::MarkingUnusedPacks,
    ),
    // commands/prune.rs:1359
    ("repacking...", OperationStep::Repacking),
    // commands/prune.rs:1294
    ("rebuilding index...", OperationStep::RebuildingIndex),
    // commands/prune.rs:1234, 1275 and 1437, 1442, 1447
    (
        "removing unindexed packs...",
        OperationStep::DeletingUnusedData,
    ),
    (
        "removing old index files...",
        OperationStep::DeletingUnusedData,
    ),
    (
        "removing old data packs...",
        OperationStep::DeletingUnusedData,
    ),
    (
        "removing old tree packs...",
        OperationStep::DeletingUnusedData,
    ),
    // repository.rs:994
    ("removing snapshots...", OperationStep::DeletingSnapshots),
    // repository.rs:1012
    ("saving snapshots...", OperationStep::SavingSnapshots),
];

/// The step `text` names, when it is one of rustic's texts in
/// [`STEP_TEXTS`], matched whole.
pub(crate) fn step_of(text: &str) -> Option<OperationStep> {
    STEP_TEXTS
        .iter()
        .find(|(known, _)| *known == text)
        .map(|(_, step)| *step)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every text of the pinned rustic that Ladderback's jobs reach, and the
    /// step each names.
    #[test]
    fn each_text_of_rustics_names_its_step() {
        use OperationStep as S;
        for (text, step) in [
            ("reading index...", S::ReadingIndex),
            ("getting snapshots...", S::GettingSnapshots),
            ("getting snapshot...", S::GettingSnapshots),
            ("reading snapshots...", S::GettingSnapshots),
            ("getting latest snapshot...", S::GettingSnapshots),
            ("getting latest~N snapshot...", S::GettingSnapshots),
            ("backing up...", S::BackingUp),
            ("listing packs...", S::ListingPacks),
            ("listing packs in hot repo...", S::ListingPacks),
            ("getting packs from repository...", S::ListingPacks),
            ("checking Snapshot in cache...", S::CheckingCache),
            ("checking Index in cache...", S::CheckingCache),
            ("cleaning up packs from cache...", S::CheckingCache),
            ("checking packs in cache...", S::CheckingCache),
            ("checking Snapshot in hot repo...", S::CheckingHotRepository),
            ("checking Index in hot repo...", S::CheckingHotRepository),
            ("checking trees...", S::CheckingTrees),
            ("reading pack data...", S::ReadingPackData),
            ("finding used blobs...", S::FindingUsedData),
            (
                "marking unneeded unindexed pack files for deletion...",
                S::MarkingUnusedPacks,
            ),
            ("repacking...", S::Repacking),
            ("rebuilding index...", S::RebuildingIndex),
            ("removing unindexed packs...", S::DeletingUnusedData),
            ("removing old index files...", S::DeletingUnusedData),
            ("removing old data packs...", S::DeletingUnusedData),
            ("removing old tree packs...", S::DeletingUnusedData),
            ("removing snapshots...", S::DeletingSnapshots),
            ("saving snapshots...", S::SavingSnapshots),
        ] {
            assert_eq!(step_of(text), Some(step), "{text:?}");
        }
    }

    /// A text the table doesn't hold whole is no step: rustic's empty name
    /// for a counter it names later, one it writes for a job Ladderback
    /// doesn't run, and one that only looks like a known text.
    #[test]
    fn a_text_rustic_didnt_write_for_a_step_names_none() {
        for text in [
            "",
            "copying data blobs...",
            "checking Config in cache...",
            "reading index",
            "Reading index...",
            " reading index...",
            "reading index... ",
            "reading index...reading index...",
            "backing up...\n",
        ] {
            assert_eq!(step_of(text), None, "{text:?}");
        }
    }
}
