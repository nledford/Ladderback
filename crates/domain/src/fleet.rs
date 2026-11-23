//! The Fleet context's primitives for pairing an agent with its hub and for
//! the sync handshake (technical design §3.1, §7.1; §16, decisions 120 and
//! 121): Ed25519 keys and signatures as the protocol accepts them, the bytes
//! every signature covers, how a key is shown to a person, and the pairing
//! token. Rules only: the signing itself is the `Signing` port's, and every
//! seed and nonce comes from the `Random` port.

mod fingerprint;
mod greeting;
mod handshake;
mod high_impact;
mod hostname;
mod key_files;
mod keys;
mod machine;
mod outbox;
mod pin;
mod registry;
mod reported;
mod route;
mod signed;
mod token;
mod zone;

pub use fingerprint::{Fingerprint, TokenId};
pub use greeting::{
    AgentEnvelope, Approval, FIRST_SEQUENCE, GENERATION, Hello, HubEnvelope, MAX_EXPIRY_AHEAD,
    MESSAGE_LIFE, PROTOCOLS, Protocols, Welcome, WelcomeRefused, hello_kind, welcome_kind,
};
pub use handshake::{
    BindingKind, Challenge, Connect, HANDSHAKES, Holder, Pair, PairAnswer, Paired, Proof, Refused,
};
pub use high_impact::needs_sign_in;
pub use hostname::{Hostname, InvalidHostname, MAX_HOSTNAME_BYTES};
pub use key_files::{
    COUNTER_LOCK_FILE, ConnectionCounter, CounterRefused, KeyFile, LEFT_OUT_OF_BACKUPS,
    MAX_TLS_KEY_BYTES, SeedFile, TlsKey, TlsKeyRefused, is_left_out, left_out_globs, one_line,
};
pub use keys::{
    KEY_BYTES, KeyRefused, PublicKey, SIGNATURE_BYTES, SMALL_ORDER_KEYS, Signature,
    SignatureRefused, SigningSeed,
};
pub use machine::{
    IdentifierMatch, InvalidMachineIdentifier, MAX_MACHINE_IDENTIFIER_BYTES, MachineIdentifier,
};
pub use outbox::{
    Ack, AckRefused, AgentReport, Happening, Ingest, MAX_EPOCHS_HELD, MAX_OUTBOX_REPORTS,
    MAX_REPORTS_HELD, OutboxAdmission, OutboxEpoch, ReportSeq, SEND_WINDOW, ack_kind, admit,
    check_ack, fresh_on_hub_time, ingest, report_kind,
};
pub use pin::{HubPin, MAX_PIN_BYTES, PinRefused};
pub use registry::{
    AgentRecord, AgentStatus, ApprovalRefused, HostnameKey, HostnameSkeleton, LONGEST_TOKEN_LIFE,
    ListedAgent, Lookalike, PENDING_LIMIT, PairRefusal, PairRequest, Pairing, PairingView,
    ProofRefusal, ProofRequest, ProofView, Proved, SHORTEST_TOKEN_LIFE, TOKEN_LIFE,
    TokenLifeRefused, TokenRecord, TokenUse, UNUSED_TOKEN_LIMIT, UnusedTokensFull,
    admits_unused_token, decide_approval, decide_pairing, decide_proof, list_agents, token_life,
    voids_tokens_for_new_key,
};
pub use reported::{
    OPERATIONS_SHOWN, REPORTS_READ, ReportedOperation, ReportedOutcome, latest_operations,
};
pub use route::{HubRoute, RouteRefused, RouteSession, is_loopback};
pub use signed::{
    Binding, EpochSeconds, Handshakes, InvalidWireName, LABEL_PREFIX, MAX_WIRE_NAME_BYTES, Nonce,
    PayloadDigest, SignedBytes, SignedMessage, WireName,
};
pub use token::{
    HubAddress, HubScheme, InvalidHubAddress, MAX_ADDRESS_BYTES, PairingToken, TOKEN_PREFIX,
    TOKEN_VERSION, TokenRefused, TokenTls,
};
pub use zone::{InvalidZoneName, MAX_ZONE_BYTES, MachineZone};
