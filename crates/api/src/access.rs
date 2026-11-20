//! Sign-in, sessions and API tokens on the wire (technical design §10.1): the
//! auth side of the notifications and auth context (§3.1).

use serde::{Deserialize, Serialize};

use crate::{AvailableUpdateDto, BuildDto};

/// The header that marks a request the web UI sent on its own, such as a
/// re-read after an event or a poll, rather than one a person caused
/// (§10.1). With sign-in on, a marked request never uses its session: only
/// what a person does keeps one from ending unused. Its value is ignored.
/// It can only shorten a session's life, so the daemon trusts it as sent.
pub const BACKGROUND_HEADER: &str = "x-ladderback-background";

/// The header `ladderback mcp` sends with every request in connect mode, so
/// that with sign-in off, when no API token says who is asking, the daemon
/// holds an AI agent's request to what AI agents may do without a token
/// (§10.5, decision 127). Its value is ignored. It can only narrow what a
/// request may do, so the daemon trusts it as sent.
pub const AI_AGENT_HEADER: &str = "x-ladderback-ai-agent";

/// A permission's level, or `none` for no access (§10.1, decision 127).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionLevelDto {
    #[default]
    None,
    Read,
    StartAndCancel,
    Preview,
    Propose,
    Run,
}

/// What an API token, or an AI agent without one, may do: each permission
/// at its level or `none`. A permission a request leaves out is `none`, and
/// a level its permission doesn't have (`"backups": "propose"`) is refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields, default)]
pub struct PermissionsDto {
    pub backups: PermissionLevelDto,
    pub backup_runs: PermissionLevelDto,
    pub previews: PermissionLevelDto,
    pub configuration: PermissionLevelDto,
    pub snapshot_contents: PermissionLevelDto,
    pub daemon_log: PermissionLevelDto,
    pub repository_check: PermissionLevelDto,
}

/// `GET /api/v1/session` and a sign-in's answer (§10.1): whether sign-in is
/// on, and whether the request's session is live. A live session's answer
/// carries the CSRF token its changed requests send as `x-csrf-token`, how
/// long the session lasts unused (`idle-seconds`, left out for a session
/// signed in with Keep me signed in on this device, which has no idle
/// limit), and how long it has left however it is used (`ends-in-seconds`,
/// left out when nothing but its idle limit, a sign-out or a new admin
/// password ends it): the web UI warns before the first ends it (components
/// §4.5). A signed-out answer says whether the sign-in form offers Keep me
/// signed in on this device (`keep-signed-in-offered`), which it doesn't
/// under Stay signed in for Until I close the browser. `exposure` is
/// `network` when the daemon listens beyond loopback, and left out on
/// loopback, as before it existed: with sign-in off, the web UI then warns
/// on every page. With sign-in off, `admin-password` says an admin password
/// `ladderback set-password` has kept since the daemon started, looked for as
/// the route is read: sign-in turns on only at the next start (decision 70),
/// and it is left out while none is kept. The build the daemon runs
/// (`version`, `commit`, `dirty`, `release`) is told to a page that may see
/// the pages, with sign-in off or a live session, and left out for a page
/// that has yet to sign in. A request sending a `read` API token is answered
/// `signed-out` with the build, without `keep-signed-in-offered` or a CSRF
/// token, whatever session cookie it also sends (decision 95). With the
/// build, and only with it, comes the `available-update` the agent's release
/// check found, when there is one (decision 107).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SessionDto {
    pub sign_in: SignInStateDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csrf_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends_in_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "ExposureDto::is_loopback")]
    pub exposure: ExposureDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_password: Option<AdminPasswordDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_signed_in_offered: Option<bool>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_update: Option<AvailableUpdateDto>,
}

/// Where a request stands with sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SignInStateDto {
    /// No admin password is set: there is nothing to sign in to.
    Off,
    /// Sign-in is on and the request carries no live session.
    SignedOut,
    /// The request's session is live.
    SignedIn,
}

/// With sign-in off, the admin password kept since the daemon started
/// (§10.1, decision 70), which only the next start turns sign-in on for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdminPasswordDto {
    /// A hash the daemon can check: a restart turns sign-in on.
    Set,
    /// A file the daemon can't read or check: a restart would refuse to start
    /// until `ladderback set-password` writes it anew.
    Unreadable,
}

/// Who can reach the daemon at the address it listens on (§10.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ExposureDto {
    /// This machine only: a loopback address.
    #[default]
    Loopback,
    /// The network: any other address.
    Network,
}

impl ExposureDto {
    /// Whether this is loopback, which the session's answer leaves out.
    pub fn is_loopback(&self) -> bool {
        *self == Self::Loopback
    }
}

/// `POST /api/v1/session`: the password, as typed, and whether to **Keep me
/// signed in on this device** (`keep-signed-in`, false when left out). The
/// password is never logged or shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SignInDto {
    pub password: String,
    #[serde(default)]
    pub keep_signed_in: bool,
}

impl std::fmt::Debug for SignInDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SignInDto {{ password: [hidden], keep_signed_in: {} }}",
            self.keep_signed_in
        )
    }
}

/// `POST /api/v1/admin-password`: the new admin password, as typed, and, with
/// sign-in on, the current one, which a change needs (§10.1, decision 70).
/// With sign-in off the current one is not asked for, and not read. Never
/// logged or shown (I4): its `Debug` hides both.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SetAdminPasswordDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_password: Option<String>,
    pub new_password: String,
}

impl std::fmt::Debug for SetAdminPasswordDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = if self.current_password.is_some() {
            "Some([hidden])"
        } else {
            "None"
        };
        write!(
            f,
            "SetAdminPasswordDto {{ current_password: {current}, new_password: [hidden] }}"
        )
    }
}

/// The session length, as **Stay signed in for** names it (§10.1): how long
/// every session lasts after its sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionLengthDto {
    #[serde(rename = "until-browser-closes")]
    UntilBrowserCloses,
    #[serde(rename = "7-days")]
    SevenDays,
    #[serde(rename = "30-days")]
    ThirtyDays,
    #[serde(rename = "90-days")]
    NinetyDays,
    #[serde(rename = "1-year")]
    OneYear,
    #[serde(rename = "until-sign-out")]
    UntilSignOut,
}

/// `GET` and `PUT /api/v1/settings/sign-in`: the sign-in settings, which
/// only a session reads or changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SignInSettingsDto {
    pub session_length: SessionLengthDto,
}

/// One API token as the token routes list it: who it is, what it may do,
/// when it was created and last used. Never its secret (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApiTokenDto {
    pub id: String,
    pub label: String,
    pub permissions: PermissionsDto,
    pub created_at: String,
    /// `null` for a token never used.
    #[serde(default)]
    pub last_used_at: Option<String>,
}

/// `GET /api/v1/tokens`: the tokens kept, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ApiTokensDto {
    pub tokens: Vec<ApiTokenDto>,
}

/// `POST /api/v1/tokens`: the label and permissions of the token to create.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CreateApiTokenDto {
    pub label: String,
    pub permissions: PermissionsDto,
}

/// `PUT /api/v1/tokens/{id}/permissions`: the permissions the token keeps
/// from now on, and the current admin password, which a change needs, so
/// that a stolen session alone can't widen a token. Its `Debug` hides the
/// password (I4).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ChangeApiTokenPermissionsDto {
    pub permissions: PermissionsDto,
    pub current_password: String,
}

impl std::fmt::Debug for ChangeApiTokenPermissionsDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangeApiTokenPermissionsDto")
            .field("permissions", &self.permissions)
            .field("current_password", &format_args!("[hidden]"))
            .finish()
    }
}

/// `PUT /api/v1/settings/without-token`: what AI agents may do without a
/// token from now on (decision 127), and, with sign-in on, the current admin
/// password, which a change then needs; with sign-in off there is none to
/// give. Its `Debug` hides the password (I4).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ChangeWithoutTokenDto {
    pub permissions: PermissionsDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_password: Option<String>,
}

impl std::fmt::Debug for ChangeWithoutTokenDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangeWithoutTokenDto")
            .field("permissions", &self.permissions)
            .field(
                "current_password",
                &self.current_password.as_ref().map(|_| "[hidden]"),
            )
            .finish()
    }
}

/// What creating a token answers: the token as it is listed, and its whole
/// text, `ladderback_<id>_<secret>`, shown this once. Its `Debug` hides the
/// secret (I4).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CreatedApiTokenDto {
    pub token: ApiTokenDto,
    pub secret: String,
}

impl std::fmt::Debug for CreatedApiTokenDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreatedApiTokenDto")
            .field("token", &self.token)
            .field("secret", &"[hidden]")
            .finish()
    }
}

#[cfg(test)]
mod tests;
