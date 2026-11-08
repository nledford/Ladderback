//! Finds the message IDs product code names, for the unused-ID lint. It lexes
//! Rust with `proc-macro2`, so comments and strings never count, and it skips
//! test code.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What one source file names through `msg::`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// The constants named: `CLI_LISTENING` for `msg::CLI_LISTENING`.
    pub names: BTreeSet<String>,
    /// Whether a `msg::*` import hides what is used.
    pub globs: bool,
}

/// The constants of `l10n::msg` that `source` names outside test code. An
/// error if the text isn't Rust a lexer can read.
pub fn scan(source: &str) -> Result<Usage, String> {
    let _ = source;
    todo!()
}

/// The workspace's product sources: `src/` and every crate's `src/`, minus
/// tests, benches, examples and build output.
pub fn product_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for top in ["src", "crates"] {
        collect(&root.join(top), &mut found);
    }
    found.sort();
    found
}

fn collect(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            let skipped = matches!(
                name.as_ref(),
                "target" | "tests" | "benches" | "examples" | "node_modules" | "dist" | ".git"
            );
            if !skipped {
                collect(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") && name != "tests.rs"
        {
            found.push(path);
        }
    }
}
