//! The handshake's messages between the core's types and their frames on the
//! wire (`api::sync`): a key, a nonce and a signature as the unpadded
//! base64url of their bytes, an agent ID as its UUID's text. A frame is read
//! into the core's types here, once, as it arrives, and a field that doesn't
//! hold what it must refuses the whole frame.

use api::sync::{
    AckPayload, AgentApproval, AgentFrame, GrantPayload, GrantsPayload, HelloPayload, HolderFrame,
    HubFrame, MAX_FRAME_BYTES, ReleasePayload, ReleasedPayload, RepoPayload, ReportPayload,
    ReposPayload, WelcomePayload,
};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use domain::fleet::{
    Ack, AgentEnvelope, AgentReport, Approval, BindingKind, Challenge, Connect, EpochSeconds,
    Handshakes, Happening, Hello, Holder, Hostname, HubEnvelope, KEY_BYTES, MachineFingerprint,
    MachineZone, Nonce, OutboxEpoch, Pair, PairAnswer, Paired, Proof, Protocols, PublicKey,
    Refused, ReportSeq, SIGNATURE_BYTES, Signature, Welcome, WireName,
};
use domain::maintenance::{Grant, LeaseEpoch, RepoGrants, RepositoryReport, RepositoryReports};
use domain::planning::Maintenance;
use domain::{AgentId, OperationId, RepositoryId, Timestamp, Uuid};

use crate::ports::OutboxEntry;

/// What an agent's frame holds, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentMessage {
    Connect(Connect),
    Pair(Pair),
    Proof(Proof),
    Signed(AgentEnvelope),
}

/// What a hub's frame holds, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubMessage {
    Challenge(Challenge),
    Answer(PairAnswer),
    Signed(HubEnvelope),
}

/// A frame whose field doesn't hold what it must: bytes of another length or
/// encoding, a key the protocol refuses, an agent ID that isn't a UUID, an
/// unknown binding kind, or a hostname or code that isn't one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireRefused;

pub fn connect_frame(connect: &Connect) -> AgentFrame {
    AgentFrame::Connect {
        agent_id: connect.agent_id.as_uuid().hyphenated().to_string(),
        agent_nonce: encoded(connect.agent_nonce.as_bytes()),
        binding_kind: connect.binding_kind.byte(),
    }
}

pub fn pair_frame(pair: &Pair) -> AgentFrame {
    AgentFrame::Pair {
        pairing_key: encoded(pair.pairing_key.as_bytes()),
        agent_key: encoded(pair.agent_key.as_bytes()),
        counter: pair.counter,
        hostname: pair.hostname.as_str().to_owned(),
        zone: pair.zone.as_ref().map(|zone| zone.as_str().to_owned()),
        pairing_signature: encoded(pair.pairing_signature.as_bytes()),
        agent_signature: encoded(pair.agent_signature.as_bytes()),
    }
}

pub fn challenge_frame(challenge: &Challenge) -> HubFrame {
    HubFrame::Challenge {
        hub_nonce: encoded(challenge.hub_nonce.as_bytes()),
        hub_key: encoded(challenge.hub_key.as_bytes()),
        hub_handshakes: challenge.hub_handshakes.bits(),
        hub_proof: encoded(challenge.hub_proof.as_bytes()),
    }
}

pub fn answer_frame(answer: &PairAnswer) -> HubFrame {
    match answer {
        PairAnswer::Paired(paired) => HubFrame::Paired {
            recovery_key: encoded(paired.recovery_key.as_bytes()),
            signature: encoded(paired.signature.as_bytes()),
            hostname: paired
                .hostname
                .as_ref()
                .map(|hostname| hostname.as_str().to_owned()),
            status: paired.approval.map(status_of),
        },
        PairAnswer::Refused(refused) => HubFrame::Refused {
            code: refused.code.as_str().to_owned(),
            holder: refused.holder.as_ref().map(|holder| HolderFrame {
                hostname: holder.hostname.as_str().to_owned(),
                last_seen: holder.last_seen.seconds(),
            }),
            signature: encoded(refused.signature.as_bytes()),
        },
    }
}

pub fn proof_frame(proof: &Proof) -> AgentFrame {
    AgentFrame::Proof {
        counter: proof.counter,
        agent_proof: encoded(proof.agent_proof.as_bytes()),
    }
}

pub fn agent_envelope_frame(envelope: &AgentEnvelope) -> AgentFrame {
    AgentFrame::Signed {
        kind: envelope.kind.as_str().to_owned(),
        seq: envelope.seq,
        payload: encoded(&envelope.payload),
        signature: encoded(envelope.signature.as_bytes()),
    }
}

pub fn hub_envelope_frame(envelope: &HubEnvelope) -> HubFrame {
    HubFrame::Signed {
        kind: envelope.kind.as_str().to_owned(),
        seq: envelope.seq,
        expires_at: envelope.expires_at.seconds(),
        payload: encoded(&envelope.payload),
        signature: encoded(envelope.signature.as_bytes()),
    }
}

/// The bytes of `hello`'s payload, which the agent signs.
pub fn hello_payload(hello: &Hello) -> Vec<u8> {
    HelloPayload {
        agent_version: hello.agent_version.clone(),
        hostname: hello.hostname.as_str().to_owned(),
        protocols: range(hello.protocols),
        clock: hello.clock.seconds(),
        zone: hello.zone.as_ref().map(|zone| zone.as_str().to_owned()),
        machine: hello.machine.as_ref().map(MachineFingerprint::to_hex),
    }
    .to_bytes()
}

/// The `Hello` a payload's bytes hold.
pub fn read_hello(bytes: &[u8]) -> Result<Hello, WireRefused> {
    let payload = HelloPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    Ok(Hello {
        agent_version: payload.agent_version,
        hostname: Hostname::parse(&payload.hostname).map_err(|_| WireRefused)?,
        protocols: protocols(payload.protocols)?,
        clock: EpochSeconds::from_seconds(payload.clock),
        zone: zone(payload.zone.as_deref())?,
        machine: payload
            .machine
            .as_deref()
            .map(|text| MachineFingerprint::from_hex(text).map_err(|_| WireRefused))
            .transpose()?,
    })
}

/// The bytes of `welcome`'s payload, which the hub signs.
pub fn welcome_payload(welcome: &Welcome) -> Vec<u8> {
    WelcomePayload {
        protocol: welcome.protocol,
        hub_protocols: range(welcome.hub_protocols),
        agent_protocols: range(welcome.agent_protocols),
        generation: welcome.generation,
        hub_handshakes: welcome.hub_handshakes.bits(),
        hub_version: welcome.hub_version.clone(),
        signed_at: welcome.signed_at.seconds(),
        status: status_of(welcome.approval),
    }
    .to_bytes()
}

/// The `Welcome` a payload's bytes hold.
pub fn read_welcome(bytes: &[u8]) -> Result<Welcome, WireRefused> {
    let payload = WelcomePayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    Ok(Welcome {
        protocol: payload.protocol,
        hub_protocols: protocols(payload.hub_protocols)?,
        agent_protocols: protocols(payload.agent_protocols)?,
        generation: payload.generation,
        hub_handshakes: Handshakes::from_bits(payload.hub_handshakes),
        hub_version: payload.hub_version,
        signed_at: EpochSeconds::from_seconds(payload.signed_at),
        approval: approval_of(payload.status),
    })
}

/// The bytes of a report's payload, which the agent signs: the epoch and
/// the operation as their UUIDs' text, what happened as `started`,
/// `succeeded` or `failed` with the code's wire name.
pub fn report_payload(entry: &OutboxEntry) -> Vec<u8> {
    let (event, code) = match &entry.report.happening {
        Happening::Started => ("started", None),
        Happening::Succeeded => ("succeeded", None),
        Happening::Failed { code } => ("failed", Some(code.as_str().to_owned())),
    };
    ReportPayload {
        epoch: entry.epoch.as_uuid().to_string(),
        seq: entry.seq.get(),
        op_id: entry.report.op_id.as_uuid().to_string(),
        event: event.to_owned(),
        at: entry.report.at.seconds(),
        code,
    }
    .to_bytes()
}

/// The report a payload's bytes hold: a place of 0 or past `i64::MAX`,
/// which the hub's store can't hold, an event this build doesn't know, a
/// failure without its code or a start or success with one refuses it.
pub fn read_report(bytes: &[u8]) -> Result<OutboxEntry, WireRefused> {
    let payload = ReportPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    let code = payload.code.as_deref().map(WireName::parse);
    let happening = match (payload.event.as_str(), code) {
        ("started", None) => Happening::Started,
        ("succeeded", None) => Happening::Succeeded,
        ("failed", Some(Ok(code))) => Happening::Failed { code },
        _ => return Err(WireRefused),
    };
    Ok(OutboxEntry {
        epoch: OutboxEpoch::from_uuid(uuid(&payload.epoch)?),
        seq: ReportSeq::new(payload.seq)
            .filter(|seq| i64::try_from(seq.get()).is_ok())
            .ok_or(WireRefused)?,
        report: AgentReport {
            op_id: OperationId::from_uuid(uuid(&payload.op_id)?),
            happening,
            at: EpochSeconds::from_seconds(payload.at),
        },
    })
}

/// The bytes of `ack`'s payload, which the hub signs.
pub fn ack_payload(ack: Ack) -> Vec<u8> {
    AckPayload {
        epoch: ack.epoch.as_uuid().to_string(),
        seq: ack.seq.get(),
    }
    .to_bytes()
}

/// The acknowledgement a payload's bytes hold.
pub fn read_ack(bytes: &[u8]) -> Result<Ack, WireRefused> {
    let payload = AckPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    Ok(Ack {
        epoch: OutboxEpoch::from_uuid(uuid(&payload.epoch)?),
        seq: ReportSeq::new(payload.seq).ok_or(WireRefused)?,
    })
}

/// The UUID `text` holds, in its hyphenated form only.
fn uuid(text: &str) -> Result<Uuid, WireRefused> {
    Uuid::try_parse(text).map_err(|_| WireRefused)
}

/// The most repositories one `ReposSnapshot` names, so its frame stays
/// within [`MAX_FRAME_BYTES`]: an agent that uses more reports the first by
/// ID, and is granted no lease on the others.
pub const MAX_REPORTED_REPOSITORIES: usize = 24;

/// The bytes of a `ReposSnapshot`'s payload, which the agent signs: at most
/// the first [`MAX_REPORTED_REPOSITORIES`] of `reports`.
pub fn repos_payload(reports: &RepositoryReports) -> Vec<u8> {
    ReposPayload {
        repos: reports
            .entries()
            .iter()
            .take(MAX_REPORTED_REPOSITORIES)
            .map(|report| RepoPayload {
                repo_id: report.repository.as_str().to_owned(),
                requested_maintenance: report.request.as_str().to_owned(),
                others_snapshots: report.holds_other_agents_snapshots,
            })
            .collect(),
    }
    .to_bytes()
}

/// The reports a `ReposSnapshot`'s bytes hold: a repository ID that isn't
/// one or a request this build doesn't know refuses it.
pub fn read_repos(bytes: &[u8]) -> Result<RepositoryReports, WireRefused> {
    let payload = ReposPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    let reports = payload
        .repos
        .iter()
        .map(|repo| {
            Ok(RepositoryReport {
                repository: RepositoryId::parse(&repo.repo_id).map_err(|_| WireRefused)?,
                request: Maintenance::parse(&repo.requested_maintenance)
                    .map_err(|_| WireRefused)?,
                holds_other_agents_snapshots: repo.others_snapshots,
            })
        })
        .collect::<Result<Vec<_>, WireRefused>>()?;
    Ok(RepositoryReports::new(reports))
}

/// The bytes of a `RepoGrants`'s payload, which the hub signs. Each expiry
/// is rounded down to its second and the signing time up, so the lease
/// the agent counts never runs longer than the hub's.
pub fn grants_payload(grants: &RepoGrants) -> Vec<u8> {
    GrantsPayload {
        generation: grants.generation,
        signed_at: seconds_up(grants.signed_at),
        grants: grants
            .grants
            .iter()
            .map(|grant| GrantPayload {
                repo_id: grant.repository.as_str().to_owned(),
                epoch: grant.epoch.get(),
                expires_at: seconds_down(grant.expires_at),
            })
            .collect(),
    }
    .to_bytes()
}

/// The grants a `RepoGrants`'s bytes hold: a repository ID that isn't one,
/// an epoch of 0, a generation or an epoch past `i64::MAX`, which the
/// agent's store can't hold, or a time past the last there is refuses them.
pub fn read_grants(bytes: &[u8]) -> Result<RepoGrants, WireRefused> {
    let payload = GrantsPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    let grants = payload
        .grants
        .iter()
        .map(|grant| {
            Ok(Grant {
                repository: RepositoryId::parse(&grant.repo_id).map_err(|_| WireRefused)?,
                epoch: lease_epoch(grant.epoch)?,
                expires_at: time(grant.expires_at)?,
            })
        })
        .collect::<Result<Vec<_>, WireRefused>>()?;
    Ok(RepoGrants {
        generation: storable(payload.generation)?,
        signed_at: time(payload.signed_at)?,
        grants,
    })
}

/// The bytes of a `ReleaseAck`'s payload, which the agent signs.
pub fn release_payload(repository: &RepositoryId, epoch: LeaseEpoch) -> Vec<u8> {
    ReleasePayload {
        repo_id: repository.as_str().to_owned(),
        epoch: epoch.get(),
    }
    .to_bytes()
}

/// The release a `ReleaseAck`'s bytes hold.
pub fn read_release(bytes: &[u8]) -> Result<(RepositoryId, LeaseEpoch), WireRefused> {
    let payload = ReleasePayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    Ok((
        RepositoryId::parse(&payload.repo_id).map_err(|_| WireRefused)?,
        lease_epoch(payload.epoch)?,
    ))
}

/// The bytes of a `Released`'s payload, which the hub signs.
pub fn released_payload(
    repository: &RepositoryId,
    epoch: LeaseEpoch,
    signed_at: Timestamp,
) -> Vec<u8> {
    ReleasedPayload {
        repo_id: repository.as_str().to_owned(),
        epoch: epoch.get(),
        signed_at: seconds_up(signed_at),
    }
    .to_bytes()
}

/// The release a `Released`'s bytes say the hub took.
pub fn read_released(bytes: &[u8]) -> Result<(RepositoryId, LeaseEpoch), WireRefused> {
    let payload = ReleasedPayload::from_bytes(bytes).map_err(|_| WireRefused)?;
    time(payload.signed_at)?;
    Ok((
        RepositoryId::parse(&payload.repo_id).map_err(|_| WireRefused)?,
        lease_epoch(payload.epoch)?,
    ))
}

/// `number` when the agent's store can hold it: up to `i64::MAX`.
fn storable(number: u64) -> Result<u64, WireRefused> {
    i64::try_from(number)
        .map(|_| number)
        .map_err(|_| WireRefused)
}

fn lease_epoch(number: u64) -> Result<LeaseEpoch, WireRefused> {
    LeaseEpoch::new(storable(number)?).ok_or(WireRefused)
}

/// The time `seconds` after the Unix epoch.
fn time(seconds: u64) -> Result<Timestamp, WireRefused> {
    let seconds = i64::try_from(seconds).map_err(|_| WireRefused)?;
    Timestamp::from_second(seconds).map_err(|_| WireRefused)
}

/// `time`'s whole seconds since the Unix epoch, rounded down; 0 before it.
fn seconds_down(time: Timestamp) -> u64 {
    u64::try_from(time.as_second()).unwrap_or(0)
}

/// `time`'s seconds since the Unix epoch, rounded up; 0 before it.
fn seconds_up(time: Timestamp) -> u64 {
    let part = u64::from(time.subsec_nanosecond() > 0);
    seconds_down(time).saturating_add(part)
}

/// How `Welcome` and `Paired` say an agent's approval on the wire.
const fn status_of(approval: Approval) -> AgentApproval {
    match approval {
        Approval::Pending => AgentApproval::Pending,
        Approval::Approved => AgentApproval::Approved,
    }
}

const fn approval_of(status: AgentApproval) -> Approval {
    match status {
        AgentApproval::Pending => Approval::Pending,
        AgentApproval::Approved => Approval::Approved,
    }
}

/// The message an agent's frame holds.
pub fn agent_message(frame: &AgentFrame) -> Result<AgentMessage, WireRefused> {
    match frame {
        AgentFrame::Connect {
            agent_id,
            agent_nonce,
            binding_kind,
        } => Ok(AgentMessage::Connect(Connect {
            agent_id: AgentId::from_uuid(Uuid::try_parse(agent_id).map_err(|_| WireRefused)?),
            agent_nonce: Nonce::from_bytes(decoded(agent_nonce)?),
            binding_kind: BindingKind::from_byte(*binding_kind).ok_or(WireRefused)?,
        })),
        AgentFrame::Pair {
            pairing_key,
            agent_key,
            counter,
            hostname,
            zone: sent_zone,
            pairing_signature,
            agent_signature,
        } => Ok(AgentMessage::Pair(Pair {
            pairing_key: key(pairing_key)?,
            agent_key: key(agent_key)?,
            counter: *counter,
            hostname: Hostname::parse(hostname).map_err(|_| WireRefused)?,
            zone: zone(sent_zone.as_deref())?,
            pairing_signature: signature(pairing_signature)?,
            agent_signature: signature(agent_signature)?,
        })),
        AgentFrame::Proof {
            counter,
            agent_proof,
        } => Ok(AgentMessage::Proof(Proof {
            counter: *counter,
            agent_proof: signature(agent_proof)?,
        })),
        AgentFrame::Signed {
            kind,
            seq,
            payload,
            signature: signed,
        } => Ok(AgentMessage::Signed(AgentEnvelope {
            kind: WireName::parse(kind).map_err(|_| WireRefused)?,
            seq: *seq,
            payload: payload_bytes(payload)?,
            signature: signature(signed)?,
        })),
    }
}

/// The message a hub's frame holds.
pub fn hub_message(frame: &HubFrame) -> Result<HubMessage, WireRefused> {
    match frame {
        HubFrame::Challenge {
            hub_nonce,
            hub_key,
            hub_handshakes,
            hub_proof,
        } => Ok(HubMessage::Challenge(Challenge {
            hub_nonce: Nonce::from_bytes(decoded(hub_nonce)?),
            hub_key: key(hub_key)?,
            hub_handshakes: Handshakes::from_bits(*hub_handshakes),
            hub_proof: signature(hub_proof)?,
        })),
        HubFrame::Paired {
            recovery_key,
            signature: signed,
            hostname,
            status,
        } => Ok(HubMessage::Answer(PairAnswer::Paired(Paired {
            recovery_key: key(recovery_key)?,
            signature: signature(signed)?,
            hostname: match hostname {
                Some(hostname) => Some(Hostname::parse(hostname).map_err(|_| WireRefused)?),
                None => None,
            },
            approval: status.map(approval_of),
        }))),
        HubFrame::Refused {
            code,
            holder,
            signature: signed,
        } => Ok(HubMessage::Answer(PairAnswer::Refused(Refused {
            code: WireName::parse(code).map_err(|_| WireRefused)?,
            holder: match holder {
                Some(holder) => Some(Holder {
                    hostname: Hostname::parse(&holder.hostname).map_err(|_| WireRefused)?,
                    last_seen: EpochSeconds::from_seconds(holder.last_seen),
                }),
                None => None,
            },
            signature: signature(signed)?,
        }))),
        HubFrame::Signed {
            kind,
            seq,
            expires_at,
            payload,
            signature: signed,
        } => Ok(HubMessage::Signed(HubEnvelope {
            kind: WireName::parse(kind).map_err(|_| WireRefused)?,
            seq: *seq,
            expires_at: EpochSeconds::from_seconds(*expires_at),
            payload: payload_bytes(payload)?,
            signature: signature(signed)?,
        })),
    }
}

fn encoded(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The bytes a signed message's payload encodes, strictly, as [`decoded`]
/// reads a field. A payload longer than a frame may be is refused unread.
fn payload_bytes(text: &str) -> Result<Vec<u8>, WireRefused> {
    if text.len() > MAX_FRAME_BYTES {
        return Err(WireRefused);
    }
    URL_SAFE_NO_PAD.decode(text).map_err(|_| WireRefused)
}

const fn range(protocols: Protocols) -> [u64; 2] {
    [protocols.lowest(), protocols.highest()]
}

fn protocols([lowest, highest]: [u64; 2]) -> Result<Protocols, WireRefused> {
    Protocols::new(lowest, highest).ok_or(WireRefused)
}

/// The `N` bytes `text` encodes, strictly: no padding, no other alphabet,
/// no other length, no stray bits. The text is measured before it is
/// decoded, so a long field is refused unread.
fn decoded<const N: usize>(text: &str) -> Result<[u8; N], WireRefused> {
    if text.len() != (N * 4).div_ceil(3) {
        return Err(WireRefused);
    }
    // Room for the largest field and the library's estimate beyond it.
    let mut buffer = [0; SIGNATURE_BYTES + 3];
    match URL_SAFE_NO_PAD.decode_slice(text, &mut buffer) {
        Ok(length) if length == N => buffer
            .get(..N)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(WireRefused),
        _ => Err(WireRefused),
    }
}

fn key(text: &str) -> Result<PublicKey, WireRefused> {
    PublicKey::from_bytes(decoded::<KEY_BYTES>(text)?).map_err(|_| WireRefused)
}

fn signature(text: &str) -> Result<Signature, WireRefused> {
    Signature::from_bytes(decoded::<SIGNATURE_BYTES>(text)?).map_err(|_| WireRefused)
}

/// A machine zone, when one was sent: a name not shaped like one refuses
/// the frame. Whether the hub's zone database knows it is for the hub.
fn zone(name: Option<&str>) -> Result<Option<MachineZone>, WireRefused> {
    name.map(|name| MachineZone::parse(name).map_err(|_| WireRefused))
        .transpose()
}

#[cfg(test)]
mod tests;
