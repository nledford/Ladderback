//! Permissions (technical design §10.1, decision 127): what a program that
//! isn't a person's browser may do. Each API token carries its own, as
//! GitHub's fine-grained tokens do, and one setting holds what AI agents may
//! do without a token, through `ladderback mcp` in headless and scratch
//! modes. A permission is a row a person sets to no access or to its level;
//! a route or an MCP tool needs one [`Permission`]. Rules only: where they
//! are kept and who changes them is the caller's.

use crate::planning::{InvalidValue, ValueKind};

/// A row of the permissions a person sets for a token, or for AI agents
/// without one, and what one route or MCP tool needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Permission {
    /// Seeing plans, repositories, snapshots, operations and status.
    Backups,
    /// Starting and cancelling backups, without reading anything.
    BackupRuns,
    /// What forget or an import would do.
    Previews,
    /// Proposing configuration changes for a person to approve (decision 89).
    Configuration,
    /// File and folder names inside snapshots.
    SnapshotContents,
    /// The daemon's log.
    DaemonLog,
    /// Starting a repository check. No route or tool uses it yet.
    RepositoryCheck,
}

impl Permission {
    /// Every row, in the order a person sees them.
    pub const ALL: [Self; 7] = [
        Self::Backups,
        Self::BackupRuns,
        Self::Previews,
        Self::Configuration,
        Self::SnapshotContents,
        Self::DaemonLog,
        Self::RepositoryCheck,
    ];

    /// Its name on the wire, in the store, in `config.toml` and in a refusal.
    pub fn as_str(self) -> &'static str {
        todo!()
    }

    /// The row `text` names, exactly as [`Self::as_str`] writes it.
    pub fn parse(text: &str) -> Option<Self> {
        todo!()
    }

    /// The level the row grants when it isn't at no access. Each row has
    /// one level today; the level is still named, on the wire and in the
    /// store, so a row can gain a second one without a new format.
    pub fn level(self) -> Level {
        todo!()
    }

    fn bit(self) -> u8 {
        match self {
            Self::Backups => 1,
            Self::BackupRuns => 2,
            Self::Previews => 4,
            Self::Configuration => 8,
            Self::SnapshotContents => 16,
            Self::DaemonLog => 32,
            Self::RepositoryCheck => 64,
        }
    }
}

/// What a row is set to, short of no access.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    Read,
    StartAndCancel,
    Preview,
    Propose,
    Run,
}

impl Level {
    const ALL: [Self; 5] = [
        Self::Read,
        Self::StartAndCancel,
        Self::Preview,
        Self::Propose,
        Self::Run,
    ];

    /// Its name on the wire, in the store and in `config.toml`.
    pub fn as_str(self) -> &'static str {
        todo!()
    }

    /// The level `text` names, exactly as [`Self::as_str`] writes it.
    pub fn parse(text: &str) -> Option<Self> {
        todo!()
    }
}

/// A level its row doesn't have, as `backups = "propose"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoSuchLevel {
    pub permission: Permission,
    pub level: Level,
}

/// The rows that are on. The default is what a new token, and AI agents
/// without one, may do: see backups, start and cancel them, preview and
/// propose. What can show a file's name or a log line to whatever model an
/// agent uses, or costs time and money, is off until a person turns it on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions(u8);

impl Default for Permissions {
    fn default() -> Self {
        Self(
            Permission::Backups.bit()
                | Permission::BackupRuns.bit()
                | Permission::Previews.bit()
                | Permission::Configuration.bit(),
        )
    }
}

impl Permissions {
    /// Every row at no access.
    pub const NONE: Self = Self(0);

    /// Whether `permission` is granted.
    pub fn grants(self, permission: Permission) -> bool {
        todo!()
    }

    /// These permissions with `permission` set to `level` (`None`: no
    /// access), the other rows as they are.
    pub fn with(self, permission: Permission, level: Option<Level>) -> Result<Self, NoSuchLevel> {
        todo!()
    }

    /// These permissions with `permission` set as the configuration file
    /// names it: `"none"` for no access, or its level.
    ///
    /// # Errors
    ///
    /// `text` is neither, or names a level `permission` doesn't have.
    pub fn with_named(self, permission: Permission, text: &str) -> Result<Self, InvalidValue> {
        todo!()
    }

    /// Whether every row is at no access.
    pub fn is_empty(self) -> bool {
        todo!()
    }

    /// The rows that are on, with their level, in [`Permission::ALL`]'s
    /// order.
    pub fn rows(self) -> impl Iterator<Item = (Permission, Level)> {
        Permission::ALL
            .into_iter()
            .filter(move |permission| self.grants(*permission))
            .map(|permission| (permission, permission.level()))
    }
}

#[cfg(test)]
mod tests;
