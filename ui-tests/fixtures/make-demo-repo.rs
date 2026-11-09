//! Initializes the `demo-repo` fixture beside this file: a real rustic
//! repository, with no snapshots, whose password is `demo-password`. The
//! `@ui` journeys copy it into each scenario's scratch folder, because the
//! daemon backs up into it for real.
//!
//! Run it by pasting this file into a crate that depends on the same
//! revision of `rustic_core` and `rustic_backend` as the workspace's
//! `Cargo.toml`, and pointing the binary at a fresh `demo-repo` folder:
//!
//! ```text
//! cargo run --bin <that crate> -- <this dir>/demo-repo
//! ```

use rustic_backend::BackendOptions;
use rustic_core::{ConfigOptions, Credentials, KeyOptions, Repository, RepositoryOptions};

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "demo-repo".to_owned());
    let backends = BackendOptions::default()
        .repository(&dir)
        .to_backends()
        .expect("the backend options");
    let repository = Repository::new(&RepositoryOptions::default().no_cache(true), &backends)
        .expect("the repository");
    repository
        .init(
            &Credentials::password("demo-password"),
            &KeyOptions::default(),
            &ConfigOptions::default(),
        )
        .expect("the repository initializes");
    println!("initialized {dir}");
}
