//! A repository's locations (technical design §6.1, decision 80): the places
//! one repository can be reached at, in the order they are tried. The first
//! is the entry's own `repository`, `repo-hot` and option sets; each further
//! one is a `[[repositories.<alias>.locations]]` table with the same keys. A
//! location's options never apply to another location, so one server's
//! credential is never sent to another; the password is the repository's.

use std::collections::BTreeMap;
use std::fmt;

use super::{InvalidValue, RepositoryConfig, RepositorySettings};
use crate::secrets::SecretRef;

/// The most locations a repository has, the first included.
pub const MAX_LOCATIONS: usize = 4;

/// How long a probe of one location may take, connect, read, open and ID
/// together, before it is killed and the location is skipped as silent.
pub const LOCATION_PROBE_SECONDS: i64 = 8;

/// The longest address an operation keeps of a location, in bytes, cut on a
/// character boundary: the record names it, it doesn't reproduce it.
pub const MAX_RECORDED_ADDRESS_BYTES: usize = 256;

/// One place a repository can be reached: a `repository` string, an optional
/// `repo-hot`, and the option sets with the credentials they hold. Its
/// `Debug` form masks what can be a secret, as [`RepositorySettings`]' does.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Location {
    pub repository: String,
    pub repo_hot: Option<String>,
    pub options: BTreeMap<String, String>,
    pub options_hot: BTreeMap<String, String>,
    pub options_cold: BTreeMap<String, String>,
}

impl Location {
    /// The location `settings` describe: everything but the password.
    pub fn of(settings: &RepositorySettings) -> Self {
        let _ = settings;
        todo!()
    }

    /// The settings that open the repository at this location, with the
    /// repository's `password`.
    pub fn with_password(&self, password: SecretRef) -> RepositorySettings {
        let _ = password;
        todo!()
    }
}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f;
        todo!()
    }
}

/// The locations of an entry after its first, in the order written. Empty
/// for a repository reached at one location, which is unchanged in every way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Locations {
    pub further: Vec<Location>,
}

/// Why a location's credentials can be read by whoever answers at its
/// address (§10.6): the cause the warning `location-plain-http-credentials`
/// names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposedCredentials {
    /// A `rest:` location written `http://` that carries a user and password
    /// or a credential option.
    PlainHttp,
    /// An `https` REST location with `http-insecure-tls` true: any
    /// certificate is accepted, so an impostor reads the credentials.
    UncheckedCertificate,
    /// An OpenDAL location whose `endpoint` or `url` is `http://`, or an
    /// rclone connection string whose `url` is, that carries a credential.
    PlainEndpoint,
    /// `opendal:ftp` with a credential.
    PlainFtp,
}

impl ExposedCredentials {
    /// The cause's code, as the warning carries it.
    pub fn code(self) -> &'static str {
        todo!()
    }

    /// The cause of `settings`, a location as written or as resolved, when
    /// whoever answers at its address can read its credentials.
    pub fn of(settings: &RepositorySettings) -> Option<Self> {
        let _ = settings;
        todo!()
    }
}

/// A location of an entry whose credentials can be read by whoever answers
/// at its address. It names the location by its position, from 1, and
/// never its address or a credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocationWarning {
    pub position: u8,
    pub cause: ExposedCredentials,
}

impl RepositoryConfig {
    /// How many locations the entry has: 1 and the further ones.
    pub fn location_count(&self) -> usize {
        todo!()
    }

    /// Whether the entry has more than one location: only then is a job's
    /// location chosen, by a probe of each.
    pub fn has_several_locations(&self) -> bool {
        todo!()
    }

    /// The settings that open the repository at each location, in order,
    /// each with the repository's password.
    pub fn location_settings(&self) -> Vec<RepositorySettings> {
        todo!()
    }

    /// The entry's locations whose credentials can be read by whoever
    /// answers at their address, as written: none for an entry with one
    /// location, which is unchanged in every way.
    pub fn location_warnings(&self) -> Vec<LocationWarning> {
        todo!()
    }
}

/// Refuses a set of further locations an entry can't hold: more than
/// [`MAX_LOCATIONS`] in all.
pub fn check_location_count(further: usize) -> Result<(), InvalidValue> {
    let _ = further;
    todo!()
}
