//! Released migrations are append-only. Every installed database records each
//! applied migration's checksum in `_sqlx_migrations`, and the store refuses to
//! open one whose recorded checksum no longer matches the embedded file. So a
//! released migration's bytes must never change: this test pins the checksum
//! sqlx itself computes for each one, from the same migrator the store uses.
//! The baseline schema replaced every migration before it once, before the
//! first release (§16, decision 165); from the first release on, the list is
//! append-only again.

use std::fmt::Write as _;

use sqlx::migrate::{MigrationType, Migrator};

static MIGRATOR: Migrator = sqlx::migrate!("../../../migrations");

/// Each released migration: version, description and the hex SHA-384 checksum
/// sqlx records. Append-only: adding a migration means adding one line here,
/// in the same PR.
const PINNED: &[(i64, &str, &str)] = &[
    (
        1,
        "baseline",
        "b54dd9c2138812d3e5be0d0d4e122e55873392287d384113e3e1613d1d83eb2e10f718f4344cb4d64ea23cdbedb9585c",
    ),
    (
        2,
        "diverged epochs",
        "11ae64b79438522f0d89befccbd5fa12cf451e2d58c58e60103800c6ab104df0be92a9b23e17e9fe0df31823df6ff5fb",
    ),
    (
        3,
        "plans last success",
        "a114c36e9d9ec26a848c79a7f8408b86d38a17fab23251577f011c2eeaf11e7a9db9f742985aa4605b6cb72f8fcf3b5c",
    ),
];

/// The migration's file name, as `sqlx::migrate!` reads it: sqlx turns the
/// name's underscores into the description's spaces.
fn file_name(version: i64, description: &str) -> String {
    format!(
        "migrations/{version:04}_{}.sql",
        description.replace(' ', "_")
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[test]
fn released_migrations_are_append_only() {
    let mut problems = Vec::new();
    let actual: Vec<(i64, String, String)> = MIGRATOR
        .iter()
        .map(|m| (m.version, m.description.to_string(), hex(&m.checksum)))
        .collect();
    // Forward-only, one file per version: otherwise the version lookups below
    // could pass over a file.
    for migration in MIGRATOR.iter() {
        if migration.migration_type != MigrationType::Simple {
            problems.push(format!(
                "version {} is a reversible migration; this project's migrations are \
                 forward-only plain .sql files",
                migration.version
            ));
        }
        if MIGRATOR
            .iter()
            .filter(|m| m.version == migration.version)
            .count()
            > 1
        {
            problems.push(format!(
                "two migrations share version {}; give the newer one the next free number",
                migration.version
            ));
        }
    }

    for &(version, description, checksum) in PINNED {
        match actual.iter().find(|(v, _, _)| *v == version) {
            None => problems.push(format!(
                "{} is missing or renamed; released \
                 migrations are append-only (add a new migration instead)",
                file_name(version, description)
            )),
            Some((_, d, _)) if d != description => problems.push(format!(
                "{} was renamed to {}; \
                 released migrations are append-only (add a new migration instead)",
                file_name(version, description),
                file_name(version, d)
            )),
            Some((_, _, c)) if c != checksum => problems.push(format!(
                "{} changed (checksum {c}, pinned \
                 {checksum}); released migrations are append-only (add a new migration instead)",
                file_name(version, description)
            )),
            Some(_) => {}
        }
    }
    for (version, description, checksum) in &actual {
        if !PINNED.iter().any(|(v, _, _)| v == version) {
            problems.push(format!(
                "{} is not listed in {}; add its line in \
                 the same PR (the list is append-only): ({version}, \"{description}\", \
                 \"{checksum}\"),",
                file_name(*version, description),
                file!()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
