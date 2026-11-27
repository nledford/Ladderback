//! Rows of the hub's registry, `pairing_tokens` and `agents` (§16, decision
//! 120). A key, an ID, a kind, a counter or a machine zone this version
//! couldn't have written is [`StoreError::Corrupt`], never a record that
//! lets anyone pair or prove more than it was given.

use app::ports::StoreError;
use domain::fleet::{
    AgentRecord, AgentStatus, BindingKind, Hostname, KEY_BYTES, MachineFingerprint, MachineZone,
    PublicKey, SecondMachine, TokenRecord, TokenUse, Tombstone,
};
use domain::{AgentId, Uuid};
use sqlx::sqlite::SqliteRow;

use super::Reader;
use crate::report::shown;

/// How many digits the `counter` column keeps: the most a `u64` has.
pub(crate) const COUNTER_DIGITS: usize = 20;

/// A counter as the `counter` column keeps it: zero-padded, so the column
/// compares in order as text.
pub(crate) fn counter_text(counter: u64) -> String {
    format!("{counter:0COUNTER_DIGITS$}")
}

/// An agent ID as the store writes it: lower-case and hyphenated.
pub(crate) fn agent_id_text(id: AgentId) -> String {
    let mut buffer = Uuid::encode_buffer();
    id.as_uuid()
        .hyphenated()
        .encode_lower(&mut buffer)
        .to_owned()
}

impl Reader<'_> {
    fn key(&self, column: &str) -> Result<Option<PublicKey>, StoreError> {
        let Some(bytes) = self.blob(column)? else {
            return Ok(None);
        };
        let bytes = <[u8; KEY_BYTES]>::try_from(bytes.as_slice()).map_err(|_| {
            self.corrupt(column, format!("is {} bytes, not {KEY_BYTES}", bytes.len()))
        })?;
        PublicKey::from_bytes(bytes)
            .map(Some)
            .map_err(|_| self.corrupt(column, "is not a key a pairing accepts"))
    }

    fn required_key(&self, column: &str) -> Result<PublicKey, StoreError> {
        self.key(column)?
            .ok_or_else(|| self.corrupt(column, "is NULL"))
    }

    /// An agent ID, which must be the exact text the store writes: a row
    /// under another spelling of it could never be found by its ID.
    fn agent_id(&self, column: &str) -> Result<Option<AgentId>, StoreError> {
        let Some(text) = self.text(column)? else {
            return Ok(None);
        };
        let id = Uuid::parse_str(&text)
            .map(AgentId::from_uuid)
            .map_err(|_| self.corrupt(column, format!("{} is not an agent ID", shown(&text))))?;
        if agent_id_text(id) != text {
            return Err(self.corrupt(column, "is not in lowercase hyphenated form"));
        }
        Ok(Some(id))
    }

    /// The `hostname` column, which must be a hostname a pairing accepts.
    fn hostname(&self) -> Result<Hostname, StoreError> {
        let text = self.required_text("hostname")?;
        Hostname::parse(&text)
            .map_err(|_| self.corrupt("hostname", format!("{} is not a hostname", shown(&text))))
    }
}

/// A row of `pairing_tokens`.
pub(crate) fn pairing_token(row: &SqliteRow) -> Result<TokenRecord, StoreError> {
    let mut reader = Reader {
        row,
        scope: "a pairing token".to_owned(),
    };
    let pairing_key = reader.required_key("pairing_key")?;
    reader.scope = format!("pairing token {}", pairing_key.to_hex());
    let kind = reader
        .integer("kind")?
        .ok_or_else(|| reader.corrupt("kind", "is NULL"))?;
    let kind = u8::try_from(kind)
        .ok()
        .and_then(BindingKind::from_byte)
        .ok_or_else(|| reader.corrupt("kind", format!("{kind} is not a binding kind")))?;
    let state_text = reader.required_text("state")?;
    let agent_id = reader.agent_id("used_machine_id")?;
    let agent_key = reader.key("used_machine_key")?;
    let state = match (state_text.as_str(), agent_id, agent_key) {
        ("unused", None, None) => TokenUse::Unused,
        ("revoked", None, None) => TokenUse::Revoked,
        ("used", Some(agent_id), Some(agent_key)) => TokenUse::Used {
            agent_id,
            agent_key,
        },
        ("unused" | "revoked" | "used", _, _) => {
            return Err(reader.corrupt(
                "state",
                format!(
                    "{} doesn't agree with used_machine_id and used_machine_key",
                    shown(&state_text)
                ),
            ));
        }
        _ => {
            return Err(reader.corrupt(
                "state",
                format!("{} is not a token's state", shown(&state_text)),
            ));
        }
    };
    Ok(TokenRecord {
        pairing_key,
        hub_key: reader.required_key("hub_key")?,
        kind,
        made_at: reader.required_time("made_at")?,
        expires_at: reader.required_time("expires_at")?,
        state,
    })
}

/// A row of `agent_tombstones`.
pub(crate) fn tombstone(row: &SqliteRow) -> Result<Tombstone, StoreError> {
    let mut reader = Reader {
        row,
        scope: "a removed agent".to_owned(),
    };
    let agent_id = reader
        .agent_id("id")?
        .ok_or_else(|| reader.corrupt("id", "is NULL"))?;
    reader.scope = format!("removed agent {}", agent_id_text(agent_id));
    Ok(Tombstone {
        agent_id,
        hostname: reader.hostname()?,
        key: reader.required_key("pubkey")?,
        removed_at: reader.required_time("removed_at")?,
    })
}

/// A row of `agents`.
pub(crate) fn agent(row: &SqliteRow) -> Result<AgentRecord, StoreError> {
    let mut reader = Reader {
        row,
        scope: "an agent".to_owned(),
    };
    let agent_id = reader
        .agent_id("id")?
        .ok_or_else(|| reader.corrupt("id", "is NULL"))?;
    reader.scope = format!("agent {}", agent_id_text(agent_id));
    let hostname = reader.hostname()?;
    let status_at = reader.required_time("status_at")?;
    let status_text = reader.required_text("status")?;
    let status = match status_text.as_str() {
        "pending" => AgentStatus::Pending { since: status_at },
        "approved" => AgentStatus::Approved { at: status_at },
        _ => {
            return Err(reader.corrupt(
                "status",
                format!("{} is not an agent's status", shown(&status_text)),
            ));
        }
    };
    let counter_text = reader.required_text("counter")?;
    let counter = (counter_text.len() == COUNTER_DIGITS
        && counter_text.bytes().all(|byte| byte.is_ascii_digit()))
    .then(|| counter_text.parse::<u64>().ok())
    .flatten()
    .ok_or_else(|| {
        reader.corrupt(
            "counter",
            format!("{} is not a counter", shown(&counter_text)),
        )
    })?;
    let zone = reader
        .text("zone")?
        .map(|text| {
            MachineZone::parse(&text)
                .map_err(|_| reader.corrupt("zone", format!("{} is not a zone name", shown(&text))))
        })
        .transpose()?;
    Ok(AgentRecord {
        agent_id,
        key: reader.required_key("pubkey")?,
        hostname,
        status,
        counter,
        last_seen: reader.required_time("last_seen")?,
        zone,
        version: None,
    })
}

/// The `machine` column of a row of `agents`: the fingerprint the hub took
/// under the agent's ID, or none (decision 150).
pub(crate) fn held_machine(row: &SqliteRow) -> Result<Option<MachineFingerprint>, StoreError> {
    let reader = Reader {
        row,
        scope: "an agent's machine".to_owned(),
    };
    reader
        .text("machine")?
        .map(|text| {
            MachineFingerprint::from_hex(&text).map_err(|_| {
                reader.corrupt(
                    "machine",
                    format!("{} is not a machine fingerprint", shown(&text)),
                )
            })
        })
        .transpose()
}

/// A row of `agents` under whose ID a second computer was refused: its ID
/// and `second_machine_at`.
pub(crate) fn second_machine(row: &SqliteRow) -> Result<SecondMachine, StoreError> {
    let mut reader = Reader {
        row,
        scope: "an agent".to_owned(),
    };
    let agent_id = reader
        .agent_id("id")?
        .ok_or_else(|| reader.corrupt("id", "is NULL"))?;
    reader.scope = format!("agent {}", agent_id_text(agent_id));
    Ok(SecondMachine {
        agent_id,
        at: reader.required_time("second_machine_at")?,
    })
}
