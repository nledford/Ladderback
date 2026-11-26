//! `sqlx::migrate!` embeds the migration files when the crate is compiled, and
//! Cargo can't see them: without this a changed or added migration would not
//! rebuild the crate. The migrations before the baseline are embedded too,
//! for the one-off copy (decision 170).

fn main() {
    println!("cargo:rerun-if-changed=../../../migrations");
    println!("cargo:rerun-if-changed=migrations-before-baseline");
}
