use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies one operation (shared kernel). Generated outside the domain
/// (UUIDv7 from the `Random` port), never here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(Uuid);

impl OperationId {
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

/// Identifies one agent (shared kernel): a UUIDv7 the agent generates once
/// and keeps in its data dir (technical design §7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(Uuid);

impl AgentId {
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

/// A value that isn't a valid ID of its kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidId {
    pub kind: IdKind,
    pub value: String,
}

impl std::fmt::Display for InvalidId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for InvalidId {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdKind {
    Repository,
    Snapshot,
    Plan,
}

macro_rules! rustic_id {
    ($(#[$doc:meta])* $name:ident, $kind:expr) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Accepts rustic's form of an ID: 64 hex digits, in either case.
            pub fn parse(value: &str) -> Result<Self, InvalidId> {
                parse_rustic_id(value, $kind).map(Self)
            }

            /// The ID as 64 lowercase hex digits.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = InvalidId;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

rustic_id!(
    /// A rustic repository's ID (shared kernel): random, created at `init`,
    /// and the same on every machine that reaches the repository (§7.3).
    RepositoryId,
    IdKind::Repository
);

rustic_id!(
    /// A rustic snapshot's ID (shared kernel).
    SnapshotId,
    IdKind::Snapshot
);

fn parse_rustic_id(value: &str, kind: IdKind) -> Result<String, InvalidId> {
    todo!()
}

/// Identifies a plan within one agent's config: its key in `config.toml`
/// (technical design §6.1). Lowercase letters, digits and hyphens, starting
/// with a letter or digit, at most 64 characters, so it fits a snapshot tag.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlanId(String);

impl PlanId {
    pub fn parse(value: &str) -> Result<Self, InvalidId> {
        todo!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PlanId {
    type Error = InvalidId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<PlanId> for String {
    fn from(id: PlanId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn a_rustic_id_is_64_hex_digits_kept_in_lowercase() {
        let id = RepositoryId::parse(&HEX.to_uppercase()).unwrap();
        assert_eq!(id.as_str(), HEX);
        assert_eq!(SnapshotId::parse(HEX).unwrap().as_str(), HEX);
    }

    #[test]
    fn anything_else_is_not_a_rustic_id() {
        for value in ["", &HEX[1..], &format!("{HEX}0"), &HEX.replace('a', "g")] {
            assert_eq!(
                RepositoryId::parse(value),
                Err(InvalidId {
                    kind: IdKind::Repository,
                    value: value.to_owned()
                })
            );
            assert_eq!(SnapshotId::parse(value).unwrap_err().kind, IdKind::Snapshot);
        }
    }

    #[test]
    fn rustic_ids_are_validated_when_read() {
        let json = format!("\"{HEX}\"");
        let id: SnapshotId = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), json);
        assert!(serde_json::from_str::<SnapshotId>("\"abc\"").is_err());
    }

    #[test]
    fn a_plan_id_is_a_short_lowercase_slug() {
        for value in ["home", "0", "photos-2026", &"a".repeat(64)] {
            assert_eq!(PlanId::parse(value).unwrap().as_str(), value);
        }
    }

    #[test]
    fn a_plan_id_cannot_break_a_snapshot_tag() {
        for value in [
            "",
            "-home",
            "Home",
            "home,work",
            "home:work",
            "home work",
            "café",
            &"a".repeat(65),
        ] {
            assert_eq!(
                PlanId::parse(value),
                Err(InvalidId {
                    kind: IdKind::Plan,
                    value: value.to_owned()
                })
            );
        }
        assert!(serde_json::from_str::<PlanId>("\"Home\"").is_err());
    }

    #[test]
    fn a_plan_id_is_stored_as_its_slug() {
        let id = PlanId::parse("home-docs").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"home-docs\"");
        assert_eq!(serde_json::from_str::<PlanId>(&json).unwrap(), id);
    }
}
