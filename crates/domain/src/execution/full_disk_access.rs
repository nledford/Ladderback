//! Full Disk Access on macOS (technical design §3.1, §16 decision 115): what
//! the agent's Full Disk Access probe found, and the rule that turns a source
//! item the grant explains into a warning of its own.

use super::{Warning, WarningKind};

/// What the Full Disk Access probe found when it listed a folder only that
/// grant opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullDiskAccess {
    /// The folder listed: the person has granted it.
    Granted,
    /// The system refused the listing (`EPERM`): the grant is missing.
    Missing,
    /// Any other answer, a folder that isn't there among them. Never
    /// reported as missing: a false alarm would send a person to System
    /// Settings for nothing.
    Unknown,
}

/// What an engine's message holds when the system refused it with `EPERM`,
/// "Operation not permitted", as the standard library writes an I/O error,
/// which rustic's own message repeats after "caused by". A file's own
/// permissions refuse with `EACCES` (`os error 13`), which no grant fixes.
const NOT_PERMITTED: &str = "(os error 1)";

impl FullDiskAccess {
    /// `warning`, as what the probe found before its backup explains it: a
    /// source item the system refused with `EPERM` while the grant was
    /// missing is [`WarningKind::FullDiskAccessMissing`], its item, step and
    /// message as they were. Every other warning, and every warning when the
    /// grant was found or unknown, is as it came.
    pub fn explain(self, warning: Warning) -> Warning {
        let explained = self == Self::Missing
            && warning.kind == WarningKind::SourceUnreadable
            && warning.detail.contains(NOT_PERMITTED);
        if explained {
            Warning {
                kind: WarningKind::FullDiskAccessMissing,
                ..warning
            }
        } else {
            warning
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(detail: &str) -> Warning {
        Warning {
            kind: WarningKind::SourceUnreadable,
            item: Some("/Users/demo/Library/Mail".to_owned()),
            during: "scan".to_owned(),
            detail: detail.to_owned(),
        }
    }

    /// As rustic reports a source it couldn't read (`display_log`).
    const RUSTIC_EPERM: &str = "Error: Failed to read the directory `/Users/demo/Library/Mail`. (kind: related to input/output operations): caused by: Operation not permitted (os error 1)";

    #[test]
    fn an_item_refused_with_eperm_while_the_grant_is_missing_is_the_grant_s_warning() {
        let explained = FullDiskAccess::Missing.explain(refused(RUSTIC_EPERM));
        assert_eq!(
            explained,
            Warning {
                kind: WarningKind::FullDiskAccessMissing,
                ..refused(RUSTIC_EPERM)
            }
        );
    }

    #[test]
    fn eperm_anywhere_in_the_message_counts_as_the_grant_s_refusal() {
        let detail =
            "Error: open failed: caused by: Operation not permitted (os error 1) : (source: x)";
        assert_eq!(
            FullDiskAccess::Missing.explain(refused(detail)).kind,
            WarningKind::FullDiskAccessMissing
        );
    }

    #[test]
    fn an_item_its_own_permissions_refuse_stays_unreadable() {
        for detail in [
            "caused by: Permission denied (os error 13)",
            "caused by: No such file or directory (os error 2)",
            "caused by: Too many levels of symbolic links (os error 10)",
            "permission denied",
        ] {
            assert_eq!(
                FullDiskAccess::Missing.explain(refused(detail)),
                refused(detail),
                "{detail}"
            );
        }
    }

    #[test]
    fn a_grant_found_or_unknown_changes_no_warning() {
        for found in [FullDiskAccess::Granted, FullDiskAccess::Unknown] {
            assert_eq!(found.explain(refused(RUSTIC_EPERM)), refused(RUSTIC_EPERM));
        }
    }

    #[test]
    fn a_warning_of_another_kind_is_never_the_grant_s() {
        let hook = Warning {
            kind: WarningKind::Hook,
            ..refused(RUSTIC_EPERM)
        };
        assert_eq!(FullDiskAccess::Missing.explain(hook.clone()), hook);
    }
}
