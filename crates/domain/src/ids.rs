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
    /// A repository's alias in the config, not its rustic ID.
    RepositoryAlias,
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
    if value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(InvalidId {
            kind,
            value: value.to_owned(),
        })
    }
}

/// Identifies a plan within one agent's config: its key in `config.toml`
/// (technical design §6.1). Lowercase letters, digits and hyphens, starting
/// with a letter or digit, at most 64 characters, so it fits a snapshot tag.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlanId(String);

/// Whether `value` is a slug: at most 64 bytes of lowercase letters, digits
/// and hyphens, the first not a hyphen. What a plan's ID and a repository's
/// alias are made of.
fn is_slug(value: &str) -> bool {
    let slug = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    value.len() <= 64
        && value.bytes().next().is_some_and(slug)
        && value.bytes().all(|b| slug(b) || b == b'-')
}

/// The slug a free-form name becomes: the lowercase ASCII letters and digits
/// of `text`, every other run of characters one hyphen, no hyphen at either
/// end, at most 64 bytes. `None` when nothing is left.
#[allow(clippy::todo, reason = "red test: interop import")]
fn slugify(text: &str) -> Option<String> {
    let _ = text;
    todo!("slugify a free-form name")
}

impl PlanId {
    /// The ID a name goes by in the config: `Home Dir` is `home-dir`. `None`
    /// when the name has no letter or digit of ASCII in it.
    pub fn slugify(text: &str) -> Option<Self> {
        slugify(text).map(Self)
    }

    pub fn parse(value: &str) -> Result<Self, InvalidId> {
        if is_slug(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidId {
                kind: IdKind::Plan,
                value: value.to_owned(),
            })
        }
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

/// Names a repository within one agent's config: the key of its
/// `[repositories.<alias>]` table (technical design §6.1, §7.3). The same
/// slug as a plan's ID. It is only a local alias: a repository is identified
/// by its [`RepositoryId`], and two machines may call it by different aliases.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepositoryAlias(String);

impl RepositoryAlias {
    /// The alias a free-form name becomes (a profile's file name, say), as
    /// [`PlanId::slugify`] makes an ID.
    pub fn slugify(text: &str) -> Option<Self> {
        slugify(text).map(Self)
    }

    pub fn parse(value: &str) -> Result<Self, InvalidId> {
        if is_slug(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidId {
                kind: IdKind::RepositoryAlias,
                value: value.to_owned(),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepositoryAlias {
    type Error = InvalidId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<RepositoryAlias> for String {
    fn from(alias: RepositoryAlias) -> Self {
        alias.0
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

    #[test]
    fn a_repository_alias_is_a_short_lowercase_slug() {
        for value in ["nas", "0", "backup-2026", &"a".repeat(64)] {
            assert_eq!(RepositoryAlias::parse(value).unwrap().as_str(), value);
        }
    }

    #[test]
    fn an_alias_cannot_be_anything_else() {
        for value in [
            "",
            "-nas",
            "Nas",
            "nas.lan",
            "my nas",
            "nas/1",
            "nas_1",
            "café",
            &"a".repeat(65),
        ] {
            assert_eq!(
                RepositoryAlias::parse(value),
                Err(InvalidId {
                    kind: IdKind::RepositoryAlias,
                    value: value.to_owned()
                })
            );
        }
        assert!(serde_json::from_str::<RepositoryAlias>("\"Nas\"").is_err());
    }

    #[test]
    fn an_alias_is_stored_as_its_slug() {
        let alias = RepositoryAlias::parse("nas-2").unwrap();
        let json = serde_json::to_string(&alias).unwrap();
        assert_eq!(json, "\"nas-2\"");
        assert_eq!(
            serde_json::from_str::<RepositoryAlias>(&json).unwrap(),
            alias
        );
    }

    #[test]
    fn a_name_becomes_a_slug_of_lowercase_letters_and_digits() {
        for (name, slug) in [
            ("home", "home"),
            ("Home Dir", "home-dir"),
            ("  --Work_Laptop!!  Docs ", "work-laptop-docs"),
            ("demo data/", "demo-data"),
            ("Café 2026", "caf-2026"),
        ] {
            assert_eq!(PlanId::slugify(name).unwrap().as_str(), slug, "{name:?}");
            assert_eq!(RepositoryAlias::slugify(name).unwrap().as_str(), slug);
        }
    }

    #[test]
    fn a_name_without_a_letter_or_digit_has_no_slug() {
        for name in ["", "/", " - ", "日本", "!!"] {
            assert_eq!(PlanId::slugify(name), None, "{name:?}");
            assert_eq!(RepositoryAlias::slugify(name), None);
        }
    }

    #[test]
    fn a_slug_is_cut_at_64_bytes_and_never_ends_in_a_hyphen() {
        let long = format!("{}-{}", "a".repeat(63), "b".repeat(10));
        assert_eq!(PlanId::slugify(&long).unwrap().as_str(), "a".repeat(63));
        let exact = "c".repeat(64);
        assert_eq!(PlanId::slugify(&exact).unwrap().as_str(), exact);
        assert_eq!(
            PlanId::slugify(&"d".repeat(100)).unwrap().as_str().len(),
            64
        );
    }

    #[test]
    fn a_slug_is_always_a_valid_id() {
        for name in ["Home Dir", "a--b", "-x-", &"y z ".repeat(40)] {
            let id = PlanId::slugify(name).unwrap();
            assert_eq!(PlanId::parse(id.as_str()), Ok(id));
        }
    }
}
