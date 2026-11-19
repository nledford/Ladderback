use crate::{
    ClockOffset, InvalidOffset, InvalidSnapshot, MAX_SNAPSHOT_PATHS, MAX_SNAPSHOT_TAGS, SnapshotDto,
};
use jiff::tz::Offset;

fn dto() -> SnapshotDto {
    SnapshotDto {
        id: "ab".repeat(32),
        time: "2026-11-09T10:15:00Z".to_owned(),
        offset: None,
        hostname: "demo-host".to_owned(),
        label: "home".to_owned(),
        tags: vec!["ladderback-plan:demo-home".to_owned()],
        paths: vec!["/home/demo".to_owned()],
        paths_truncated: false,
        tags_truncated: false,
    }
}

fn offset(seconds: i32) -> Offset {
    Offset::from_seconds(seconds).unwrap()
}

/// The contract of the field (§12.1): a daemon that knows the offset
/// sends it as RFC 3339 writes one, and a listing from one older than the
/// field, which never sends it, reads as having none.
#[test]
fn the_offset_travels_as_rfc_3339_writes_it_and_is_absent_from_an_older_daemon() {
    for (seconds, written) in [
        (-6 * 3600, "-06:00"),
        (0, "+00:00"),
        (13 * 3600, "+13:00"),
        (5 * 3600 + 45 * 60, "+05:45"),
        (-(9 * 3600 + 30 * 60), "-09:30"),
        (19 * 60 + 32, "+00:19:32"),
        (-(25 * 3600 + 59 * 60 + 59), "-25:59:59"),
    ] {
        let taken = dto().taken_at_offset(offset(seconds));
        assert_eq!(taken.offset, Some(ClockOffset::new(offset(seconds))));
        let json = serde_json::to_string(&taken).unwrap();
        assert!(json.contains(&format!(r#""offset":"{written}""#)), "{json}");
        assert_eq!(serde_json::from_str::<SnapshotDto>(&json).unwrap(), taken);
    }
    let older = r#"{"id":"abababababababababababababababababababababababababababababababab","time":"2026-11-09T10:15:00Z","hostname":"h","label":"","tags":[],"paths":[]}"#;
    assert_eq!(
        serde_json::from_str::<SnapshotDto>(older).unwrap().offset,
        None
    );
    let json = serde_json::to_string(&dto()).unwrap();
    assert!(!json.contains("offset"), "{json}");
}

#[test]
fn an_offset_is_read_only_as_rfc_3339_writes_one() {
    assert_eq!("-00:00".parse(), Ok(ClockOffset::new(Offset::UTC)));
    assert_eq!("+25:59:59".parse(), Ok(ClockOffset::new(offset(93_599))));
    for refused in [
        "",
        "Z",
        "UTC",
        "-0600",
        "-06",
        "+6:00",
        "-06:0",
        "06:00",
        " -06:00",
        "-06:00 ",
        "\u{2212}06:00",
        "+26:00",
        "+06:60",
        "+06:00:60",
        "+06:00:0",
        "+06:00:00:00",
        "+0a:00",
        "+0::00",
        "+06-00",
        "-06:00[America/Chicago]",
    ] {
        assert_eq!(
            refused.parse::<ClockOffset>(),
            Err(InvalidOffset),
            "{refused:?}"
        );
    }
    let hostile = r#"{"id":"abababababababababababababababababababababababababababababababab","time":"2026-11-09T10:15:00Z","offset":"America/Chicago","hostname":"h","label":"","tags":[],"paths":[]}"#;
    assert!(serde_json::from_str::<SnapshotDto>(hostile).is_err());
}

#[test]
fn a_snapshot_round_trips_through_json() {
    let dto = dto();
    let built = SnapshotDto::new(
        dto.id.clone(),
        dto.time.clone(),
        dto.hostname.clone(),
        dto.label.clone(),
        dto.tags.clone(),
        dto.paths.clone(),
    )
    .unwrap_or_else(|e| panic!("a valid snapshot: {e:?}"));
    let json = serde_json::to_string(&built).unwrap();
    assert_eq!(serde_json::from_str::<SnapshotDto>(&json).unwrap(), built);
    assert_eq!(built, dto);
}

#[test]
fn an_id_that_is_not_64_hex_digits_is_refused() {
    let dto = dto();
    assert_eq!(
        SnapshotDto::new(
            "nope".to_owned(),
            dto.time,
            dto.hostname,
            dto.label,
            dto.tags,
            dto.paths,
        ),
        Err(InvalidSnapshot::Id)
    );
}

#[test]
fn a_time_that_is_not_rfc_3339_is_refused() {
    let dto = dto();
    assert_eq!(
        SnapshotDto::new(
            dto.id,
            "not a time".to_owned(),
            dto.hostname,
            dto.label,
            dto.tags,
            dto.paths,
        ),
        Err(InvalidSnapshot::Time)
    );
}

#[test]
fn a_snapshot_that_lost_paths_or_tags_says_which() {
    let truncated = dto().truncated(true, false);
    assert!(truncated.paths_truncated && !truncated.tags_truncated);
    let truncated = dto().truncated(false, true);
    assert!(!truncated.paths_truncated && truncated.tags_truncated);
    let json = serde_json::to_string(&dto().truncated(true, true)).unwrap();
    assert!(json.contains(r#""paths_truncated":true"#), "{json}");
    assert!(json.contains(r#""tags_truncated":true"#), "{json}");
}

#[test]
fn a_snapshot_from_a_daemon_that_never_said_is_not_truncated() {
    let json = r#"{"id":"abababababababababababababababababababababababababababababababab","time":"2026-11-09T10:15:00Z","hostname":"h","label":"","tags":[],"paths":[]}"#;
    let dto: SnapshotDto = serde_json::from_str(json).unwrap();
    assert!(!dto.paths_truncated && !dto.tags_truncated);
}

#[test]
fn tags_beyond_the_cap_are_refused_and_the_cap_itself_is_taken() {
    let dto = dto();
    let build = |tags: usize| {
        SnapshotDto::new(
            dto.id.clone(),
            dto.time.clone(),
            dto.hostname.clone(),
            dto.label.clone(),
            vec!["tag".to_owned(); tags],
            dto.paths.clone(),
        )
    };
    assert_eq!(build(MAX_SNAPSHOT_TAGS + 1), Err(InvalidSnapshot::Tags));
    assert!(build(MAX_SNAPSHOT_TAGS).is_ok());
}

#[test]
fn paths_beyond_the_cap_are_refused() {
    let dto = dto();
    assert_eq!(
        SnapshotDto::new(
            dto.id,
            dto.time,
            dto.hostname,
            dto.label,
            dto.tags,
            vec!["/home/demo".to_owned(); MAX_SNAPSHOT_PATHS + 1],
        ),
        Err(InvalidSnapshot::Paths)
    );
}
