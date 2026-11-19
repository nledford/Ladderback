//! How the `en-US` catalog words a prune's review (technical design §8.4,
//! "Catalog rules"): a count takes the plural form of the noun it counts, and
//! a glossary term is said through its term.

use api::MsgArg;
use l10n::{MessageId, msg};

use crate::support::english;

fn said(id: MessageId, args: &[(&str, MsgArg)]) -> String {
    let shown = english().message(id, args);
    assert!(shown.problems().is_empty(), "{:?}", shown.problems());
    shown.into_text()
}

fn prunes(marked: u64, deleted: u64, recovered: u64) -> String {
    said(
        msg::UI_REVIEW_PRUNES,
        &[
            ("marked", MsgArg::Count(marked)),
            ("deleted", MsgArg::Count(deleted)),
            ("bytes", MsgArg::Bytes(1536)),
            ("recovered", MsgArg::Count(recovered)),
        ],
    )
}

#[test]
fn a_prune_counts_each_kind_of_pack_in_the_plural_form_of_pack() {
    assert_eq!(
        prunes(1, 1, 1),
        "Marked as unused now: 1 pack. \
         Deleted now, marked earlier: 1 pack, freeing 1.5 KiB. \
         Used by a snapshot again, marked for recovery now or brought back now: 1 pack."
    );
    assert_eq!(
        prunes(2, 1_234, 5),
        "Marked as unused now: 2 packs. \
         Deleted now, marked earlier: 1,234 packs, freeing 1.5 KiB. \
         Used by a snapshot again, marked for recovery now or brought back now: 5 packs."
    );
    assert_eq!(
        prunes(1, 2, 0),
        "Marked as unused now: 1 pack. \
         Deleted now, marked earlier: 2 packs, freeing 1.5 KiB. \
         Used by a snapshot again, marked for recovery now or brought back now: 0 packs."
    );
}

#[test]
fn a_prune_says_how_long_a_marked_pack_waits_in_the_plural_form_of_the_unit() {
    let days = |count| {
        said(
            msg::UI_REVIEW_PRUNE_WAIT_DAYS,
            &[("days", MsgArg::Count(count))],
        )
    };
    let hours = |count| {
        said(
            msg::UI_REVIEW_PRUNE_WAIT_HOURS,
            &[("hours", MsgArg::Count(count))],
        )
    };
    assert_eq!(
        days(1),
        "A later prune deletes the packs marked now, after 1 day."
    );
    assert_eq!(
        days(6),
        "A later prune deletes the packs marked now, after 6 days."
    );
    assert_eq!(
        hours(1),
        "A later prune deletes the packs marked now, after 1 hour."
    );
    assert_eq!(
        hours(5),
        "A later prune deletes the packs marked now, after 5 hours."
    );
}
