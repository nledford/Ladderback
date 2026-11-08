//! `sqlx::migrate!` embeds the migration files when the crate is compiled, and
//! Cargo can't see them: without this a changed or added migration would not
//! rebuild the crate.

fn main() {
    println!("cargo:rerun-if-changed=../../../migrations");
}
