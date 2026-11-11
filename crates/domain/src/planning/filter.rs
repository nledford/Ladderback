//! What a plan leaves out of its backup (technical design §6.1): rustic's
//! `Excludes` and `LocalSourceFilterOptions`, under the keys rustic's
//! profiles use.
//!
//! Left out on purpose: `exclude-larger-than`, which would drop a file from a
//! snapshot with no warning, for a size the user set once and forgot.

use super::GlobFile;

/// The patterns, marker files and switches that decide what a plan's
/// sources contribute. Every list is in the order it was written, and
/// nothing here is read from disk: a glob file that doesn't exist is a
/// failure when the plan runs, not a refusal here.
///
/// The whole of it is optional: the default leaves nothing out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceFilter {
    /// Glob patterns; a leading `!` excludes (rustic's `globs`).
    pub globs: Vec<String>,
    /// The same, case-insensitively (`iglobs`).
    pub iglobs: Vec<String>,
    /// Files that hold glob patterns, one per line (`glob-files`). Each is an
    /// absolute path, so it names the same file however the daemon started.
    pub glob_files: Vec<GlobFile>,
    /// The same, case-insensitively (`iglob-files`).
    pub iglob_files: Vec<GlobFile>,
    /// Leave out what `.gitignore` files leave out (`git-ignore`).
    pub git_ignore: bool,
    /// Apply `.gitignore` files outside a git repository too
    /// (`no-require-git`).
    pub no_require_git: bool,
    /// File names to treat like `.gitignore` (`custom-ignorefiles`).
    pub custom_ignorefiles: Vec<String>,
    /// Leave out a folder that holds a file of this name (`exclude-if-present`).
    pub exclude_if_present: Vec<String>,
    /// Leave out what has this extended attribute (`exclude-if-xattr`).
    pub exclude_if_xattr: Vec<String>,
    /// Don't cross into other file systems (`one-file-system`).
    pub one_file_system: bool,
}
