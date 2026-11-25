use super::*;

use Level::{Preview, Propose, Read, Run, StartAndCancel};
use Permission::{
    BackupRuns, Backups, Configuration, DaemonLog, Previews, RepositoryCheck, SnapshotContents,
};

fn rows(permissions: Permissions) -> Vec<(Permission, Level)> {
    permissions.rows().collect()
}

#[test]
fn by_default_a_program_may_see_and_run_backups_preview_and_propose_and_nothing_more() {
    assert_eq!(
        rows(Permissions::default()),
        [
            (Backups, Read),
            (BackupRuns, StartAndCancel),
            (Previews, Preview),
            (Configuration, Propose)
        ]
    );
    for private in [SnapshotContents, DaemonLog, RepositoryCheck] {
        assert!(!Permissions::default().grants(private), "{private:?}");
    }
}

#[test]
fn backup_runs_and_backups_are_independent_rows() {
    let runs = Permissions::NONE
        .with(BackupRuns, Some(StartAndCancel))
        .unwrap();
    assert!(runs.grants(BackupRuns));
    assert!(!runs.grants(Backups), "starting backups reads nothing");
    let read = Permissions::NONE.with(Backups, Some(Read)).unwrap();
    assert!(read.grants(Backups) && !read.grants(BackupRuns));
    let both = runs.with(Backups, Some(Read)).unwrap();
    assert_eq!(both.with(BackupRuns, None).unwrap(), read);
    assert!(runs.with(BackupRuns, None).unwrap().is_empty());
}

#[test]
fn setting_one_row_leaves_the_others_as_they_are() {
    let changed = Permissions::default()
        .with(Previews, None)
        .and_then(|p| p.with(DaemonLog, Some(Read)))
        .unwrap();
    assert_eq!(
        rows(changed),
        [
            (Backups, Read),
            (BackupRuns, StartAndCancel),
            (Configuration, Propose),
            (DaemonLog, Read)
        ]
    );
    assert_eq!(
        rows(Permissions::NONE.with(RepositoryCheck, Some(Run)).unwrap()),
        [(RepositoryCheck, Run)]
    );
}

#[test]
fn a_row_refuses_a_level_it_does_not_have() {
    assert_eq!(
        Permissions::NONE.with(SnapshotContents, Some(StartAndCancel)),
        Err(NoSuchLevel {
            permission: SnapshotContents,
            level: StartAndCancel
        })
    );
    assert_eq!(
        Permissions::default().with(Backups, Some(Propose)),
        Err(NoSuchLevel {
            permission: Backups,
            level: Propose
        })
    );
}

#[test]
fn each_row_and_level_is_read_back_from_its_name() {
    let names: Vec<(&str, &str)> = Permission::ALL
        .iter()
        .map(|row| (row.as_str(), row.level().as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("backups", "read"),
            ("backup-runs", "start-and-cancel"),
            ("previews", "preview"),
            ("configuration", "propose"),
            ("snapshot-contents", "read"),
            ("daemon-log", "read"),
            ("repository-check", "run")
        ]
    );
    for row in Permission::ALL {
        assert_eq!(Permission::parse(row.as_str()), Some(row));
        assert_eq!(Level::parse(row.level().as_str()), Some(row.level()));
    }
    assert_eq!(Permission::parse("Backups"), None);
    assert_eq!(Level::parse(""), None);
}

#[test]
fn each_row_grants_only_itself() {
    for permission in Permission::ALL {
        let just_that = Permissions::NONE
            .with(permission, Some(permission.level()))
            .unwrap();
        let granted: Vec<Permission> = Permission::ALL
            .into_iter()
            .filter(|p| just_that.grants(*p))
            .collect();
        assert_eq!(granted, [permission]);
    }
}

#[test]
fn granting_a_row_already_granted_keeps_it() {
    let read = Permissions::NONE.with(Backups, Some(Read)).unwrap();
    assert_eq!(read.with(Backups, Some(Read)), Ok(read));
}

#[test]
fn a_row_is_set_as_the_configuration_file_names_it() {
    let read = Permissions::NONE.with_named(Backups, "read").unwrap();
    assert_eq!(rows(read), [(Backups, Read)]);
    assert!(read.with_named(Backups, "none").unwrap().is_empty());
    for refused in ["propose", "Read", ""] {
        assert!(read.with_named(Backups, refused).is_err(), "{refused:?}");
    }
}
