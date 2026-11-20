use super::*;

fn pattern(text: &str) -> NamePattern {
    NamePattern::parse(text).expect("a pattern")
}

fn id(n: u8) -> SnapshotId {
    SnapshotId::parse(&format!("{n:02x}").repeat(32)).expect("a snapshot ID")
}

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(1_800_000_000 + second).expect("a time in range")
}

fn listed(n: u8, second: i64, plan: Option<&str>) -> Listed {
    Listed {
        id: id(n),
        time: at(second),
        tags: plan
            .map(|plan| vec![format!("{PLAN_TAG_PREFIX}{plan}")])
            .unwrap_or_default(),
    }
}

fn ids(chosen: &Chosen) -> Vec<SnapshotId> {
    chosen
        .searched
        .iter()
        .map(|searched| searched.snapshot.clone())
        .collect()
}

#[test]
fn a_name_matches_itself_whatever_its_case_and_nothing_longer_or_shorter() {
    let tax = pattern("Tax");
    assert!(tax.matches("tax"));
    assert!(tax.matches("TAX"));
    assert!(!tax.matches("taxes"));
    assert!(!tax.matches("ta"));
    assert!(!tax.matches("a tax"));
}

#[test]
fn a_star_stands_for_any_run_of_characters_and_a_question_mark_for_one() {
    assert!(pattern("taxes-*.pdf").matches("taxes-2025.pdf"));
    assert!(pattern("taxes-*.pdf").matches("taxes-.pdf"));
    assert!(!pattern("taxes-*.pdf").matches("taxes-2025.pdf.bak"));
    assert!(pattern("*tax*").matches("my tax returns"));
    assert!(pattern("*").matches("anything"));
    assert!(pattern("20??").matches("2025"));
    assert!(!pattern("20??").matches("202"));
    assert!(!pattern("20??").matches("20255"));
    // A `*` given back characters until the rest matches.
    assert!(pattern("*a*b").matches("aaxab"));
    assert!(!pattern("*a*b").matches("aaxa"));
    assert!(pattern("a**").matches("a"));
}

#[test]
fn a_pattern_matches_a_name_of_any_script() {
    assert!(pattern("ÉTÉ-*").matches("été-photos"));
    assert!(pattern("?.txt").matches("é.txt"));
    assert!(pattern("写真*").matches("写真2025"));
}

#[test]
fn a_pattern_of_many_stars_against_a_long_name_is_answered() {
    let stars = pattern(&"*a".repeat(100));
    let name = "a".repeat(99) + &"b".repeat(4_000);
    assert!(!stars.matches(&name));
}

#[test]
fn a_text_that_names_no_entry_is_no_pattern() {
    assert_eq!(NamePattern::parse(""), Err(InvalidPattern::Empty));
    assert_eq!(
        NamePattern::parse("tax/2025"),
        Err(InvalidPattern::NotAName)
    );
    assert_eq!(NamePattern::parse("tax\0"), Err(InvalidPattern::NotAName));
    let longest = "é".repeat(MAX_PATTERN_CHARS);
    assert_eq!(pattern(&longest).as_str(), longest);
    assert_eq!(
        NamePattern::parse(&format!("{longest}a")),
        Err(InvalidPattern::TooLong)
    );
}

#[test]
fn the_caps_are_their_defaults_unless_told_and_clamped_to_their_bounds() {
    let caps = SearchCaps::clamped(None, None);
    assert_eq!(
        (caps.snapshots(), caps.matches()),
        (DEFAULT_SNAPSHOTS, DEFAULT_MATCHES)
    );
    let caps = SearchCaps::clamped(Some(0), Some(0));
    assert_eq!((caps.snapshots(), caps.matches()), (1, 1));
    let caps = SearchCaps::clamped(Some(u64::MAX), Some(u64::MAX));
    assert_eq!(
        (caps.snapshots(), caps.matches()),
        (MAX_SNAPSHOTS, MAX_MATCHES)
    );
    let caps = SearchCaps::clamped(Some(7), Some(9));
    assert_eq!((caps.snapshots(), caps.matches()), (7, 9));
}

#[test]
fn a_search_reads_the_newest_snapshots_first_up_to_its_cap() {
    let chosen = choose(
        vec![
            listed(1, 10, None),
            listed(2, 30, None),
            listed(3, 20, None),
        ],
        None,
        2,
    );
    assert_eq!(ids(&chosen), vec![id(2), id(3)]);
    assert_eq!(chosen.left_out, 1);
    assert_eq!(
        chosen.searched.first().map(|first| first.time),
        Some(at(30))
    );
}

#[test]
fn a_search_reads_every_snapshot_when_they_are_within_its_cap() {
    let chosen = choose(vec![listed(1, 10, None), listed(2, 30, None)], None, 2);
    assert_eq!(ids(&chosen), vec![id(2), id(1)]);
    assert_eq!(chosen.left_out, 0);
}

#[test]
fn snapshots_of_the_same_time_are_read_in_the_order_of_their_ids() {
    let chosen = choose(vec![listed(2, 10, None), listed(1, 10, None)], None, 5);
    assert_eq!(ids(&chosen), vec![id(1), id(2)]);
}

#[test]
fn a_search_of_one_plan_reads_only_the_snapshots_its_first_plan_tag_names() {
    let home = PlanId::parse("home").expect("a plan ID");
    let mut both = listed(4, 40, Some("work"));
    both.tags.push(format!("{PLAN_TAG_PREFIX}home"));
    let chosen = choose(
        vec![
            listed(1, 10, Some("home")),
            listed(2, 20, Some("work")),
            listed(3, 30, None),
            both,
        ],
        Some(&home),
        5,
    );
    assert_eq!(ids(&chosen), vec![id(1)]);
    assert_eq!(chosen.left_out, 0);
}
