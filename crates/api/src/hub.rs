//! The hub's own routes under `/api/v1/hub` (§16, decisions 120 and 121):
//! its key, and the pairing tokens its page makes and cancels. Every daemon
//! also answers which
//! hub it is paired with, at [`PAIRING_PATH`].

use serde::{Deserialize, Serialize};

/// Where the hub's key is read: `GET`, answered 404 by a daemon that isn't
/// a hub.
pub const HUB_PATH: &str = "/api/v1/hub";

/// Where the page makes a pairing token: `POST` a [`MakePairingTokenDto`].
pub const PAIRING_TOKENS_PATH: &str = "/api/v1/hub/pairing-tokens";

/// Where a machine says whether it is paired with a hub, and with which:
/// `GET`, answering a [`PairingDto`] (components §4.6, `pairing_state`).
pub const PAIRING_PATH: &str = "/api/v1/pairing";

/// The certificate a pairing token's address shows: the hub's own, which
/// the token pins, or one a certificate authority signed, as a proxy's in
/// front of the hub is (decision 120, "Binding to the TLS session").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TokenCertificateDto {
    Pinned,
    #[default]
    Ca,
}

/// The hub's key as a person compares it and, with sign-in off, types it
/// for a step-up: its fingerprint in full (20 Crockford base32 characters
/// in groups of four), the one ID a screen shows for it; the certificate a
/// token is offered first, the hub's own when the page came over the hub's
/// built-in TLS; and the fingerprint of that certificate, when the hub
/// serves one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HubDto {
    pub fingerprint: String,
    #[serde(default)]
    pub certificate: TokenCertificateDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own_certificate: Option<String>,
}

/// Whether this machine is paired with a hub: true once the hub accepted
/// its pairing, whether or not a person has approved it there yet. A paired
/// machine names its hub.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PairingDto {
    pub paired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub: Option<PairedHubDto>,
}

/// The hub a machine is paired with, as the machine pinned it: the address
/// it reaches the hub at, the fingerprint of the hub's key in full (as
/// [`HubDto`] has it, for a person to compare with the hub's page), and the
/// certificate the machine trusts there, none for a plain `http://` address.
/// All of it is public: the pin holds no secret, and no token is kept once
/// used (I4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PairedHubDto {
    pub address: String,
    pub fingerprint: String,
    /// How the hub's certificate is checked: absent for a plain `http://`
    /// address, which isn't encrypted and shows no certificate; the hub key
    /// still proves the hub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate: Option<TokenCertificateDto>,
}

/// A pairing token asked for: the address agents will use, the certificate
/// that address shows (a certificate authority's when absent), its life in
/// minutes (15 when absent, 5 to 1440), and the step-up confirmation: the
/// admin password with sign-in on, the hub key's key ID otherwise. The
/// confirmation is never logged or shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MakePairingTokenDto {
    pub address: String,
    #[serde(default)]
    pub certificate: TokenCertificateDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub life_minutes: Option<u32>,
    pub confirmation: String,
}

impl std::fmt::Debug for MakePairingTokenDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MakePairingTokenDto")
            .field("address", &self.address)
            .field("certificate", &self.certificate)
            .field("life_minutes", &self.life_minutes)
            .finish_non_exhaustive()
    }
}

/// A pairing token made, shown once: its text, its token ID, the hub key's
/// fingerprint it carries, its address, when it lapses (RFC 3339), how
/// many seconds it has left by the hub's clock, which the page counts down
/// on its own monotonic clock, and the certificate its address shows. Its
/// `Debug` hides the text (I4).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PairingTokenDto {
    pub token: String,
    pub token_id: String,
    pub fingerprint: String,
    pub address: String,
    pub expires_at: String,
    pub seconds_left: u64,
    /// The certificate the token's address shows, as it was made: none for
    /// a plain `http://` address, which shows no certificate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate: Option<TokenCertificateDto>,
}

impl std::fmt::Debug for PairingTokenDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingTokenDto")
            .field("token_id", &self.token_id)
            .field("fingerprint", &self.fingerprint)
            .field("address", &self.address)
            .field("certificate", &self.certificate)
            .field("expires_at", &self.expires_at)
            .field("seconds_left", &self.seconds_left)
            .finish_non_exhaustive()
    }
}

/// Where the page reads the machines the hub has registered: `GET`,
/// answered with a [`HubAgentsDto`].
pub const HUB_MACHINES_PATH: &str = "/api/v1/hub/machines";

/// Where the page approves the waiting machine `machine_id`: `POST` an
/// [`ApproveAgentDto`], answered with the machine's [`HubAgentDto`].
pub fn machine_approval_path(machine_id: &str) -> String {
    format!("{HUB_MACHINES_PATH}/{machine_id}/approval")
}

/// Where the page rejects the waiting machine `machine_id`, or removes it
/// once approved: `POST` a [`RemoveAgentDto`], answered `204 No Content`.
pub fn machine_removal_path(machine_id: &str) -> String {
    format!("{HUB_MACHINES_PATH}/{machine_id}/removal")
}

/// Whether a registered agent waits for a person's approval or has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentStatusDto {
    Pending,
    Approved,
}

/// The agent a waiting agent's hostname looks like (decision 120,
/// "Hostnames"): its agent ID, its hostname and when the hub last heard
/// from it (RFC 3339).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct LookalikeDto {
    pub machine_id: String,
    pub hostname: String,
    pub last_seen: String,
}

/// One agent the hub has registered, as a person compares it with the
/// machine's own screen: its agent ID, its hostname, its key (64 hex
/// digits, which an approval names back) and that key's fingerprint in full;
/// whether it waits or is approved, since when (RFC 3339) and how many
/// seconds ago that was by the hub's clock, so a browser whose clock is
/// wrong still shows it right; while it waits, the agent its hostname looks
/// like; and when the hub last refused a second computer that connected
/// under its agent ID (RFC 3339), until a person has the hub forget the
/// computer it took.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HubAgentDto {
    pub machine_id: String,
    pub hostname: String,
    pub key: String,
    pub fingerprint: String,
    pub status: AgentStatusDto,
    pub since: String,
    pub seconds_ago: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub looks_like: Option<LookalikeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_machine_at: Option<String>,
}

/// The agents the hub has registered, oldest first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HubAgentsDto {
    pub machines: Vec<HubAgentDto>,
}

/// Where the hub's dashboard reads its approved machines and their latest
/// operations (§4.3, #569): `GET`, answered with a [`HubMachinesDto`].
pub const HUB_DASHBOARD_PATH: &str = "/api/v1/hub/dashboard";

/// How a machine's operation came out, as far as the hub heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineOutcomeDto {
    /// It started, and the hub heard no end yet.
    Running,
    Succeeded,
    Failed,
    // The wire keeps the British spelling: changing it would change the API.
    #[serde(rename = "cancelled")]
    Canceled,
}

/// One of a machine's latest operations, from its reports (§7.2): when it
/// started and ended by the machine's clock (RFC 3339), either unknown when
/// the hub didn't take that report, how it came out, and the wire name of
/// the error code it failed with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MachineOperationDto {
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub outcome: MachineOutcomeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// One approved machine on the hub's dashboard: when the hub last heard
/// from it (RFC 3339), whether it is online, its IANA time zone when it
/// gave one, how many of its reports the hub let go of past the cap, and
/// its latest operations, newest first. Online means the hub holds the
/// machine's sync connection, which it closes once the machine sent
/// nothing for 90 seconds (§16, decision 159).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HubMachineDto {
    pub machine_id: String,
    pub hostname: String,
    pub last_seen: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    pub reports_dropped: u64,
    pub operations: Vec<MachineOperationDto>,
    /// Its plans as it last reported them; none before it reported any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plans: Option<MachinePlansDto>,
    /// The latest progress of the operation it runs, while the hub's
    /// reports say it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<MachineProgressDto>,
}

/// The hub's approved machines, oldest first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HubMachinesDto {
    pub machines: Vec<HubMachineDto>,
}

/// An approval asked for: the key the person compared (64 hex digits, as
/// [`HubAgentDto::key`]), so the hub approves that agent with that key and
/// no other, and the step-up confirmation: the admin password with sign-in
/// on, the hub key's key ID otherwise. The confirmation is never logged or
/// shown (I4): its `Debug` hides it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ApproveAgentDto {
    pub key: String,
    pub confirmation: String,
}

impl std::fmt::Debug for ApproveAgentDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApproveAgentDto")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// A rejection or removal asked for: the key the person saw in the
/// machine's row (64 hex digits, as [`HubAgentDto::key`]), so the hub
/// removes that agent with that key and no other. No step-up: a removal
/// only takes authority away.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RemoveAgentDto {
    pub key: String,
}

/// Where the hub's page reads whether the hub is in recovery mode (§7.3,
/// decision 137): `GET`, answered with a [`HubRecoveryDto`].
pub const HUB_RECOVERY_PATH: &str = "/api/v1/hub/recovery";

/// Where a person ends recovery mode, once the old hub is stopped or gone:
/// `POST` with an [`EndRecoveryDto`], answered `204` whether or not the hub
/// was in recovery mode.
pub const HUB_RECOVERY_CONFIRMATION_PATH: &str = "/api/v1/hub/recovery/confirmation";

/// Whether the hub is in recovery mode: a machine proved a lease above
/// what the hub counts, so it grants and renews no lease until a person
/// confirms the old hub is stopped or gone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HubRecoveryDto {
    pub recovering: bool,
}

/// The end of recovery mode asked for, with the step-up confirmation, as
/// [`ApproveAgentDto::confirmation`]. Its `Debug` hides it (I4).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct EndRecoveryDto {
    pub confirmation: String,
}

impl std::fmt::Debug for EndRecoveryDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EndRecoveryDto").finish_non_exhaustive()
    }
}

/// A machine's plans as it last reported them (§7.2): at most
/// `domain::fleet::MAX_PLANS_REPORTED`, in its configuration's order, and
/// how many more it has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MachinePlansDto {
    pub plans: Vec<MachinePlanDto>,
    pub left_out: u64,
}

/// One of a machine's plans: its ID, its last run, when its last successful
/// backup ended, and its next planned run, by the machine's clock (RFC
/// 3339), each none when it has none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MachinePlanDto {
    pub plan: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<MachinePlanRunDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run: Option<String>,
}

/// A plan's last run: when it last changed (RFC 3339) and its status, as
/// the history page names it: `queued`, `running`, `succeeded`,
/// `succeeded-with-warnings`, `failed` or `cancelled`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MachinePlanRunDto {
    pub at: String,
    pub status: String,
}

/// The latest progress of a machine's running operation `operation`, as
/// the local Operations page shows one (decision 83): its step, what it
/// counts, how far it came of its total, and when the machine heard it
/// (RFC 3339).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MachineProgressDto {
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<crate::OperationStep>,
    pub unit: crate::ProgressUnit,
    pub current: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    pub at: String,
}
