//! Released migrations are append-only. Every installed database records each
//! applied migration's checksum in `_sqlx_migrations`, and the store refuses to
//! open one whose recorded checksum no longer matches the embedded file. So a
//! released migration's bytes must never change: this test pins the checksum
//! sqlx itself computes for each one, from the same migrator the store uses.

use std::fmt::Write as _;

use sqlx::migrate::{MigrationType, Migrator};

static MIGRATOR: Migrator = sqlx::migrate!("../../../migrations");

/// Each released migration: version, description and the hex SHA-384 checksum
/// sqlx records. Append-only: adding a migration means adding one line here,
/// in the same PR.
const PINNED: &[(i64, &str, &str)] = &[
    (
        1,
        "operations",
        "27ad56a7416ee4e55fdbc6c7671a82d8783984ee9fa2a95253ef23bd3db95f0388e1a69b1048fd42adb5467c8d808a5f",
    ),
    (
        2,
        "one maintenance per repo",
        "495792bc209277ec7b71b6952b3b2296ef7bc07f5ddda4516ec9a87be30a19f01bd7716dfd971a956e5934f877bff5ab",
    ),
    (
        3,
        "adopt is maintenance",
        "8d1f91dd6af09506838384d197258b81d28cb5404aac888ac3a55934300389a7bf358b8f4f3e12a8dca58404ba59314d",
    ),
    (
        4,
        "operation repository alias",
        "cebb9f0f4846fd5f8b3ee81c1fb8027d2c6699db299c8aa04d142eee3b35ecca7d41eda62b28ca4948ab2d03e7ea6960",
    ),
    (
        5,
        "succeeded by plan",
        "10159eec955d734ba13c1c07860c97510541111484e5ee33e12939ddeaa9fdfadfb29b9810072941e8d931c4be23bf78",
    ),
    (
        6,
        "schedule state",
        "4a84f175b1178248cfe28b1117d4757156533a3a7dfce2b11e8e9a8e96545d2b7f7142088842cef74ada58758b6ca1ae",
    ),
    (
        7,
        "retry state",
        "1729efcdc036ba1818418d1a64ef9444c709160cbd3e0471cdefeb2ee6e56f47971704d94f828bae66d51639e68c75e8",
    ),
    (
        8,
        "operation locations",
        "7154dd8356081399fbbcad0bb11fef4add0563974f83b02131b6a85c9f5c5755662aaa6e394d4127f8249d48ec02c4c6",
    ),
    (
        9,
        "api tokens",
        "ddc0bc83e36ff7d2bf9f2c00032a2b927779748afc5fa073628810cf9d1437f7797bfcb589ec5b10c450724ff9767ac8",
    ),
    (
        10,
        "sessions",
        "961881a91d88912756b22c2ad65aabb9e5aa88b8a34b443236d717bdb71ca096f35ed18ac68529c733b4c895cfcd21bd",
    ),
    (
        11,
        "repo stats",
        "cd8d0a85daae74ff46af6c76947d7c7f752c0f3088e8147e42d3f9e8ba57017308d0f1db91e1319845d6b303952a0ac4",
    ),
    (
        12,
        "release check",
        "6f46345273e91c654e3cde06e8aa1ac34119bc65836707ff1402c8497ebc7d4fb809d03ee6a7ae23f413f032b206c238",
    ),
    (
        13,
        "proposals",
        "7fa15de54028b669ff140db4200844314faf4c280bdaf37abbb1fb1fe2f0c02cd084a32d216739265d81e9df77670299",
    ),
    (
        14,
        "proposal decisions",
        "67517d0b1f90020e6290e1a810f07e66cd7447205e02863022393026f9068ce47d5698428f8f85219c17bdae670fcf75",
    ),
    (
        15,
        "operation entry",
        "342a20b97c987a48dfe03883e9356e8be99b82b3ce089dea11e75538bdd08924dfb59d357be8b70a8abfb46696f10c20",
    ),
    (
        16,
        "hub registry",
        "d358afcedf70fbedbb51e9016085519b92106907ae3bd4f5361bd1d57101037c7495f6097c867edb6edcd97674ef0cc7",
    ),
    (
        17,
        "api token permissions",
        "3a9e32929bac3a5fe6397385a801d5f1d5381e2d86c2549b473284d4379e2a7ba6ada382eb705ac6d79ac9c4f00b9797",
    ),
    (
        18,
        "hub tls key",
        "e057a5dc118f0f41846e5d508466456b29797f226263b0ded669e5e084bc47436aed08e4809af14735f281b2c59f0db0",
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
