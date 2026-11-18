# Decision 120: How an agent and its hub prove who they are

**At connect, the hub and the agent each prove their identity with an Ed25519 signature over a domain-separated message: a fixed label, the hub's key, the agent ID, both sides' fresh nonces and, when the hub serves TLS itself, the TLS 1.3 session's exporter value (RFC 9266). The hub has a key pair of its own that every agent pins at pairing, carried in the pairing token beside the certificate's fingerprint. Every hub message that gives an agent authority or makes it act is signed with that key, for one agent and one connection, with a sequence number and an expiry. Pairing uses a one-time key the token carries. Mutual TLS, a required tunnel and peer-to-peer are not needed** (recorded 2026-11-18, §3.1, §7.1, §7.2, §10.3; #567, items 1–4 of the maintainer's comment of 2026-11-14).

The decision also fixes the order of the handshake, which decision 88 left open. The maintainer pre-approved security crates on 2026-11-18; "Crates" names them.

## Where each item of the maintainer's comment is answered

| Item of the comment on #567 | Answered in |
|---|---|
| 1. What the agent signs; the binding to TLS in built-in mode and why not behind a proxy | "The signed messages", "Binding to the TLS session" |
| 2. Why not mutual TLS, a required tunnel or peer-to-peer; running over a tunnel stays supported | "Why not the alternatives" |
| 3. Tests: the bare nonce, another hub, another agent ID | "Tests that pin it", first bullet |
| 4. The hub key pair: pinned at pairing, both sides prove themselves, signed commands, no second encryption, backup and rotation | "The hub key", "Signed hub messages", "Why not the alternatives", "Tests that pin it" |

The decision also settles the pairing token, a hostname already taken, an agent re-paired after losing its data folder and #296 ("Pairing"), and the crates ("Crates").

## The handshake

An agent opens `/sync/v1`, a WebSocket over TLS. Until both proofs are checked, each side sends only what the diagram shows: nothing of the agent's plans, repositories or hostname reaches a hub the agent hasn't verified, and nothing of the hub's reaches a stranger.

```
agent                                      hub
  | Connect { agent_id, agent_nonce }       |
  |---------------------------------------->|
  |   Challenge { hub_nonce, hub_key,       |
  |               hub_proof, handovers }    |
  |<----------------------------------------|
  | (checks the hub key and hub_proof)      |
  | Proof { counter, agent_proof }          |
  |---------------------------------------->|
  | Hello { ... }                           |
  |---------------------------------------->|
  |   Welcome, signed (§7.2)                |
  |<----------------------------------------|
```

- **The agent proves nothing until the hub has.** The key in `Challenge` must be the agent's pinned hub key, or reach it through the hub's handovers ("The hub key"), and `hub_proof` must verify. Otherwise the agent closes, sends no `Proof` and no `Hello`, and says `hub-key-mismatch` or `hub-proof-invalid`.
- **The hub answers every `Connect`.** An agent ID it doesn't know gets a `Challenge` like any other, so a stranger can't list the registry. The refusal comes at `Proof`, one code (`proof-refused`) for an unknown agent, a deleted one and a wrong signature.
- **Nonces.** Each is 32 bytes from the `Random` port, fresh for each connection attempt and good for that connection only. The hub forgets its nonce when the proof is checked or after 15 s.
- **Bounds before the proofs.** A frame over 4 KiB, a handshake over 15 s and more than 32 unauthenticated connections at once are refused (§13.5). A `Hello` before a good `Proof` is a protocol error.
- **The order is new.** §7.1 had the hub send a nonce first. The hub can only sign the agent's nonce if the agent speaks first. No hub has shipped, so nothing is broken. From here decision 88's rule holds: the handshake is the same in every protocol, so two releases can always begin it. Its label and path name it and never change in place; a different handshake would be a new path and a decision, never a field in the connection, so nothing in it can be lowered.

## The signed messages

Every signature is Ed25519 (RFC 8032) over bytes built one way, by a pure function in `domain` that the tests pin with fixed vectors.

- **Encoding.** One byte for the label's length, the label in ASCII, then the fields in the order below, with no separator. Integers are big-endian unsigned 64-bit; a time is seconds since the Unix epoch. A key is its 32 raw bytes, an agent ID the 16 bytes of its UUID, a nonce 32 bytes. Only a hostname and a message kind are variable: each is one length byte, then UTF-8. Every other field has a fixed width, so no two messages with different labels or fields encode to the same bytes.
- **The binding field** is 33 bytes: a kind byte (0, none, or 1, `tls-exporter`) and 32 bytes that are zero for kind 0.
- **Every label starts `ladderback-sync-v1 `.** The `v1` names the handshake, as `/sync/v1` does, not a protocol of the range `Hello` carries. The rest names the message, so a signature made for one message never verifies as another.

| Message | Signed by | Label ends | Fields, in order |
|---|---|---|---|
| Hub proof | the hub key | `hub-proof` | hub key, agent ID, agent nonce, hub nonce, binding |
| Agent proof | the agent key | `agent-proof` | hub key, agent ID, agent key, agent nonce, hub nonce, binding, counter |
| Pairing | the token's pairing key and the agent key, two signatures over the same bytes | `pair` | hub key, agent ID, agent key, agent nonce, hub nonce, binding, counter, hostname |
| Signed hub message | the hub key | `signed` | hub key, agent ID, agent nonce, hub nonce, sequence, expires at, kind, SHA-256 of the payload |
| Handover | the old hub key | `handover` | old key, new key, made at |

Why each field is there:

- **The label** keeps the proofs, the pairing, a signed message and a handover from standing in for one another, and keeps any of them from being a signature over something else the key signs. A signature over the bare nonce verifies as none of them.
- **The hub's key** is the hub's identity in every message, in both TLS modes ("The hub key"). A signature made for another hub is refused, because that hub's key is another value.
- **The agent ID and the two nonces** tie a signature to one agent and one connection. A recorded proof or command can't be replayed on another connection, and a hub's signature for one agent is no good to another.
- **Both nonces in both proofs.** Each side signs a value the other side drew, so neither can have a signature made in advance, and the hub's signing for a stranger's nonce gives the stranger nothing: the hub's own fresh nonce is in what it signs.
- **Reflection.** The two proofs differ in label, in signing key and in field order, so a hub proof presented back to the hub as an agent proof, or the reverse, verifies as neither.
- **The counter** is the agent's connection counter ("A copied data folder").

## Binding to the TLS session

**Built-in TLS.** The proofs and every signed hub message carry the connection's `tls-exporter` value (RFC 9266): the TLS 1.3 exporter with the label `EXPORTER-Channel-Binding`, an empty context and 32 bytes. Each side computes it from its own end of the session (rustls `export_keying_material`), so the signatures hold only on the TLS session they were made for. The listener accepts TLS 1.3 only, which keeps the exporter free of RFC 7627's conditions.

What the binding adds to the pinned certificate: a party that holds the certificate's private key (a copy in a backup, say) but neither Ed25519 key can terminate an agent's TLS and relay to the hub, and each side's signature then covers a different session, so both refuse. And the hub's identity no longer rests on the agent's pinning code being right.

**Behind a TLS-terminating proxy** there is no shared session. The agent's TLS ends at the proxy; the hub sees another connection, so the two ends compute different values and every honest connection would fail. The binding is kind 0, and the hub's key carries the whole of the hub's identity.

- **The mode is never negotiated.** An agent paired with a pinned certificate always signs kind 1 and accepts nothing else; an agent paired with a CA certificate always signs kind 0. A hub in built-in mode requires kind 1, a hub behind a proxy requires kind 0. A mismatch is a signature that doesn't verify, never a quiet fallback.
- **What a proxy can still do.** It can relay a handshake it didn't start. What it can't do: act as the hub to an agent without the real hub (no hub proof), act as the agent to the hub without the agent, or make an agent act (the signed messages). It can read, drop and alter unsigned messages ("What this doesn't defend").
- **A plain `http://` address is refused at pairing** unless it is loopback, as the MCP token's address is (§10.5).

## The hub key

- **Made once.** The hub makes an Ed25519 key pair the first time it starts as a hub with no registered agent and no recorded key. The seed is 32 bytes from the `Random` port, which also lets a test seed it. It is not the hub's own agent key: the hub is an agent as well, and a signature made as an agent must never be one made as the hub.
- **Kept in the data folder, never the database**, as the agent's key is: a file `hub-key` of one line, the seed as 64 lower-case hex digits, mode 0600, written whole under a private name and linked into place, as the agent ID file is. `db rebuild` can't change it. Other operating-system users can't read it; a program running as the person can, which decision 99 accepts. In memory the seed is wiped when dropped.
- **Pinned at pairing.** The token carries the public key, so each agent holds it before it sends anything. It is shown in Settings as its SHA-256 in groups of four hex digits, for a person to compare with the agent's pairing page.
- **The hub never makes a replacement by itself.** The database records the key's fingerprint when the key is made. A hub that starts with the file missing or another key refuses `/sync/v1`, says so on its pages and in its log, and offers the two ways out: restore the file, or reset the key, which names every agent that will need to pair again. The same holds for the certificate in built-in mode.
- **Backup.** Copy the file. It is one line in the data folder a person backing up the hub already includes, so there is no export command; Settings shows its path and fingerprint, so a restored key can be checked. Anyone holding the file can pose as the hub to every agent, so it is kept like a password.
- **Rotation is a handover the old key signs.** The hub makes a new key, signs `old key, new key, time` with the old one, stores that handover (public, beside the key) and replaces the key, deleting the old seed. At connect it sends its handovers in `Challenge`, at most 8, oldest first. An agent whose pin is the `old key` of a link in the chain follows it, checks each signature with the key before, and keeps the new key as its pin once the whole handshake, `Welcome` included, has succeeded. A chain that doesn't reach the agent's pin is `hub-key-mismatch`. An agent that missed more than 8 rotations pairs again.
- **What losing it costs.** Every agent refuses the hub (`hub-key-mismatch`), and each pairs again with a new token on the person's confirmation. Agents keep backing up and, as with a version mismatch (decision 88), their leases lapse in at most 7 days, after which they run no scheduled maintenance. The hub's registry survives, and an agent that pairs again with its own ID and key keeps its record.
- **A leaked key can't be recovered by rotating.** Whoever holds the old key can sign a handover to a key of their own. The remedy is a reset, then pairing every agent again.

## Signed hub messages

A hub message is signed when it gives an agent authority or makes it act: `Welcome`, `RepoGrants` and, from M9, `RunPlan`, `CancelOp` and `RestoreRequest`, and `UpdatePlan` the day it ships. The agent obeys none of them unsigned. Heartbeat replies, `Ack`, `FetchLog` and the other status traffic are not signed, and payloads are not encrypted a second time: the link carries no secrets (decision 3, I4).

- **The envelope** holds `kind`, `seq`, `expires_at`, the `payload` as the base64url of its JSON bytes and the `signature`. The agent builds the signed message from the envelope, its own agent ID, its pinned hub key and the two nonces of this connection, so a message made for another agent, hub or connection fails to verify. It parses the payload only after the signature holds, from the same bytes the digest covered, so nothing is canonicalized.
- **Sequence.** The hub counts from 1 on each connection; the agent refuses any `seq` not above the last it accepted on this connection. A recorded message can't be played again on its connection, and the nonces keep it from another. Gaps are allowed. The agent records `seq` before it acts.
- **Expiry.** `expires_at` is at most 10 minutes after the hub signs. The agent refuses a message whose time has passed by its clock, or that expires more than 15 minutes ahead, so a hub bug can't mint a message that never lapses. The hub signs when it sends, never when it queues, so a command waiting for an offline agent gets a fresh signature and a fresh sequence on connect.
- **Order of checks.** Signature first, with nothing recorded for a message that fails; then sequence; then expiry; then the agent acts. A refusal is a code in the log and a closed connection for a forged or replayed message.
- **`Welcome` carries `agent_protocols`,** the range the hub saw in `Hello`. The agent refuses a `Welcome` whose echo isn't the range it sent, or whose `protocol` isn't the highest both ranges share. A proxy that lowers either side's range, to push the pair onto an older protocol (decision 88), is caught.
- **`RepoGrants` keep their own `expires_at`,** the lease's (§7.3). The message's expiry is how long it may be accepted, not how long the lease runs.
- **Decisions 2 and 12 stand.** The hub still sends only the request and the agent runs it, a restore only into a new folder and only where `allow-hub-restore` is set, and `UpdatePlan` stays reserved. The signature is one more condition on these, never a way round them.

## Pairing

- **The token** is `ladderback_pair_` and the unpadded base64url of 102 bytes: a version byte (1), the hub key (32), a TLS byte (0 CA, 1 pinned), the certificate's SHA-256 fingerprint (32, zeros for CA), the seed of a one-time pairing key (32) and the first 4 bytes of the SHA-256 of all before them. The checksum only catches a mistyped token before any connection is made; it is no security. The prefix lets a secret scanner find a leaked token.
- **The hub keeps only the pairing key's public half,** with its expiry and whether it was used, so a stolen database holds no usable token. The token is shown once. Its life is 15 minutes by default, 5 minutes to 24 hours on request, and at most 20 are unused at once. It is spent only by a pairing that succeeds.
- **Pairing is the handshake, with a different last step.** The agent starts with `Connect` and checks the hub's proof against the key in the token, so the token's seed never leaves an agent that hasn't verified the hub, and the seed itself never crosses the wire at all. It sends `Pair`, naming the public half of the pairing key so the hub finds the token, with the pairing message signed twice: by the pairing key, which is the hub's authorization, and by the agent key, which proves it holds the key it registers. The hub answers with a signed `Paired`.
- **A hostname taken by another agent** is refused with `hostname-taken`, naming the holder and when it was last seen. Names compare ignoring case, as DNS names do. The token isn't spent, so the person changes the agent's hostname, or removes the old agent on the hub, and pairs again with the same token.
- **Pairing again as the same identity** (the same ID and key) reuses its record, and sets the hub's stored counter to the agent's. An agent ID that arrives with another key is refused (`agent-key-mismatch`): a token authorizes a new agent, never taking over a registered one. An agent that pairs with a token whose hub key isn't the one it has pinned asks its person to confirm, naming both fingerprints, before it replaces the pin.
- **An agent that lost its data folder** has no ID or key to show, so it pairs as a new agent. Its old record still holds the hostname: the hub says which agent that is, and the person removes it, which orphans its snapshots for the adoption of §7.3, then pairs with the token that is still unspent.
- **A stolen unused token** lets one rogue agent register under a free hostname until the token lapses or is used. The real agent then sees `token-used`. The rogue agent is a record in the hub's list that anyone can remove; it can't take over a registered agent, and it is sent nothing but its own grants, which it gets only for a repository no other agent uses (§7.3). Having a person approve each new agent on the hub would remove even that, at the cost of a step in every pairing; not chosen, and the maintainer can revisit it.
- **A copied data folder (#296).** Two machines with one ID and one key can both prove themselves. The agent's own check is first: the machine identifier stored beside the ID, checked at start (the maintainer's decision on #296), which stops maintenance on the copy. The hub adds a second: each agent keeps a connection counter in its data folder and writes it, durably, before each proof; the hub takes only a counter above the one it holds. A copy and its original soon disagree, and the one behind is refused `identity-conflict`, after its signature verifies.
- **What the hub does on a conflict.** It raises an alert naming the agent, stops granting and renewing leases for that agent ID, and keeps its other state. The person resolves it by pairing the real machine again, which resets the counter; the copy takes a new ID ("this is a new machine", #296), or stays refused. An agent told `identity-conflict` keeps backing up and runs no maintenance, as #296 decided.
- **What this can't catch.** A disk image cloned with its machine identifier, whose original is switched off while the copy connects first, looks like one machine until the original returns; both then hold a lease for up to 7 days. The prune guard's other layers (§7.3, I3) and rustic's repository lock still apply. A data folder restored from a backup looks the same and gets the same remedy: pair again.

## Why not the alternatives

| Alternative | What it costs | Why not |
|---|---|---|
| **Mutual TLS** with a client certificate per agent | A certificate authority on the hub, issuing and renewing a certificate per agent. A reverse proxy ends TLS and passes the certificate on only as a header, which §10.1 never trusts (forwarded headers are never read). | The design must work behind a proxy, so the hub still needs an application-level identity. A second system for the first |
| **A required WireGuard or Tailscale tunnel** | A kernel module or a daemon on every machine, administrator rights, key distribution (or an account at a service) for each one, and an address that is no identity. | The hub still has to know which agent is which inside the tunnel, so it still needs this decision, and a required tunnel keeps out everyone who can't install one |
| **Peer-to-peer** | NAT traversal, discovery and relays, so that agents can reach each other. | Agents never talk to one another: one hub is the registry and the one arbiter of maintenance (I1). The cost buys nothing |
| **The bare nonce, signed** (§7.1's first draft) | Nothing, and so a signature that can be made for another hub, another agent or another message. | A signature over a bare nonce isn't tied to anything: it is the hand-rolled piece the maintainer asked to see closed |
| **A token format with its own signing** (JWT, PASETO) | A parser with a history of algorithm confusion, a JSON canonical form, and a crate for it. | The payload is signed as the bytes sent, with one algorithm and no field naming it |
| **One key for the TLS certificate and the hub** | Behind a proxy the certificate isn't the hub's, so the hub key must exist anyway; and the TLS key is read by the TLS library. | One more reason for the key to leak, with no gain |
| **RFC 7250 raw public keys in place of a certificate** | rustls supports them, which would drop the certificate generator. | A browser can't use one, and the built-in listener also serves the web UI |
| **Signing every agent → hub message** | A session key from an authenticated key exchange (X25519), which has a key schedule to design and review. | The proxy is the operator's own, the traffic carries no authority and no secrets, and the maintainer scoped it out. Recorded in "What this doesn't defend" |

**Running Ladderback over an existing tunnel stays supported.** Nothing in the design depends on the network. Over a tunnel the hub still serves built-in TLS, pinned, so nothing changes; the plain `http://` refusal above applies to a tunnel address as to any other.

## What this doesn't defend

- **Unsigned traffic through a hostile proxy.** A TLS-terminating proxy the operator doesn't trust can read and change what agents report (status, events, plan summaries, logs they send) and what the hub answers with status. It can't make an agent prune, restore, run a plan or accept a lease, and it can't forge an agent or the hub. Run built-in TLS, or a tunnel, if the proxy isn't yours.
- **A leaked hub key,** or a leaked agent key, until it is replaced by a reset or the agent is removed.
- **A person's own account.** A program running as the person reads both keys (decision 99).
- **The clock.** A hub or agent clock far off makes signed messages expire or look too far ahead; `Hello` already warns above 5 minutes (§7.2).

## Crates

The maintainer pre-approved security crates on 2026-11-18, to be chosen for review and track record, not for being in the tree. None is added by this decision; the implementation PR (#567) adds them, with `just deny` run on the result.

| Crate | Version | Used for | Alternative, and why not |
|---|---|---|---|
| `ed25519-dalek` | 3.0.0 (default features `fast` and `zeroize`; no `rand_core`, `serde`, `pem` or `batch`). Brings `curve25519-dalek` 5 and `ed25519` 3; `sha2` 0.11 and `subtle` are in the tree | Signing and verifying with `verify_strict`, which refuses non-canonical signatures and small-order keys, and constant-time arithmetic. The signing key wipes itself on drop. The seed comes from the `Random` port. `technology.md` §8.2 already names it | `ring` or `aws-lc-rs`, both in the tree and sound, but picked for TLS, not as this project's signature library, and their keys' wiping isn't what the maintainer asked for by name. `ed25519-dalek` 2.2.0 has the longer record in use; take it if the maintainer prefers that line to 3.0 |
| `rustls` | 0.23 (a direct dependency; the provider the tree already has) | TLS 1.3 only, the pinned-certificate verifier, the exporter value | Tungstenite's TLS features hide the connection, so the exporter isn't reachable |
| `tokio-rustls` | 0.26 | The server and the client over `tokio`, with the connection's exporter in reach | `axum-server`, a new package, with less control over the connection |
| `rustls-platform-verifier` | 0.7 (in the tree through `reqwest`) | The agent's CA validation of a proxy's certificate, with the operating system's trust store, as `reqwest` does | `webpki-roots` (a fixed list, no private CA) or `rustls-native-certs` |
| `rcgen` | 0.14 (`default-features = false`, feature `aws_lc_rs`; no `pem` or `x509-parser`). Brings `yasna`; `time` and `rustls-pki-types` are in the tree | Making the built-in listener's self-signed certificate once (ECDSA P-256, which every browser takes). Maintained by the rustls organization | Writing the ASN.1 by hand, `openssl` the program, or the person bringing a certificate (that is the proxy mode) |
| `tokio-tungstenite` | the version `axum`'s `ws` uses (0.29), no TLS feature | The agent's WebSocket client over a stream `tokio-rustls` made; the workspace's 0.26 serves tests today | Keeping 0.26 leaves two copies of the crate in a normal build |

Already in the tree and used as they are: `sha2` (the payload digest, the certificate fingerprint), `base64`, `uuid` (v7), `axum` (`ws`) and `zeroize`. No HMAC crate is needed: the token's key signs, so no secret is stored on the hub or sent. The implementation PR extends `technology.md` §8.2's crypto row to name the crates above.

## Tests that pin it

Each is a test that fails before the behaviour exists (§13.1).

- **The messages.** A signature over the bare nonce, hub's or agent's, is refused. A proof made for another hub key or another agent ID is refused. A signature of one label presented as another, the hub proof as the agent proof and the reverse, is refused. A proof from an earlier connection (other nonces) is refused. Another binding value or kind is refused. The encoding has fixed vectors, so changing it is a decision, not an accident.
- **Signed messages.** An unsigned, a wrongly signed, an expired, a replayed (same or lower `seq`) and a too-far-ahead message are each refused and change nothing. So are one signed for another agent or connection, one whose payload was altered, and a `RunPlan` signature on a `RepoGrants` payload. A forged message doesn't advance the sequence.
- **The hub's identity.** An agent refuses a hub whose key isn't the pinned one, and sends no proof and no `Hello`. A valid handover chain is followed and the new pin kept only after `Welcome`; a broken, forged or over-long chain is refused.
- **The key's life.** A hub that starts with its key missing and agents registered refuses `/sync/v1` and makes no new key; it makes one only on a first start.
- **Downgrade.** A `Welcome` with a lowered `protocol`, or an echo of a range the agent didn't send, is refused.
- **Binding.** Built-in mode: a relay between two TLS sessions is refused on both sides. A hub in that mode refuses kind 0, and an agent paired with a pinned certificate never signs kind 0.
- **Pairing.** A token used twice, an expired one and one with a wrong checksum are refused. A taken hostname leaves the token unspent; the token is spent by a good pairing only. A known ID with another key is refused. A wrong pairing key, a wrong agent key, or a mismatch of the token's hub key with the hub's, is refused, and the agent sends nothing of the token to a hub it didn't verify.
- **Copies.** A counter equal to or below the stored one, after a good signature, is `identity-conflict`, raises the alert and stops lease renewal.
- **Bounds and secrets.** An oversized or late pre-authentication frame closes the connection. A canary repository password appears in no frame (decision 3, I4).
- **Where they run.** The encoding and the checks in `domain` and `app` with fakes; the signing port has a contract suite over its fake and its real adapter; the TLS binding runs `@e2e` against a real listener.

## Appendix: the scenarios `fleet/pairing.feature` takes

Both Rust runners fail a scenario with an undefined step (`fail_on_skipped`, §13.1), so the file can't be committed ahead of its steps. #567's red `test:` commit adds it with these as its first scenarios.

```gherkin
Feature: Pairing an agent with a hub

  Scenario: An agent pairs with a hub
    Given a hub with a pairing token
    When an agent pairs with the token
    Then the hub lists the agent under its hostname
    And the token can't be used again

  Scenario: A token that has lapsed is refused
    Given a hub with a pairing token that has lapsed
    When an agent pairs with the token
    Then the pairing is refused as expired
    And the hub lists no agent

  Scenario: A hostname that is taken is refused and the token stays good
    Given an agent paired under the hostname "laptop"
    And a pairing token
    When a second agent named "laptop" pairs with the token
    Then the pairing is refused because the hostname is taken
    And the refusal names the agent that holds it
    When the second agent is renamed "desktop" and pairs with the same token
    Then the hub lists both agents

  Scenario: An agent paired again keeps its record
    Given an agent paired with the hub
    And a new pairing token
    When the agent pairs again
    Then the hub lists one agent, with its history

  Scenario: An agent that lost its data folder pairs as a new agent
    Given an agent paired under the hostname "laptop"
    And the agent has lost its data folder
    When the agent pairs with a new token
    Then the pairing is refused because the hostname is taken
    When the old agent is removed from the hub
    And the agent pairs with the same token
    Then the hub lists one agent under "laptop"

  Scenario: A registered agent can't be taken over with another key
    Given an agent paired with the hub
    When another machine pairs with a token under the first agent's ID
    Then the pairing is refused because the ID belongs to another key

  Scenario: A copied data folder is told apart from the original
    Given an agent paired with the hub
    And a second machine has a copy of its data folder
    When both machines connect to the hub
    Then the hub refuses the one whose connection count is behind
    And the hub raises an alert naming the agent
    And the hub stops renewing that agent's leases
    And the refused machine keeps backing up and runs no maintenance

  Scenario: An agent refuses a hub whose key isn't the one it trusts
    Given an agent paired with a hub
    When another hub that has another key answers its connection
    Then the agent refuses the hub
    And the agent keeps running its plans
    And the agent says the hub's key isn't the one it trusts

  Scenario: An agent follows a hub that changed its key
    Given an agent paired with a hub
    When the hub changes its key with a handover signed by the old one
    Then the agent trusts the new key from its next connection

  Scenario: A hub that lost its key is refused until its agents pair again
    Given an agent paired with a hub
    When the hub starts again with a new key and no handover
    Then the agent refuses the hub
    When the agent pairs again with a new token and confirms the new hub key
    Then the agent connects and keeps its record

  Scenario: A proof made for another hub or another agent is refused
    Given an agent paired with a hub
    When a proof is made for another hub's key
    Then the hub refuses it
    When a proof is made for another agent's ID
    Then the hub refuses it

  Scenario: A signature over the bare nonce is refused
    Given an agent paired with a hub
    When the agent's key signs only the hub's nonce
    Then the hub refuses the connection

  Scenario Outline: A command the agent can't trust is refused
    Given an agent connected to its hub
    When the hub sends a command that is <fault>
    Then the agent refuses it and acts on nothing

    Examples:
      | fault                                |
      | unsigned                             |
      | signed with the wrong key            |
      | signed for another agent             |
      | signed for an earlier connection     |
      | past its expiry                      |
      | sent again after it was accepted     |
      | altered after it was signed          |

  Scenario: A downgrade of the protocol is noticed
    Given an agent and a hub that both speak two protocols
    When the agent's range is lowered on its way to the hub
    Then the agent refuses the hub's answer

  Scenario: The hub never receives a repository secret
    Given an agent with a repository whose password is a canary value
    When the agent pairs and syncs with the hub
    Then the canary appears in nothing the hub received
```
