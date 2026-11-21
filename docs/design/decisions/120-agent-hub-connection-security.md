# Decision 120: How an agent and its hub prove who they are

**At connect, the hub and the agent each prove their identity with an Ed25519 signature over a domain-separated message: a fixed label, the hub's key, the agent ID, both sides' fresh nonces and the kind of TLS binding in use. Every message after that is signed the same way in both directions, with a sequence number. The hub has a key of its own that every agent pins at pairing, with an offline recovery key beside it. A newly paired agent is held pending until a person approves it on the hub. Mutual TLS, a required tunnel and peer-to-peer are not needed** (recorded 2026-11-18, §3.1, §7.1, §7.2, §10.3; #567, items 1–4 of the maintainer's comment of 2026-11-14; revised the same day after four independent reviews and the maintainer's answers).

This decision is the security core: identities, keys, the handshake, signed messages, the pairing token and its server side, approval, revocation, rotation and recovery, and what a hostile hub or agent can do. How a person pairs a machine (the hub's dialog, QR codes, the camera, the terminal commands, every error's next step) is [decision 121](121-pairing-a-machine-with-its-hub.md), which links here for each security rule rather than restating it.

The decision also fixes the order of the handshake, which decision 88 left open. The maintainer pre-approved security crates on 2026-11-18; "Crates" names them. The decision is too large for one pull request; issue #1040's "Implementation slices for #567 and #568" gives the order in which it is built, each slice its own pull request off `main`.

**Amended by decision 122** (2026-11-18): every surface this decision adds is built behind the `hub` Cargo feature, which release builds leave out until M8's release candidate. "The first release" here means M8's full release; until it, the handshake and its fields may change on `main` in place. The default backup exclusions of the hub's and the agent's key files ("The hub key") apply in every build.

## Where each item is answered

| Item | Answered in |
|---|---|
| #567 comment, 1: what the agent signs; the binding to TLS in built-in mode and why not behind a proxy | "The signed messages", "Binding to the TLS session" |
| 2: why not mutual TLS, a required tunnel or peer-to-peer; running over a tunnel stays supported | "Why not the alternatives" |
| 3: tests for the bare nonce, another hub, another agent ID | "Tests that pin it", first bullet |
| 4: the hub key pair: pinned at pairing, both sides prove themselves, signed commands, backup and rotation | "The hub key", "Signed messages after the handshake", "Tests that pin it" |
| Pairing token, hostname clash, re-pairing, a lost data folder, #296 | "Pairing", "A copied data folder (#296)" |
| A new agent held until a person approves it | "Pending approval" |
| A lost or leaked key; removing or re-keying an agent | "The hub key", "Removing, re-keying and recovering an agent" |
| What a hostile hub or agent can do | "What a hostile hub or agent can do" |
| How decision 88's window holds | "Versions: how decision 88's window holds" |
| Pairing experience, the Recovery Kit, QR codes, the terminal | [Decision 121](121-pairing-a-machine-with-its-hub.md) |

## Scope

- **One hub per agent.** An agent pins one hub key and reports to one hub. An agent served by two registries at once is a non-goal.
- **A move of this hub, and a restore of it, are [decision 137](137-a-hub-moves-by-handover-never-by-copy.md)'s.** A hub moves to another machine by a handover its old key signs, as when a paired machine is promoted to hub, or by the recovery handover when the old hub is gone, and both build on the chain below. A hub restored from a backup to an older epoch, or copied while the old one runs, enters recovery mode once it sees the rewind, and each machine refuses a lease below the highest it has seen. After any start the hub moves no owner until the machines have reported. A copy on another machine leaves recovery only by the recovery handover, and a hub that can't read its machine identifier doesn't start (I1).
- **The hub's generation is reserved now.** `generation` is a `u64`, 1 at first, signed in `Welcome` and `RepoGrants` and kept by the agent in `hub-pin`, per hub key. An agent refuses a lower one than it holds, from the first release, with the code `generation-lower`: it doesn't act on that `Welcome` or `RepoGrants`, keeps backing up, and shows its person the next step ([decision 121](121-pairing-a-machine-with-its-hub.md)). Nothing in this decision raises it; [decision 137](137-a-hub-moves-by-handover-never-by-copy.md) does, when a hub moves by handover, and agents that already refuse a lower one are fenced from the first day. It is in the first release because a field an agent relies on for safety can't be added later as an optional one (decision 88, item 9).
- **The generation floor follows the key.** Following a recovery handover starts the new key's floor afresh ("Rotation, recovery and the handover chain"); an ordinary handover carries it over. No setting lowers the floor, so a leaked hub key that sent the highest generation has set it until a recovery handover ("What recovery can't do"). Lease floors are ordered by the recovery number first, so a recovery handover outranks them too ([decision 137](137-a-hub-moves-by-handover-never-by-copy.md)).
- **Until M9's leases exist, pairing takes no lease and no backup away.** The hub grants no lease before M9, so a paired agent keeps the scheduled prune and check that M5 gives it unpaired, with M5's warning about other hosts' snapshots as its guard (§7.3: without a hub, `local` runs directly). That holds under §7.3, since a lease can lapse only if one was granted.
- **Two refusals can pause scheduled maintenance from this decision on,** `identity-conflict` ("A copied data folder") and `clock-skew` ("Time"), and from M9 a third, `epoch-lower` ([decision 137](137-a-hub-moves-by-handover-never-by-copy.md)); none ever stops a backup, and "What a hostile hub or agent can do" says who can cause them. The rule proposed for M9, which this decision doesn't make: an agent starts requiring a lease for a repository from the first `RepoGrants` it receives, so an agent that has never had one, a pending agent included, runs as before. Decision 88's item 6 speaks of an agent that has held leases.
- **TLS early data (0-RTT) stays off.** rustls's server leaves it off (`max_early_data_size` is zero by default) and its client does too (`enable_early_data` is false by default); both are set explicitly and tested, since a replayed first flight would not be bound to a session. A replayed `Connect` gets a challenge nobody answers.

## The handshake

An agent opens `/sync/v1`, a WebSocket over TLS. Until both proofs are checked, each side sends only what the diagram shows: nothing of the agent's plans, repositories or hostname reaches a hub the agent hasn't verified, and nothing of the hub's reaches a stranger.

```
agent                                      hub
  | Connect { agent_id, agent_nonce,        |
  |           binding_kind }                |
  |---------------------------------------->|
  |   Challenge { hub_nonce, hub_key,       |
  |      hub_handshakes, hub_proof,         |
  |      handovers, recovery }              |
  |<----------------------------------------|
  | (checks the hub key and hub_proof)      |
  | Proof { counter, agent_proof }          |
  |---------------------------------------->|
  | Hello { ... }, signed by the agent      |
  |---------------------------------------->|
  |   Welcome, signed by the hub (§7.2)     |
  |<----------------------------------------|
```

At pairing, `Proof` is replaced by `Pair` and the hub answers `Paired` ("Pairing"); the same connection then goes on with `Hello` and `Welcome`.

- **The agent proves nothing until the hub has.** The key in `Challenge` must be the agent's pinned hub key, or reach it through the hub's handovers or its recovery handover ("The hub key"), and `hub_proof`, which signs the handshakes the hub speaks, must verify. Otherwise the agent closes, sends no `Proof` and no `Hello`, and says `hub-key-mismatch` or `hub-proof-invalid`.
- **The hub answers every `Connect`,** and signs the `binding_kind` the agent named in it, so it needs no lookup of the agent ID before it answers: a stranger can't list the registry, nor learn an agent's kind by trying both. The refusal comes at `Proof`, one code (`proof-refused`) for an unknown agent, a removed one, a wrong signature and a kind the agent isn't recorded with; the one exception is a good proof from an agent whose pending record lapsed, which is answered `pending-expired` ("Pending approval"). A hub that can't offer a kind 1 `Connect` closes with `binding-unavailable`, the same for every agent ID. That is the hub that isn't on TLS, and the one whose session didn't negotiate the hybrid key exchange of "Post-quantum key exchange for agent–hub connections".
- **Nonces.** Each is 32 bytes from the `Random` port, fresh for each attempt. The two nonces stay the connection's context and are in every signature of it ("Signed messages after the handshake"). The hub's pending challenge, the entry that lets it check a `Proof`, is dropped when the proof is checked or after 15 s.
- **Bounds before the proofs** (§13.5). A frame over 4 KiB is refused. The agent speaks first, so a connection that hasn't sent a good `Connect` within 2 s is closed, and one that hasn't finished the handshake within 15 s. At most 32 connections may be unauthenticated at once; when the table is full the hub evicts the oldest unauthenticated one rather than refusing the newcomer, so a flood can't hold every slot for good. In built-in TLS mode at most 4 may come from one address, evicting that address's oldest first. Behind a proxy every connection has the proxy's address, so only the total applies. A `Hello` before a good `Proof` is a protocol error.
- **The order is new.** §7.1 had the hub send a nonce first. The hub can only sign the agent's nonce if the agent speaks first. No hub has shipped, so nothing is broken. From here decision 88's rule holds, as [amended by this decision](88-hub-and-agent-version-compatibility.md): the handshake is the same in every protocol, so two releases can always begin it.
- **The handshake is named by its label and path and never changes in place.** A different handshake is a new path and a decision. What an agent does when a hub offers only an older one is under "Versions: how decision 88's window holds".

## The signed messages

Every signature is Ed25519 (RFC 8032) over bytes built one way, by a pure function in `domain` that the tests pin with fixed vectors.

- **Encoding.** One byte for the label's length, the label in ASCII, then the fields in the order below, with no separator. Integers are big-endian unsigned 64-bit; a time is whole seconds since the Unix epoch. A key is its 32 raw bytes, an agent ID the 16 bytes of its UUID, a nonce 32 bytes, and the handshakes a hub speaks a 64-bit set (bit n−1 is handshake n). Only a hostname, a message kind and a refusal code are variable: each is one length byte, then UTF-8 (a hostname is at most 255 bytes; a kind and a code are ASCII). Every other field has a fixed width, so no two messages with different labels or fields encode to the same bytes.
- **The binding field** is 33 bytes: a kind byte (0, none, or 1, `tls-exporter`) and 32 bytes that are zero for kind 0.
- **Every label starts `ladderback-sync-v1 `.** The `v1` names the handshake, as `/sync/v1` does, not a protocol of the range `Hello` carries. The rest names the message, so a signature made for one message never verifies as another.

| Message | Signed by | Label ends | Fields, in order |
|---|---|---|---|
| Hub proof | the hub key | `hub-proof` | hub key, agent ID, agent nonce, hub nonce, binding, handshakes |
| Agent proof | the agent key | `agent-proof` | hub key, agent ID, agent key, agent nonce, hub nonce, binding, counter |
| Pairing | the token's pairing key and the agent key, two signatures over the same bytes | `pair` | hub key, agent ID, agent key, agent nonce, hub nonce, binding, counter, hostname |
| Paired | the hub key | `paired` | hub key, agent ID, agent key, recovery key, agent nonce, hub nonce, binding |
| Hub message | the hub key | `hub-signed` | hub key, agent ID, agent nonce, hub nonce, binding, sequence, expires at, kind, SHA-256 of the payload |
| Agent message | the agent key | `agent-signed` | hub key, agent ID, agent nonce, hub nonce, binding, sequence, kind, SHA-256 of the payload |
| Refusal | the hub key | `hub-refused` | hub key, agent ID, agent nonce, hub nonce, binding, code |
| Handover | the old hub key | `handover` | old key, new key, made at |
| Recovery handover | the recovery key | `recovery-handover` | recovery key, new hub key, recovery number, made at |
| Recovery rotation | the old recovery key | `recovery-rotation` | old recovery key, new recovery key, recovery number, made at |

Why each field is there:

- **The label** keeps one message from standing in for another, and keeps any of them from being a signature over something else the key signs. A signature over the bare nonce verifies as none of them.
- **The hub's key** is the hub's identity in every message, in both TLS modes ("The hub key"). A signature made for another hub is refused, because that hub's key is another value.
- **The agent ID and the two nonces** tie a signature to one agent and one connection. A recorded proof or command can't be replayed on another connection, and a hub's signature for one agent is no good to another.
- **Both nonces in both proofs.** Each side signs a value the other side drew, so neither can have a signature made in advance. The hub signing for a stranger's nonce gives the stranger nothing: the hub's own fresh nonce is in what it signs, and no other message has the label `hub-proof`.
- **Reflection.** The two proofs differ in label and signing key, so a hub proof presented back to the hub as an agent proof, or the reverse, verifies as neither. The label alone separates them: the two share a field prefix.
- **The binding in every message** makes each signature good on one TLS session only, where the kind has one, and names the kind in what is signed.
- **The counter** is the agent's connection counter ("A copied data folder").

## Binding to the TLS session

**Built-in TLS.** A kind 1 message carries the connection's `tls-exporter` value (RFC 9266): the TLS 1.3 exporter with the label `EXPORTER-Channel-Binding`, an empty context and 32 bytes. Each side computes it from its own end of the session (rustls `export_keying_material`), so the signatures hold only on the TLS session they were made for. The listener accepts TLS 1.3 only, which keeps the exporter free of RFC 7627's conditions.

What the binding adds to the pinned certificate: a party that holds the certificate's private key (a copy in a backup, say) but neither Ed25519 key can terminate an agent's TLS and relay to the hub, and each side's signature then covers a different session, so both refuse. The agent's pinned-certificate verifier must also check the TLS 1.3 `CertificateVerify`, so that a server that presents the pinned certificate without its private key is refused (a test below).

**One listener (#1210).** The hub serves the web UI, the API and `/sync/v1` on one address. A connection whose first byte opens a TLS handshake (`0x16`) gets the built-in TLS 1.3; one that opens in plain HTTP is served only from loopback, where a local proxy or a browser on the hub's own machine is, and closed from anywhere else, so nothing beyond the machine crosses the network unencrypted. A connection has 10 s to send its first byte and 10 s more to finish the handshake. Over the built-in TLS, the 4-per-address bound of "Bounds before the proofs" counts the address the connection came from.

A proxy on another machine therefore forwards to the hub's `https://` address; its connections are kind 0, as below. So does one on the host of a container on a bridge network, whose connections come from the bridge's gateway, not loopback. The first plain connection closed is logged as a warning that says so. For the same reason the daemon's `listening on` line names the `https://` address when the built-in TLS serves and the hub listens beyond loopback. It names the `http://` one on loopback, where plain HTTP is served too; on an unspecified address (`0.0.0.0`, `::`), which no other machine can use and which reaches loopback on the hub's own; and without the TLS.

**At most 64 connections at the door.** Until the server takes it, a connection is at the door: waiting for its first byte, in its handshake, or waiting to be handed on. At most 64 may be there at once; a newcomer past that closes the oldest still there, as the bound on unauthenticated connections does, so a flood that sends nothing, or one byte of a handshake, can't hold the door shut. A connection that is let in passes the door in milliseconds; 64 is twice the 32 unauthenticated `/sync/v1` connections, since the door also takes every connection of the web UI and the API.

**TLS 1.3 only, on every connection.** The listener accepts nothing older, and so does every client the agent builds, including one to a proxy in kind 0, where no exporter is needed. A proxy that speaks only TLS 1.2 is refused on purpose: one rule for every `ClientConfig` is simpler to test than one per binding kind, and a deployment that can't offer TLS 1.3 is told to update its proxy.

**Post-quantum key exchange for agent–hub connections.** Every rustls config Ladderback builds takes its key exchange groups from a provider of our own, `aws_lc_rs::default_provider()` with `kx_groups` set explicitly, and never from the provider's default list, so a build feature that reorders that list changes nothing here. The hybrid group `X25519MLKEM768` exists in the aws-lc-rs provider, which is the provider this decision names; the `ring` provider has none.

- **A pinned address (kind 1): the hybrid is the only group offered and accepted.** The agent offers `X25519MLKEM768` alone and checks `negotiated_key_exchange_group()` after the handshake, so a recorded session can't be decrypted later by a quantum computer, and a peer that speaks only X25519 is refused.
- **The hub's listener also serves browsers,** and it can't tell an agent from a browser before the HTTP upgrade, so it accepts the hybrid and X25519 both, and the client's order decides which a session uses (rustls takes the first group in the client's list that it supports). It enforces the rule at `Connect`: the hub hands `/sync/v1` a session's exporter value only when the session negotiated the hybrid, so a kind 1 `Connect` on any other has no binding and is closed with `binding-unavailable`.
- **Toward a proxy (kind 0): the hybrid is preferred, and X25519 is the fallback,** since a proxy may not speak it. That leg is as safe as the proxy's own TLS.
- **With built-in TLS, the Recovery Kit follows the same rule.** The hub reads its own session's group and sends the kit page and its Download only on a session that negotiated `X25519MLKEM768`, since the seed is the one long-lived root key; on any other session the page refuses with `kit-command-only` and points to `ladderback recovery-kit`. Behind a proxy the hub can't read the proxy's session; "What this doesn't defend" names that.
- **Signatures stay Ed25519.** A proof is made and checked within one connection, so a recording gains nothing from a later forgery. A move to ML-DSA or a hybrid signature needs a new handshake, with a new label and path under decision 88's rules ("Versions"), and the hub key's handover carries the new key type.

**Behind a TLS-terminating proxy** there is no shared session. The agent's TLS ends at the proxy; the hub sees another connection, so the two ends compute different values and every honest connection would fail. The binding is kind 0, and the hub's key carries the whole of the hub's identity.

- **The agent names the kind in `Connect`, and the hub signs that kind.** The hub could not otherwise know which kind to sign before it knows which agent is asking, and defaulting would make a pinned agent's pairing proof relayable. An agent always checks the hub's proof under the kind it sent, so a flipped byte fails closed.
- **The kind is never negotiated, and is a property of the address.** An agent connecting to an address pinned to the hub's certificate always sends kind 1; one connecting to an address behind a CA certificate always sends kind 0. The token's TLS byte (0 CA, 1 pinned) gives the kind of the token's address; the transport statement gives the kind of every other ("Addresses and the transport statement").
- **The hub stores the token's TLS byte** with the pairing key, and requires at `Pair` that `Connect` named that kind. It records, per agent, the set of kinds the agent may use (the token's, and the kinds of the addresses it publishes), and refuses at `Proof` a kind outside the set. A mismatch is a signature that doesn't verify or a refusal, never a quiet fallback.
- **So one hub serves both:** agents on the LAN pinned to its certificate, and the same agents or others through a proxy. A proxy that re-encrypts to the hub's own listener gives the hub a TLS session the agent doesn't share; those connections are kind 0 and the hub accepts that of agents recorded for it. A plain loopback `http://` connection is kind 0.
- **What a proxy can still do.** It can relay a handshake it didn't start. It can't act as the hub to an agent without the real hub (no hub proof), act as the agent to the hub without the agent, or make an agent act, or make the hub decide, with a message of its own. It can read what isn't encrypted, delay it and cut the connection ("What this doesn't defend").
- **A plain `http://` address is refused** by an agent that pairs or connects, unless it is loopback, as the MCP token's address is (§10.5).

## The hub key

- **Made once.** The hub makes an Ed25519 key pair the first time it starts as a hub with no registered agent and no recorded key. The seed is 32 bytes from the `Random` port, which also lets a test seed it. It is not the hub's own agent key: the hub is an agent as well, and a signature made as an agent must never be one made as the hub.
- **Kept in the data folder, never the database**, as the agent's key is: a file `hub-key` of one line, the seed as 64 lower-case hex digits, mode 0600, written whole under a private name and linked into place, as the agent ID file is. `db rebuild` can't change it. Other operating-system users can't read it; a program running as the person can, which decision 99 accepts. In memory the seed is wiped when dropped. A `hub-key` file that exists is never overwritten, whatever the database holds, with one exception: the rotation below.
- **The certificate's key is kept beside it.** In built-in mode the hub makes an ECDSA P-256 key once (`hub-tls-key`, as private as `hub-key`) and a self-signed certificate for it. The fingerprint in the token is the SHA-256 of the certificate's public key (SubjectPublicKeyInfo), so the hub may reissue the certificate from the same key when it lapses without any agent pairing again. A new TLS key changes the fingerprint and costs the agents that pinned it a new address statement or a re-pairing. **The agent's pinned verifier ignores the certificate's dates and names:** it compares the key's fingerprint and checks the `CertificateVerify` signature, so a lapsed certificate can't lock out an agent that was offline for weeks, and a hub reached by another name still verifies.
- **Pinned at pairing.** The token carries the public key, so each agent holds it before it sends anything. The agent keeps its pin in its data folder (`hub-pin`, a text file of `name = value` lines in which a reader ignores names it doesn't know and keeps them when it rewrites the file), never its database. The pin file also holds the recovery key and the highest recovery number followed, the hub's generation per key, the addresses (the token's and the person's, and the latest transport statement) and the hub release last seen, so a newer agent's file is read by an older one.
- **A fingerprint is shown one way everywhere.** It is the SHA-256 of the key's 32 raw bytes (of the certificate's public-key encoding for the TLS key), shown as its first 100 bits: 20 characters of Crockford base32 in five groups of four, upper case, joined by hyphens (`7K3Q-M9XD-2RTV-8HFA-QW5N`), with the full 64 hex digits on demand. 100 bits keeps a search for a key that matches by chance beyond reach, where 32 bits, which a search finds in minutes, would not. Where it is shown and compared is [decision 121](121-pairing-a-machine-with-its-hub.md); the function is in `domain`, with fixed vectors.
- **Three other things are called IDs,** each by what it labels: the **agent ID** (the UUIDv7 the agent makes), the **key ID** (the first 8 hex digits of a key's SHA-256) and the **token ID** (the same, of a pairing key). A key ID is a label beside a name, never a way to verify a key and never a way to tell lookalike names apart.
- **The hub never makes a replacement by itself.** The database records the key's fingerprint when the key is made. A hub that starts with the file missing or another key refuses `/sync/v1`, says so on its pages and in its log, and offers three ways out: restore the file, recover with the recovery seed (below), or reset the key, which names every agent that will need to pair again. The same holds for the TLS key in built-in mode, whose fingerprint the hub records in `hub_tls_key` (§9).
- **A TLS key that can't serve fails closed (#1210).** The hub keeps its one listener without the TLS: it serves its own machine over plain HTTP on loopback and closes every TLS handshake and every plain connection from beyond it; `/sync/v1` answers `503`; and the log names the key file and how to fix it (restore it from a backup, or make it its owner's only). The daemon, and the hub machine's own backups, go on. Its pages don't say so yet, as they don't for the hub key; nor is the TLS key's reset built. A hub with agents, no recorded fingerprint and no key file, a development hub from before `hub_tls_key`, is refused the same way, since an agent may have pinned a key whose record is gone.
- **Backup.** Copy `hub-key`, `hub-tls-key` and `hub-handovers` (below) to somewhere only the person can read; Settings shows their paths and the fingerprint, so a restored key can be checked. Anyone holding the files can pose as the hub to every agent, so they are kept like a password. **Ladderback's own plans leave them out:** a plan whose sources include Ladderback's data folder excludes `hub-key`, `hub-tls-key`, `hub-recovery-seed` and the agent key file by default and says so on the plan's page, because a repository another machine can read would hand that machine the key. The seed file matters most: it is the root key, which overrides every other. Switching the exclusion off asks for a confirmation that names who can read the repository.
- **A `hub-key` left in a backup after a rotation** can still impersonate the hub to an agent that hasn't connected since: delete old backups, or treat the rotation as a leak.

### The recovery key

- **Made with the hub key.** At first start the hub also makes a recovery key pair. It keeps the public half in a file `hub-recovery-key` in the data folder as well as in the database, so `db rebuild` keeps it and `hub-recover` has something to check a seed against. The seed is 52 Crockford base32 characters in groups of four ([decision 121](121-pairing-a-machine-with-its-hub.md) shows it as a Recovery Kit). Until the person confirms they have it, the seed waits in a file `hub-recovery-seed`, mode 0600, which the default backup exclusion leaves out. The hub never holds a confirmed seed, and never logs it.
- **The kit is viewable in the browser for 7 days only, and only with sign-in on.** The web page that shows the kit, and its Download, work for a signed-in session with a fresh step-up for 7 days after the hub is set up, and for 7 days after each `hub-recover`. With sign-in off or in proxy-header mode, after the 7 days, and with built-in TLS on a session that didn't negotiate the hybrid key exchange, the page and the Download are refused with `kit-command-only`, and the hub shows the seed only through `ladderback recovery-kit`, run on the hub's machine by the user that owns the data folder. Settings says the kit is waiting while the file stands, and when the page is refused, that the command shows it.
- **Why the limit.** Without it, whoever learns the admin password (a phished or shared one) could fetch the root key at any time while it stays unconfirmed, sign a recovery handover to their own key with the highest number, and capture every agent for good. The hub can't tell a browser on its own machine from one behind a proxy on that machine: a proxy that rewrites `Host` passes for loopback ("Who may make a token, approve or re-key"), and the typed key ID that stands in for a password with sign-in off is public. So the page makes no exception for loopback and needs sign-in.
- **Every view raises a §11 event.** The web page, a download and the command each raise `hub.recovery_kit_viewed`, which names which of them, when, who (the signed-in user, or the operating-system user for the command) and the peer address, never the seed. Settings shows the last view.
- **One step-up opens the kit page, and its Download needs no second one.** The step-up that opens the kit page covers its Download, for at most 10 minutes and only while the person stays in the kit's pages. The kit page and its Download are one action ("Who may make a token, approve or re-key"). The confirmation is another action, so the kit's step-up never covers it (next bullet).
- **The person confirms on a page that doesn't show the seed,** by typing back one group picked at random when the page loads, with its number shown ("group 7 of 13"), never always the last. The confirmation takes a new step-up, taken after the kit's step-up has ended, and the web page offers it at any time, also after the 7 days, since it shows no seed. A paste is allowed.
- **What the confirmation catches.** A kit that is missing, cut short or mistyped: a person who has the kit on paper or in a file can answer, one who never saved it can't answer by accident. It doesn't prove the kit is kept offline, since a person can copy a group from the kit's page to this one. The hub accepts Crockford's aliases (`O` for `0`, `I` and `L` for `1`) and any case, and the field's description says so. The field is separate from the step-up, whose field is the password. The right group deletes the file; `ladderback recovery-kit --confirm` on the hub's machine asks for a random group the same way.
- **Pinned beside the hub key.** `Paired` carries the recovery public key, signed by the hub key, and the agent stores it in `hub-pin` with the hub key. After that it changes only by a `recovery-rotation` the recovery key itself signs ("Rotation, recovery and the handover chain"), never by the hub key alone, so a leaked hub key can't replace it. A hub key leaked before an agent first paired could give that agent the wrong one; that is the same exposure as a hub posing at pairing.
- **A lost seed costs nothing beyond what the key buys.** The hub key still rotates by handover, and a leak without the seed ends in a reset. Settings tells the person which they have.

### Rotation, recovery and the handover chain

- **Rotation is a handover the old key signs.** The hub makes a new key, signs `old key, new key, time` with the old one and stores that in `hub-handovers` (public). It closes every open connection, since the old seed signed them, and voids every unused pairing token, since each carries the old key. At connect it sends its handovers in `Challenge`, at most 8, oldest first. An agent whose pin is the old key of a link follows it, checking each signature with the key before. The agent never checks a link's `made at`.
- **Crash-safe order.** It is the one exception to "never overwritten", and the order is fixed so a crash at any step leaves a hub that starts. The new seed is written under the private name `hub-key.new` and synced, with its folder. The handover is appended to `hub-handovers` and synced. One database transaction records that the old or the new key is accepted and voids the tokens. `hub-key.new` is renamed over `hub-key` and the folder synced. A last transaction drops the old key and the connections close. At start, a `hub-key.new` whose handover is in the file is finished; one without is deleted, so the rotation never happened. A second handover from a key that already has one is refused, so a retry can't fork the chain.
- **The agent keeps the new key once the whole handshake has succeeded,** `Welcome` included. Until then it verifies every message of that connection, `Welcome` too, against the key it reached through the chain, and the pin on disk still holds the old one. A chain that doesn't reach the agent's pin is `hub-key-mismatch`. An agent that missed more than 8 rotations follows a recovery handover if the hub has one, and otherwise pairs again.
- **Recovery from a leaked key.** `ladderback hub-recover`, run by the user that owns the data folder, takes the recovery seed only at a prompt with no echo or on standard input, never as an argument, where `ps` and the shell's history would keep it. It reads the seed in any case, with or without hyphens, spaces and line breaks, and with Crockford's aliases, so a seed typed from paper under stress is read. Its help recommends a file or a here-document (`ladderback hub-recover < seed.txt`) over `echo <seed> |`, which leaves the seed in the shell's history. It derives the public key from the seed, shows its fingerprint (the one on the Recovery Kit) and compares it with `hub-recovery-key` before it changes anything. A seed that isn't the stored one is refused and nothing changes, so a mistyped seed can't rotate the hub onto a chain no agent follows.
- **What it makes.** A new hub key, rotated in by the order above, and a new recovery key. The old recovery key signs a recovery handover ("the hub key is now X") and a `recovery-rotation` to the new recovery key, both carrying the recovery number, which rises by one at each use. The hub keeps them, public, in `hub-recoveries` (at most 8), and sends them in `Challenge`. It shows the new seed as a Recovery Kit as at first start, with a new 7-day window, and the person confirms it the same way; the old seed is forgotten, never stored.
- **An agent follows a recovery handover from any pin.** It needs only its pinned recovery key (or the chain of rotations from it) and a recovery number above the highest it has followed; it depends on none of the keys it has seen. So an agent that was led onto an attacker's key, or whose history of keys an attacker pushed out with a chain of handovers, follows it the moment it reaches the real hub. Following it discards the agent's transport statements and their numbers and restarts its generation floor, and the recovery number it now holds outranks its lease floors ([decision 137](137-a-hub-moves-by-handover-never-by-copy.md)); the token's address and the person's own addresses stay.
- **What recovery can't do.** Someone who holds the leaked key can act as the hub, within the bounds of "What a hostile hub or agent can do", to an agent they reach before the real hub, until that agent follows a recovery handover. The agent tries every address in turn, the ones it was given among them, so it reaches the real hub to follow it. A leak that is also a position in the network that blocks the real hub is not closed by any key scheme.
- **A leaked key always needs a recovery or a reset.** The holder can raise an agent's generation floor to the maximum with one `Welcome`, so the real hub, on that key or on the next one by an ordinary handover, is refused as `generation-lower`. That holds whether or not the agent reaches the real hub again: only a recovery handover restarts the floor, and a reset is a new key.
- **What losing the key costs, with the recovery seed:** nothing beyond the recovery. A lost key is recovered as a leaked one is, since the recovery handover is signed by the recovery key and needs nothing from the old private key.
- **What losing both costs.** Every agent refuses the hub (`hub-key-mismatch`), and each pairs again with a new token, naming both fingerprints for the person to confirm. Agents keep backing up and, as with a version mismatch (decision 88), once leases exist (M9) theirs lapse in at most 7 days, after which they run no scheduled maintenance. The hub's registry survives, and an agent that pairs again with its own ID and key keeps its record, approved as it was.

### The key files on disk

- **What each holds.** `agent-key`, `hub-key` and `hub-recovery-seed`: the 32-byte seed as 64 lower-case hex digits and a line feed. `hub-recovery-key`: the public key, written the same way. `hub-tls-key`: the PKCS#8 encoding of the ECDSA P-256 key in a PEM `PRIVATE KEY` block, 64 base64 characters a line, the key at most 4096 bytes. `connection-counter`: a decimal number and a line feed. `hub-pin`: `name = value` lines ("The hub key"). A reader takes a file of one line with or without its line feed, and the PEM block with any line breaks (CRLF too) and white space after it; anything else, or more bytes than the file can hold, is a damaged file, and no error carries a file's text (I4).
- **How they are written.** Mode 0600 on Unix. A file is written whole under a private name that starts with its own (`hub-key.<id>.tmp`) and synced. A key file is then hard-linked into place, so two first starts agree on one key and an existing key is never overwritten; the pin and the counter are renamed over the old file. The folder is synced before the write answers. Every read is capped and lands in a buffer wiped when dropped. All of it is behind the `Identity` port, whose adapter is platform's `identity` module, built only with the `hub` feature ([decision 122](122-hub-out-of-releases-until-m8.md)).
- **A file others can read or change is refused.** On Unix a key file with any group or world permission is refused (`Exposed`, with its mode) before any of it is read, and left as it is: only Ladderback writes these files, so another mode means one was copied or changed by hand, and a key others can read or replace is no longer this machine's alone (#1137).
- **The counter's lock** is an advisory lock on an empty file of its own, `connection-counter.lock`, beside a process-wide lock ("A copied data folder"): the counter's own file is replaced whole at each write, and a lock on a file that is then replaced would lock nothing. A second attempt while one holds the lock is refused at once, never queued.
- **How a backup leaves them out.** Each backup adds a glob `!<data folder>/<name>*` to its iglobs, for each of `hub-key`, `hub-tls-key`, `hub-recovery-seed` and `agent-key`, case-insensitive so another spelling on a case-insensitive file system stays out, and ending in `*` so a staged or rotated name (`hub-key.new`) goes too. The folder's characters are escaped, and the globs are written for the folder as given and as it resolves, since rustic walks a source's resolved path (`/private/var` on macOS). A plan dry run adds the same globs, last, to every walk, so it counts what the backup takes, and never tallies them as one of the plan's excludes.
- **After every glob of the plan's.** In gitignore's rules the last glob that matches a path decides it, so the key files' globs come after every glob of the plan's own, its iglob files' lines included, and none of them, an include-everything `*` among them, can bring a file back. rustic adds the iglob files' lines after `iglobs`, so Ladderback reads them itself, as rustic would, and adds its own after them.
- **A data folder no glob can name fails closed.** A glob is text, so a folder whose path, as given or resolved, isn't UTF-8 can't be named by one. Then a backup whose sources hold the folder, as written or resolved, is refused with `key-files-not-left-out` while the folder holds a file the globs would leave out, or can't be listed, before any secret is read; with none there, or with sources elsewhere, the backup runs as before. A plan dry run of the same sources is refused the same way, by the same check, once the allowlist's checks pass, since it exists to predict what the backup does. The person moves the data folder or takes the folder that holds it out of the plan's sources.

## Addresses and the transport statement

A token carries one address and one kind. The hub may later publish more.

- **A transport statement** is a hub message (`TransportStatement`) that lists, for every agent, up to 8 addresses, each with its kind and, for a pinned one, the certificate's fingerprint, with the hub's `generation` and a number that only rises. It is signed by the hub key like every hub message, so a proxy can't add or change an address. The agent keeps the list in `hub-pin` and replaces it only when its (generation, number) is higher, compared in that order. A leaked hub key can number a statement as high as it likes, so the list is never the only way to a hub (next bullet), and recovery discards it.
- **Two kinds of address no statement can remove.** The token's address, and any address the person sets on the agent (in Settings, or `ladderback pair <address>`), are kept in `hub-pin` as fallbacks. The agent tries them after a statement's addresses, so a hostile list leaves the real hub reachable and a recovery handover has somewhere to arrive.
- **Why.** A laptop that reaches the hub pinned on the LAN and through a proxy away from home needs both kinds, and a hub whose address, certificate or proxy changes would otherwise re-pair every agent. The person publishes the new address first and removes the old one later.
- **The agent tries the address that last worked, then the others,** a statement's and the fallbacks, each with its own kind and pin, one attempt in flight at a time. A statement's addresses follow the plain `http://` rule above.
- **The hub knows who has the list.** An agent answers a statement with `TransportAck { number }`. Settings shows which agents hold the current list, so the person removes an old address only when the agents that matter have the new one. An agent that doesn't speak the message (an older protocol, "Versions") stays listed as unconfirmed.

## Signed messages after the handshake

**Every message after the handshake, in both directions, is signed:** `Welcome`, `Paired`, `RepoGrants`, `Ack`, `FetchLog`, `TransportStatement`, `Removed` and, from M9, `RunPlan`, `CancelOp` and `RestoreRequest` (`UpdatePlan` the day it ships) from the hub; `Hello`, `PlansSnapshot`, `ReposSnapshot`, `OpEvent`, `Heartbeat`, `ReleaseAck`, `TransportAck`, `LogChunk` and `RestoreResult` from the agent. Neither side acts on, stores or answers an unsigned message. An earlier draft left plans, events, `Ack` and the rest unsigned, on the reasoning that they only change what a person sees; they don't. The hub's forget uses a published plan's `KeepOptions`, a forged `Ack` makes an agent drop outbox events the hub never saw, and a forged `OpEvent` result ends a prune guard's wait. Signing costs microseconds a message [INFERENCE], and signing everything leaves no list of exceptions to get wrong. Payloads are not encrypted a second time: the link carries no secrets (decision 3, I4).

- **The envelope** holds `kind`, `seq`, `expires_at` (integer seconds, hub messages only), the `payload` as the unpadded base64url of its JSON bytes and the `signature`. The digest is over the decoded bytes, and the decoding is strict: a padded, mixed-alphabet or otherwise non-canonical encoding is refused. The receiver parses the payload only after the signature holds, from the same bytes the digest covered, so nothing is canonicalized.
- **The key and the connection.** The receiver builds the signed message from the envelope, the agent ID, the hub key this connection verified (the key in `Challenge`, reached from the pin through any handovers) and the two nonces of this connection, so a message made for another agent, hub or connection fails to verify. A hub key changed mid-connection is no hub key this connection verified.
- **Sequence.** Each side counts its own messages from 1 on each connection, and the receiver accepts only the next number, exactly one above the last it accepted. A repeat, a gap or a message out of order closes the connection (`sequence-error`), so a party in the middle can't drop one signed message and let the rest through; it can only cut the connection. A forged message doesn't advance the count. The receiver records the number before it acts. This `seq` is the connection's, not an outbox event's `(epoch, seq)` (§7.2).
- **Time.** `Welcome` carries `signed_at`, the hub's clock when it signed. Because the agent's nonce is in what the hub signs, `Welcome` is fresh. The agent records `offset = signed_at − its own clock` and warns above 5 minutes, but it never closes for a skew.
- **Expiry is judged on the hub's time,** reconstructed on the agent's own monotonic clock from `signed_at`, so an agent whose clock is minutes slow still works and a NAS that drifts off sync costs nothing. The agent refuses a message whose `expires_at` has passed or lies more than 15 minutes ahead (the hub signs at most 10 minutes ahead). A lease's own `expires_at` is converted to the agent's wall clock with the same offset, so sleep still counts (§7.3 measures I3 on wall time). An offset above 24 hours is a `clock-skew` alert, and the agent runs no scheduled maintenance until it is fixed.
- **The hub signs when it sends, never when it queues,** so a command waiting for an offline agent gets a fresh signature and sequence on connect. How old a command may be is the command's own rule ("Commands").
- **Order of checks.** Signature first, with nothing recorded for a message that fails; then sequence; then expiry; then the receiver acts. A message of a kind the receiver doesn't know is verified and checked the same way and uses its sequence number, but nothing acts on it, so the next message is accepted ("Versions"). A forged or replayed message also closes the connection, and the refusal is a code in the log.
- **Refusals are signed.** A hub that has proved itself refuses with a `hub-refused` message carrying the code, under its key, both nonces and the binding. An agent acts on a refusal (tells its person, stops maintenance, says to make a replacement token) only if it verifies. An unsigned or wrongly signed one is a dropped connection: the agent backs off, tells its person nothing and stops no maintenance, so a proxy can't send `identity-conflict` to send a person to re-key a healthy agent. A refusal before the hub's proof (`binding-unavailable`, a bad `Connect`) can't be signed and is treated the same.
- **`Welcome` carries `agent_protocols`,** the range the hub saw in `Hello`. The agent refuses a `Welcome` whose echo isn't the range it sent, or whose `protocol` isn't the highest both ranges share. A proxy that lowers either side's range, to push the pair onto an older protocol (decision 88), is caught. `Hello` is signed by the agent, so the hub sees the range the agent sent. `Welcome` also carries `generation`, `hub_handshakes`, `hub_version` and the agent's `status` ("Versions", "Pending approval").
- **`RepoGrants` keep their own `expires_at`,** the lease's (§7.3). The message's expiry is how long it may be accepted, not how long the lease runs.
- **The hub identifies an agent by the hostname registered at pairing,** never the one in `Hello`. A pending agent's hostname is fixed; an approved agent's rename comes in the signed `Hello` and is applied only if no other agent holds the new name.
- **The release acknowledgement.** To move a lease, the hub stops renewing the old owner's and grants the new owner only after the old owner's signed `ReleaseAck { repo_id, epoch }` for the epoch it held, or after that lease expires (§7.3). An agent sends it once it has dropped the lease and has no maintenance running on the repository. A forged one can't exist: it is an agent message, signed, on one connection. Without it the move waits for the lease to run out, which is the safe direction.
- **Decisions 2 and 12 stand.** The hub still sends only the request and the agent runs it, a restore only into a new folder and only where `allow-hub-restore` is set, and `UpdatePlan` stays reserved. The signature is one more condition on these, never a way round them.

## Commands

A hub command (`RunPlan`, `CancelOp`, `RestoreRequest`, from M9) can wait for an agent that is offline for weeks. Replay protection on one connection is not enough for that.

- **A command carries a `command_id` and a `requested_at`,** in its payload, so both are signed. `command_id` is a UUIDv7 the hub makes when the person asks; `requested_at` is the hub's time then, not the time it was sent.
- **The agent refuses a command older than 15 minutes, or dated more than 15 minutes ahead,** judged on the hub's time as above. A hub clock days ahead when the person asked would otherwise let the command outlive the 24 hours of IDs the agent remembers and run twice. The hub drops a queued command that old and tells the person it didn't run; a person who asks again gets a new ID.
- **The agent remembers the IDs it has acted on** for 24 hours, in its store, and records an ID before it acts. A command delivered again after a disconnect gets the first run's recorded result, and never runs twice; a restore that was cut off in transit doesn't restore twice.
- **`RunPlan` is rate limited by the agent:** at most one hub-started run of a plan in 5 minutes and 12 an hour across the agent, the rest answered `rate-limited`, because each run fires the plan's hooks. The numbers are defaults the tests pin.
- **Both fields are part of a command's definition** from the first release that has it, and an agent refuses a command without them. Changing either is a new protocol (decision 88, item 9).

## Pairing

The token is `ladderback_pair_` and the unpadded base64url of: a version byte (1), the hub key (32), a TLS byte (0 CA, 1 pinned), the certificate's public-key fingerprint (32, zeros for CA), the seed of a one-time pairing key (32), the hub address (one length byte, then at most 200 bytes of UTF-8: `https://host:port`) and the first 4 bytes of the SHA-256 of all before them. It is 103 bytes plus the address. The checksum only catches a mistyped token before any connection is made; it is no security. The prefix lets a secret scanner find a leaked token.

- **The address is what the person was using** when the hub's page made the token, and the dialog lets them edit it. It needs no secrecy and no trust: a changed address leads to a host that can't give the hub's proof, and the agent then sends nothing of the token.
- **An address carries its kind, pinned or CA.** The dialog's address field has a certificate choice beside it, and `ladderback pair <address>` takes `--kind pinned` or `--kind ca` with the address. `ladderback pairing-token --address <address>` makes a pinned token for an `https://` address, and a CA one with `--certificate-authority` (#1210). The default for a token is the kind of the listener the request came in on (built-in TLS is pinned; a plain listener behind a proxy is CA), and for `pair` the kind of the token's address. So a person who edits the address to a proxy's and chooses CA gets a CA token (TLS byte 0), and an honest proxied setup is never handed a pinned token for an address it can't pin.
- **The hub keeps only the pairing key's public half,** with its TLS byte, hub key, who made it, its expiry, whether it was used or revoked, and for a replacement token the agent ID it is for. A stolen database holds no usable token. The token is shown once. Its life is 15 minutes by default, 5 minutes to 24 hours on request, and at most 20 are unused at once. It is spent in the same transaction that records the agent, so two agents pairing with one token at once get one success and one `token-used`.
- **An unused token can be revoked.** The hub lists unused tokens by token ID with their expiry, and a person revokes one from Settings or with `ladderback pairing-token --revoke <id>`; a revoked token is refused as `token-revoked`. A screenshot synced to a cloud, a share into a notes app or a terminal log can't be waited out. A key rotation voids every unused token too.
- **Pairing is the handshake, with a different last step.** The agent starts with `Connect` and checks the hub's proof against the key in the token, so the token's seed never leaves an agent that hasn't verified the hub, and the seed itself never crosses the wire at all. It sends `Pair`, naming the public half of the pairing key so the hub finds the token, with the pairing message signed twice: by the pairing key, which is the hub's authorization, and by the agent key, which proves it holds the key it registers. The hub answers with a signed `Paired`.
- **A retry of the same pairing is answered again.** If `Paired` is lost on the way, the agent pairs again with the same token. The hub recognizes the same agent ID and key on a token it spent for exactly that agent and answers as before, while the agent is pending or approved; any other agent gets `token-used`. So the real agent isn't sent to look for a thief by a dropped connection.
- **The token never goes on a command line,** where another user of the machine could read it with `ps` and the shell would keep it in its history ([decision 121](121-pairing-a-machine-with-its-hub.md) says how it is given instead).
- **Hostnames.** A hostname is at most 255 bytes of UTF-8 with no control, format or bidirectional-control character, no other character that shows as nothing or as a blank (the assigned default-ignorable characters, such as the Hangul fillers and the variation selectors, the braille blank, and the line and paragraph separators, #1065), and no space at either end. The hub compares names as `casefold(NFC(name))`, with `casefold` the standard library's `to_lowercase`, which is enough for names; one taken by another agent, pending or approved, is refused with `hostname-taken`, naming the holder and when it was last seen. The token isn't spent. Ladderback doesn't restrict hostnames to ASCII, since machines are named "Nick's MacBook Pro" or in other scripts.
- **Lookalike hostnames.** A name whose UTS #39 skeleton, `skeleton(casefold(skeleton(NFC(name))))` with each run of spaces read as one, equals another agent's or a tombstone's, such as "lаptop" with a Cyrillic а or "Iaptop" with a capital I beside "Laptop", is not refused, since a false match would lock out an honest name; the hub flags it on the pending row and in the approval, which compares keys, not names. The inner skeleton reads a capital before the case fold changes it, so a capital I is an l, as it looks. How a person sees the flag, and the note on non-ASCII names, is [decision 121](121-pairing-a-machine-with-its-hub.md).
- **The lookalike flag** names the first other agent, pending or approved, whose name has that skeleton, and is worked out each time the hub lists its agents rather than kept, so it holds against the registry as it is; two pending lookalikes are each flagged with the other. Tombstones join the comparison when removal keeps them (#1065).
- **Pairing again as the same identity** (the same ID and key) reuses its record and keeps its approval. The hub's stored counter becomes the greater of its own and the agent's, and a token that isn't a replacement never lowers it or clears a conflict. An agent ID that arrives with another key is refused (`agent-key-mismatch`): a token authorizes a new agent, never taking over a registered one. An agent that pairs with a token whose hub key isn't the one it has pinned asks its person to confirm, naming both fingerprints, before it replaces the pin.
- **A removed agent's ID is a tombstone.** Removing an agent, or rejecting a new pending one, keeps its agent ID, its hostname and its last key's fingerprint, so a rogue can't register under an ID whose snapshots are orphaned and have the hub's forget treat them as its own. An ordinary token under that ID is refused as `agent-removed`, and the agent tells its person to ask for a replacement token. Only a replacement token made for that ID lets it in again, and the agent then comes back pending.
- **A replacement token** is made on the hub for one agent ID, from a conflict alert, from the agent's row (re-key) or from a tombstone, and is refused for any other ID (`token-not-for-this-agent`). The agent takes a new key under its old ID. For a registered agent the hub **holds the new key pending beside the record**, which keeps its old key, its approval, its counter and its conflict as they were: the old key keeps verifying, or stays refused if the record was in conflict, until a person approves the new one. Approval swaps the key, adopts the pending key's counter as the record's and clears a conflict. Rejection discards only the new key, so a leaked replacement token used first can't remove a real agent. A tombstone has no record to keep, so that agent comes back pending.
- **A pending replacement key has a counter of its own and no connection slot.** Its proofs are checked against its own counter, never the record's, and are answered `status: pending`; they close no live connection and take no slot of "One live connection per agent ID". So a stranger who uses a leaked replacement token first and proves with a high counter can't close the real agent's session or push its counter into an `identity-conflict`.
- **A pending replacement key lapses, counts and shows like a pending agent.** It lapses 7 days after it is made and is then discarded, the record untouched. It counts toward the limit of 20, is in the Banner's count, and raises `agent.pending`. Approving it closes every live connection of the old key, since that key no longer verifies, so a session held with a leaked key ends with the re-key.
- **An agent that lost its data folder** has no ID or key to show, so it pairs as a new agent. Its old record still holds the hostname: the hub says which agent that is, and the person removes it, which orphans its snapshots for the adoption of §7.3, then pairs with the token that is still unspent.
- **A standalone agent's agent ID is public** in the tags of its snapshots (§7.3), and it may never have been registered. A rogue with a stolen unused token could register under that ID with its own key. Approval is the defense: the hub's alert and the approval show the new agent's agent ID and key fingerprint, and if the person rejects the rogue, the real agent, refused with `token-used` or `agent-removed`, tells its person what to do. The decision doesn't pretend otherwise: a standalone agent's agent ID is not a secret.

## Pending approval

**Every newly paired agent is held pending until a person approves it on the hub.** It costs one step in a pairing the person is already making, and it ends the harm of a stolen token: the rogue appears in a list with its own key, and nothing it says is acted on.

- **What pending means.** The hub keeps the agent's proofs, `Hello`, snapshots and reports, and shows them on the pending row, but acts on none of them: no grants, claims or leases, no commands, no ownership decision, no effect on the prune guard, and a claim it makes doesn't make another agent's refused. Its hostname is fixed. It counts toward the limit of 20 pending agents; more pairings are refused (`pending-limit`) with the token unspent.
- **A pending agent lapses after 7 days.** The hub drops it, frees its hostname and keeps no tombstone, since nobody judged it. It keeps a lapsed marker for 30 days: the agent ID and key fingerprint, with no hold on the hostname. An agent that reconnects and proves its key, a laptop asleep for 8 days among them, gets a signed `pending-expired` that says to pair again with a new token under the same ID and key; any other caller gets `proof-refused`. Pairing again drops the marker. After the 30 days the marker is gone, and the agent, which knows it was last told `pending`, reads `proof-refused` as the same message. The pending row shows its age, so a person sees what is about to lapse.
- **The agent keeps working.** It runs its plans locally, is told `status: pending` in `Welcome`, shows the same fingerprint the hub shows, and says it waits for approval. It keeps the scheduled maintenance it runs unpaired ("Scope"). What it shows while it waits, and on approval, rejection, removal and expiry, is [decision 121](121-pairing-a-machine-with-its-hub.md).
- **Approval.** The hub shows the agent's hostname, agent ID and key fingerprint, and the agent shows the same; the person compares them, and approves or rejects. **Approval is a high-impact action:** on the web it needs a signed-in session and a fresh step-up ("Who may make a token, approve or re-key"). Approval sends the agent a signed `Welcome` again with `status: approved`. Rejecting a new agent removes it, keeps its tombstone and tells it (`Removed`); rejecting a pending replacement key discards only that key ("Pairing").
- **After approval** the hub evaluates what the agent reported, grants what the rules (§7.3) allow and sends signed `RepoGrants`. The approval records who and when.
- **A person can't miss a new agent.** The hub raises the `agent.pending` event (§11) and shows a notice on every page while any agent waits, and the web UI says so politely when one arrives; [decision 121](121-pairing-a-machine-with-its-hub.md) says how.
- **`ladderback pairing-token` in a terminal may wait for the pairing** and ask whether to approve it there. The user that owns the data folder needs no step-up, as for the command's other uses ([decision 121](121-pairing-a-machine-with-its-hub.md)).
- **A re-pair of an approved identity, or a replacement of its key,** follows "Pairing": the same ID and key stays approved; a new key waits beside the record.

## Who may make a token, approve or re-key

Making a token, approving a pending agent, making a replacement token, re-keying, resetting or rotating the hub key, re-pairing an agent and viewing or downloading the Recovery Kit are high-impact actions: a token authorizes a machine, approval gives it grants and commands, and the kit holds the root key. All are for a session only, never an API token, and have no MCP tool (§10.5). Revoking a token or holding an agent only removes authority and needs a session alone.

- **A signed-in session and a fresh step-up, every time** (§10.4): the password re-entered, its attempts sharing the sign-in waits, one step-up for each action. A session that is old or recent proves nothing here. The refusal is `step-up-failed`, as for a restore. The Recovery Kit's page and its Download are one action ("The recovery key"); its confirmation is another, with a step-up of its own.
- **Beyond loopback, sign-in must be on.** A request is beyond loopback when its `Host` isn't a loopback name or its peer isn't a loopback address. That test is a guard, not a proof: a proxy on the same machine that rewrites `Host` (nginx sends `Host: 127.0.0.1:9899` from 127.0.0.1 by default) makes a remote request look local, and the hub can't tell. With sign-in off or in proxy-header mode there is no password to re-enter, and the typed key ID that stands in for it only guards against misclicks (§10.4). Those requests are refused with `pairing-needs-sign-in`; the person sets an admin password. The Recovery Kit's page and Download are held to a stricter rule: they need sign-in to be on whether the request is on loopback or not (`kit-command-only`, "The recovery key").
- **On loopback with sign-in off,** the step-up is the typed key ID. On a hub it is the hub key's key ID, as for a restore; on an agent that pairs, it is the key ID of the hub key in the token being confirmed. Decision 99 trusts the person and the programs they run. With sign-in off the key ID is public (`Challenge` sends the hub key to anyone): it guards against a misclick, and §10.1 alone stops a malicious page. **It holds only with no proxy in front.** Behind a proxy that rewrites `Host`, a remote request passes for loopback and the public key ID is all it takes to make a token or approve an agent, so a hub behind a proxy turns sign-in on, and Settings and the docs say so.
- **The allowed hosts apply as always** (§10.1): a `Host` that isn't a configured name or loopback is refused before any route runs.
- **HTTPS can't be enforced by the daemon behind a proxy,** since forwarded headers are never read. Built-in TLS is HTTPS by construction. Otherwise Settings and the docs say that making or pairing over plain `http://` beyond loopback shows the token to the network, and the web UI, which can see `isSecureContext`, refuses to show or take a token on a page that isn't one. That protects a person from a mistake, not from an attacker, who needn't use the page.
- **An agent's pairing page refuses a hub address that is plain `http://` and not loopback,** whatever the page is served over.
- **The hub's command line is the exception to all of the above:** `ladderback pairing-token`, `hub-recover` and the approval prompt need no sign-in and no step-up, and run only as the user that owns the data folder, as `set-password` does (decision 99, decision 70). A request to the web UI never reaches them.

## A copied data folder (#296)

Two machines with one ID and one key can both prove themselves. The agent's own check is first: the machine identifier stored beside the ID, checked at start (the maintainer's decision on #296), which stops maintenance on the copy. The hub adds a second.

- **The connection counter.** Each agent keeps one in its data folder and writes it before each proof, atomically and with an fsync of the file and its folder, so a power loss can't roll it back. An agent that has an ID but no counter file starts at 0, like a restore from a backup.
- **One attempt in flight, under a lock.** The next value is taken under a process-wide lock and an advisory lock on the counter file, held for the whole attempt, so a reconnect loop and a "connect now" button, or two processes on one folder, can't each send the same value. A second attempt waits or gives up.
- **The hub compares and sets in one transaction.** It takes a counter above the one it holds, after the signature verifies, as `UPDATE … WHERE counter < ?`, so two proofs racing can't both win.
- **A reordered proof isn't a conflict.** The hub numbers the challenges it issues, and the numbers survive a restart (they are stored with the counters), so a held-up proof that arrives after a restart is still known to be late. A proof whose counter isn't above the stored one, for a challenge issued before the one whose proof set the stored counter, is refused as `stale-proof`: nothing is recorded and no alert is raised. This is a slow attempt arriving late, which a hostile proxy can cause at will, and it must not lock a laptop out. A counter that isn't above the stored one on a newer challenge is `identity-conflict`.
- **One live connection per agent ID.** A good proof for an ID that already has a live connection closes the older one, since a half-open connection after a network change looks the same. So the original and a copy that both stay up take turns, and the one behind is refused `identity-conflict` at its next proof. Without this rule both would hold sessions and leases. A proof under a pending replacement key is no connection of that ID: it takes no slot and closes nothing ("Pairing").
- **A conflict stays until a person resolves it.** The hub raises an alert naming the agent, refuses every proof for that ID with `identity-conflict`, stops granting and renewing its leases, and keeps its other state. An agent told so keeps backing up and runs no maintenance, as #296 decided. It doesn't clear when the counters happen to agree, and no token but a replacement one clears it.
- **Resolving it takes a replacement token** ("Pairing"). The real machine pairs with it and takes a new agent key under its old ID; the hub holds that key pending beside the record, with a counter of its own, and once a person approves it, swaps the key, adopts that counter and clears the conflict. The copy's key no longer verifies, whatever its counter, and it stays refused. If the person rejects the new key, the record and its conflict are as they were. A copy that should be a machine of its own takes a new ID ("this is a new machine", #296).
- **What this can't catch.** A disk image cloned with its machine identifier, whose original is switched off while the copy connects first, looks like one machine until the original returns; both then hold a lease for up to 7 days. The prune guard's other layers (§7.3, I3) and rustic's repository lock still apply. A data folder restored from a backup looks the same and gets the same remedy.

## Removing, re-keying and recovering an agent

- **Removing an agent** closes its connection with a final signed `Removed` message, keeps its tombstone and orphans its snapshots (§7.3). A removed agent that reconnects gets `proof-refused`, as any stranger does, and tells its person this hub may have removed it. It keeps backing up.
- **Re-keying an agent whose key leaked** doesn't mean removing it: the person makes a replacement token from the agent's row, the agent pairs with it and takes a new key under its old ID and history, and approval follows. Until the token is used the old key works; **holding** the agent in the meantime sets it back to pending.
- **A leaked hub key** is recovered by the recovery key, or by a reset ("The hub key").
- **A stolen backup of an agent's data folder** is a stolen identity. Re-key the agent, or remove it and pair a new one.

## What a hostile hub or agent can do

**A hostile hub, or a leaked `hub-key`** can sign anything the hub can. It is bounded, and the bounds are the point:

- It can run a plan now (rate limited above), cancel an operation, and from M9 request a restore, which the agent still refuses unless it sets `allow-hub-restore` and always writes into a new folder of the agent's choosing.
- It can't read a repository secret (they never cross the link, I4), run a command of its own, edit a plan (`UpdatePlan` is reserved), or restore anywhere else.
- **It can break I1** by granting two agents a lease on one repository. The hub is the arbiter; the other layers of §7.3 (other hosts' tagged snapshots, the deadline and the prune guard) are what remains.
- **An owner agent computes its deadlines from its own `keep-delete`,** never the grant's. `agent:<id>` is granted only where no other agent uses the repository (§7.3), and the hub holds no secret there, so a hostile hub that inflated `keep_delete` or `prune_margin` could make an owner's own backups outlive its own prune (I3). On a hub-owned repository the hub has the secret and the prune; the other agents take the grant's values, as §7.3 has them, trusting the hub with that repository.
- **It can pause maintenance, never backups.** A hub, or a holder of a leaked hub key, can send a `Welcome` whose `signed_at` is 25 hours off, and the agent then runs no scheduled prune or check as `clock-skew` ("Time"); a copy of an agent's data folder can raise `identity-conflict`, which stops lease renewal from M9. An agent keeps backing up in both. The person sees the message, and a corrected clock, a recovery or a reset ends it. A grant a leaked key signs can also raise a machine's lease floor to the maximum, and a recovery handover outranks it ([decision 137](137-a-hub-moves-by-handover-never-by-copy.md)).

**A hostile or rogue agent** is a pending one until approved, so what it can do is bounded by that and by approval's care:

- A pending agent changes nothing. An approved one that turns hostile can claim a repository first, report a repository so the hub stops renewing another's lease, and report running operations. **The hub caps any reported `deadline` at `index_read_at + keep_delete`** and `index_read_at` at the hub's clock, so a rogue can't hold the prune guard for ever. The rest is the remedy of "Removing, re-keying and recovering an agent".
- A rogue can't forge another agent: it has no key for it.
- A removed or decommissioned agent can't pause the hub with an old grant. Only a machine with no record and no tombstone puts a hub in recovery mode by offering a grant the hub signed, and the hub answers it as it answers any unknown machine ([decision 137](137-a-hub-moves-by-handover-never-by-copy.md)).

## Why not the alternatives

| Alternative | What it costs | Why not |
|---|---|---|
| **Mutual TLS** with a client certificate per agent | Issuing and renewing a certificate per agent, which pinned self-signed client certificates would keep small. A reverse proxy ends TLS and passes the certificate on only as a header, which §10.1 never trusts (forwarded headers are never read). | The design must work behind a proxy, and there the client certificate never reaches the hub, so the hub still needs an application-level identity. The cost isn't what rules it out; the proxies are |
| **A required WireGuard or Tailscale tunnel** | A kernel module or a daemon on every machine, administrator rights, key distribution (or an account at a service) for each one, and an address that is no identity. | The hub still has to know which agent is which inside the tunnel, so it still needs this decision, and a required tunnel keeps out everyone who can't install one |
| **Peer-to-peer** | NAT traversal, discovery and relays, so that agents can reach each other. | Agents never talk to one another: one hub is the registry and the one arbiter of maintenance (I1). The cost buys nothing |
| **The bare nonce, signed** (§7.1's first draft) | Nothing, and so a signature that can be made for another hub, another agent or another message. | A signature over a bare nonce isn't tied to anything: it is the hand-rolled piece the maintainer asked to see closed |
| **PASETO v4.public** (or JWT) for the signed messages | A crate (`pasetors`) and a token with a header and JSON claims. PASETO has no algorithm field and signs the raw bytes with a pre-authentication encoding, so the usual JWT reasons (algorithm confusion, a JSON canonical form) don't apply to it. | It would carry the fields this decision already encodes in 20 lines, and its implicit assertions would be the nonces and the binding we put in the signed bytes anyway. Our signed bytes are fixed-width apart from the hostname, the message kind and a refusal code, which each have a length byte, and need no parser. The reason is need, not the crate's record, which wasn't weighed |
| **Noise `KK`** with `snow` | See "Noise KK, weighed" below. | Safer in two ways, simpler in two, and not at equal cost; kept as a possible later handshake |
| **One key for the TLS certificate and the hub** | Behind a proxy the certificate isn't the hub's, so the hub key must exist anyway; and the TLS key is read by the TLS library. | One more reason for the key to leak, with no gain |
| **RFC 7250 raw public keys in place of a certificate** | rustls supports them, which would drop the certificate generator. | A browser can't use one, and the built-in listener also serves the web UI |
| **Encrypting every message after the handshake** | A session key from an authenticated key exchange (X25519), which has a key schedule to design and review. | Signing needs no key exchange and covers what matters: what moves an agent or the hub. What is left is status the link carries in the clear, with no secret (I4). Under "What this doesn't defend" |
| **A WireGuard-style pre-shared key per agent,** mixed into the handshake | A secret per agent that the hub must keep, pair, rotate, recover and put in the Recovery Kit | **Not chosen,** for three reasons. Its main benefit, protecting recorded traffic from a future quantum computer, comes from the hybrid key exchange ("Post-quantum key exchange for agent–hub connections") with no extra secret. It would put a per-agent secret in the hub's database, which holds only public halves today, so a stolen database or backup would hand over every agent's key. And it adds a secret to pair, rotate, recover and print. It would add something only against a future quantum forgery of signatures, an active attack that a move to a post-quantum signature answers |
| **A short typed code by a PAKE** (SPAKE2, CPace), like `7-purple-lantern` | A PAKE crate, a handshake step to design and review, and a code short enough to guess unless attempts are limited. | **Not in the first version; a later option.** Conditions: a well-reviewed crate (not our own protocol), few attempts per code and then it dies, a short life, and the pinned hub key resting on the PAKE (the agent learns the hub key from the exchange, so the exchange must authenticate it). The QR and the text cover the first version |
| **The agent shows a code, and the person approves it on the hub from a phone** (RFC 8628, started by the agent) | An unauthenticated request path on the hub that any host can start, and a code a person copies between two screens. | **Not chosen.** The agent learns the hub's key on first contact, from whoever answers at the address, so a host in the middle is trusted unless the person compares fingerprints. It would be safe with a short authentication string compared on both screens, a rate limit, a short life and the same step-up: a PAKE-like design, so it waits with the PAKE. Pending approval (above) gives the person's approval on the hub without it |

### Noise KK, weighed

Noise `KK` (through `snow`) pins both static keys at pairing and runs end to end inside the WebSocket, so a proxy sees and can change nothing. It would replace the per-message signatures and the TLS binding. It is not chosen, because it is not strictly safer at equal cost.

- **What it gives.** Forward secrecy, confidentiality of every byte, integrity with no envelope, expiry or clock logic, and no binding kinds. Confidentiality isn't nothing here: no secret crosses the link (I4), but logs, paths, hook command lines and snapshot names do, and a hostile proxy reads them today ("What this doesn't defend").
- **`snow` 0.10.0 says of itself** that it "has not received any formal audit" (its README, read 2026-11-18). It can use `ring` for its primitives, which `Cargo.lock` already holds, so the cost isn't a pile of new packages. It is a second crypto provider beside `aws-lc-rs`, which the one-provider rule of "Crates" avoids, and a protocol with no audit.
- **A second key type.** Noise statics are X25519. Ed25519 stays for the pairing token, the handover, the recovery key and a replacement, which are all signatures, so each identity holds two keys.
- **Pairing doesn't fit `KK`.** The hub doesn't yet know the agent's key, so a second pattern (`XK`) is needed. The token can't be a pre-shared key: the hub holds only its public half, and a pre-shared key is a secret the hub would have to keep. The token key still signs inside the payload.
- **The first message is replayable and its sender authentication is open to key-compromise impersonation** (the Noise specification's payload properties for the `es` and `ss` tokens, §7.7, read 2026-11-18). Nothing in it may act.
- **Everything else stays:** pending approval, counters and conflicts, tombstones, recovery, the transport statement, command IDs and ages (a session can't say how old a queued command is), and a framing layer, since a Noise message is at most 65,535 bytes.

It does simplify: no signature per message, no clock logic, no binding. Revisit it as a `/sync/v2` handshake, by a decision of its own, if the link ever carries a secret, or if `snow` is audited. The rule that a handshake never changes in place allows it.

## What this doesn't defend

- **A hostile proxy** can read what agents and the hub send, since payloads aren't encrypted, delay it and cut the connection. It can't make an agent or the hub act on anything altered or invented, since every message is signed, and it can't drop one signed message and let later ones through, since the sequence has no gaps. A dropped tail (a cut connection) looks like a lost connection, and I1 and I3 then rest on §7.3's other layers (the tags of other agents' snapshots, the deadline and the prune guard), which are unchanged. Run built-in TLS, or a tunnel, if the proxy isn't yours.
- **Denial of service before the proofs.** Anyone who can reach the hub can open connections. The `Connect` deadline, the eviction of the oldest unauthenticated connection and the per-address cap in built-in mode keep honest agents connecting; behind a proxy every connection has the proxy's address, so the proxy's own rate limits are the defense.
- **A leaked hub key** between its leak and its recovery or reset; **a leaked agent key** until the agent is re-keyed or removed.
- **A quantum computer.** A recorded pinned session can't be decrypted later, since only the hybrid key exchange is offered on it; a session toward a proxy is as safe as the proxy's TLS. With built-in TLS the Recovery Kit is sent only on a hybrid session. **Left over for a proxied hub:** the admin password, the step-ups and the kit page travel over whatever the proxy and the browser negotiate, so a recording of a classical session could in time give a quantum attacker the password and the root seed; a person who wants none of that exposure runs `ladderback recovery-kit` on the hub's machine. A computer that can forge Ed25519 could act as the hub or an agent, an active attack that no recording enables and this decision doesn't defend; the answer is a new handshake with a post-quantum or hybrid signature ("Post-quantum key exchange for agent–hub connections").
- **What the UI shows, and I4.** I4 keeps repository secrets off the agent–hub link; it doesn't forbid showing a person the secrets they must hold. The pairing token ("Pairing") and the recovery seed ("The recovery key") are shown once, to the person, after a step-up, as decision 89's generated password is. Neither is a repository secret, and neither crosses the link: the token's seed never leaves the hub's page, and the recovery seed is shown only to the person.
- **A person's own account.** A program running as the person reads the keys (decision 99).
- **The clock.** A hub or agent clock far off: `Hello` warns above 5 minutes (§7.2), and above 24 hours an agent runs no scheduled maintenance.
- **The tombstone and approval lists** hold identifiers, not secrets; whoever reads the hub's database learns the agents' public keys (I4: nothing more).

## Versions: how decision 88's window holds

Decision 88 is binding on this one: hub and agent releases that differ by a protocol keep working, and a mismatch never stops local backups (items 2 and 6). Each new rule is checked against it.

- **The handshake family is the same in every protocol.** `Connect`, `Challenge`, `Proof`, `Pair`, `Paired`, `Hello`, `Welcome`, the envelope (`kind`, `seq`, `expires_at`, `payload`, `signature`) and the encoding are defined here and changed only by a new handshake path and a decision (88, as amended). The fields this decision adds to them (`binding_kind`, `hub_handshakes`, `generation`, `signed_at`, `hub_version`, `status`, the recovery key) are in the handshake from the first release, which no hub has shipped; a reader of any protocol ignores a field of this family it doesn't know.
- **A hub proves the handshakes it speaks, in its proof.** `hub_handshakes`, a set of handshake numbers, is among the signed fields of `hub-proof` and so in every `Challenge`. An agent that speaks a newer handshake than a hub tries the newer path first and falls back on a 404, as it must, since an older hub and a proxy answer 404 alike. It never trusts what the 404 seems to say; it checks it against the hub's signed list.
- **The key exchange of a pinned connection is part of `/sync/v1`.** The group set a kind 1 connection offers and accepts, `X25519MLKEM768` alone, changes only with a new path and a decision, as anything else a path fixes does (88, item 6). A release that dropped the hybrid inside `/sync/v1` would break its N−1 pinned agents at the TLS layer, before `Connect`, with no "out of date" message. A later group set is offered on `/sync/v2`, and a hub keeps accepting the old one on `/sync/v1` while it speaks it.
- **A hub reinstalled as an older release keeps its agents syncing, with no click and no re-pairing** (decision 88, item 6 and the amendment). The agent falls back to the older path, the hub proves itself under the key the agent pinned, and its signed list says it speaks only that handshake. The agent proceeds on the handshake the hub proved, backs up as always, and shows in its status which hub release it now sees. No proof or counter is held back, so nothing can look like a copy, and no identity conflict follows. Which protocol they share is `Hello`'s and `Welcome`'s business as ever; with none in common, item 6 applies.
- **A 404 that the hub's signed list contradicts is refused.** If the agent fell back after a 404 and the hub's proof lists the newer handshake, something between them hid it. The agent closes, sends no `Proof`, raises `handshake-hidden` and tries again later. No setting lowers this. A decision that retires an unsafe handshake adds a signed minimum to the hub's proof, which an agent won't go below; with one handshake today there is nothing to retire.
- **Pairing needs a release that speaks this handshake, not the hub's release.** The token's version byte and `Pair` belong to the handshake family. A token version this agent doesn't know is `token-unsupported` and the person is told to update the agent; a hub makes the lowest token version that carries what it needs. After `Paired`, `Hello` and `Welcome` negotiate the protocol as usual, and an agent that shares none is paired, pending or approved, and "out of date" as 88 says.
- **New messages are additive and fail safe.** A peer that doesn't know a message kind verifies its signature, checks its sequence number and its expiry, counts it, ignores it and logs it once; it never closes the connection for a kind's sake. So a newer hub that sends a kind within a protocol doesn't send an older agent into `sequence-error`. `ReleaseAck` missing means the hub waits for the lease to lapse (the safe direction, §7.3). `TransportStatement` missing means an agent keeps its token's address, and `TransportAck` missing leaves the agent listed as unconfirmed. Approval and tombstones are hub state an agent only sees as `status` and a refusal. None changes what a lease, a grant, a deadline or ownership means, so none needs a new protocol (item 9).
- **A signature covers the payload's bytes, not its fields.** The digest is over the bytes the sender wrote, and a receiver parses them only after the signature holds. A payload gaining an optional field within a protocol (88, item 1) therefore verifies on the protocol's older reader, which ignores the field; nothing is re-encoded on the way. The signed `expires_at`, `seq` and `signed_at` are in the envelope of every protocol.
- **Fields an agent relies on for safety can't be optional.** `command_id` and `requested_at` belong to a command's definition, and an agent refuses a command without them rather than ignore them. The hub's cap on a reported deadline and an owner's use of its own `keep-delete` are local rules and need no peer's cooperation.
- **Meaning stays under item 9.** The counter, the pending state and the clock rules change no lease or grant semantics. `Heartbeat` and `ReposSnapshot` fields keep theirs.
- **An agent's data folder read by an older agent** works: `hub-pin` is read as text and unknown names are kept; the counter file and agent key file are unchanged.

## Crates

The maintainer pre-approved security crates on 2026-11-18, to be chosen for review and track record, and said to choose the most cautious option. None is added by this decision; the implementation PR (#567) adds them, with `just deny` run on the result.

### Ed25519: `aws-lc-rs`, not `ed25519-dalek`

`aws-lc-rs` 1.19.0 (`aws-lc-sys` 0.47.0) is already in the tree, linked for TLS, and adds no new crypto code. `ed25519-dalek` 3.0.0 is four months old; its 2.2 line has the longer record, and it needs `verify_strict` to refuse what it refuses. What I read and ran, in `aws-lc-sys` 0.47.0's source, `curve25519.c`, `curve25519_nohw.c` and the `s2n-bignum` headers:

| Property | Result | How known |
|---|---|---|
| A signature whose `S` is not below the group order, or whose top three bits are set, is refused (malleability) | Refused | **Verified** in the source (`ed25519_verify_internal`: the `kOrder` loop and the `signature[63] & 224` check), and **ran**: `S + L` was refused |
| The signature's `R` must re-encode to the same bytes | Refused otherwise | **Verified** in the source: the computed `R` is compared byte for byte, so a non-canonical `R` can't match |
| A public key whose `y` is not below `p`, or whose `x = 0` carries a sign bit, is refused | Refused | **Verified** for the C path (`x25519_ge_frombytes_vartime`); the assembly path that x86-64 and AArch64 builds use (`edwards25519_decode`, in `s2n-bignum`) **documents** the same refusals in its header, and its body wasn't read [INFERENCE: it implements them]; **ran** on this machine's AArch64 build: `y = p + 1` was refused |
| A small-order public key is refused | **Not refused** | **Verified** in the source (no check anywhere) and **ran**: the signature `R = identity, S = 0` verified under the key `01 00…00` for the one message tried, as it does for every message under the identity. Under the other seven small-order keys such a forgery verifies only when the message's hash is a multiple of the point's order, so one run of one message refused six of them and accepted `26e8…fc85` by that chance; a run is no proof of refusal |
| A small-order `R` is refused | Not refused | **Verified** in the source. Harmless: only the single, cofactorless check is used, and a small-order `R` can't satisfy it for an honest key |
| Key memory is cleansed when freed | Yes | **Verified**: `OPENSSL_free` calls `OPENSSL_cleanse` on the whole allocation (`crypto/mem.c`) for the default allocator |

So `aws-lc-rs` is strict on every encoding but not on small-order public keys. The protocol closes that gap in two ways. `domain` refuses, before any key is stored or verified with, the eight encodings of the curve's small-order points: `0100…00`, `ecff…ff7f`, `0000…00`, `0000…0080`, `c7176a70…037a`, `c7176a70…03fa`, `26e8958f…fc05` and `26e8958f…fc85` (I computed each point's order with a short script, and the tests carry the full values). It also refuses any encoding whose `y` is not below `p`, one comparison, so the protocol doesn't rest on the assembly body I didn't read.

A small-order key is also harmless in this protocol, even if one slipped in. The reason isn't the fingerprint comparison, since a small-order key shows the same fingerprint on both screens. It is that each key is chosen by the party it identifies, or by one that party authorized, so a weak key harms only its own holder: the hub's key is the hub's, an agent's is the agent's, the pairing key is the hub's and the recovery key is the person's. That is a table of eight constants and a comparison, not crypto code. If a later review wants a library that does it, `ed25519-dalek` 2.2 with `verify_strict` is the swap. The seed comes from the `Random` port into `Ed25519KeyPair::from_seed_unchecked`, held in a `Zeroizing` buffer meanwhile.

### Other crates

| Crate | Version | Used for | Alternative, and why not |
|---|---|---|---|
| `aws-lc-rs` | 1.19.0 as a direct dependency (already in the tree); the features `rustls` builds it with, no `fips` | Ed25519 signing and verification, the one crypto provider for TLS too | `ed25519-dalek` above |
| `rustls` | 0.23, a direct dependency with default features off and `aws_lc_rs`, `std` on, **one provider: aws-lc-rs** | TLS 1.3 only, the hybrid key exchange, the pinned-key verifier (which checks `CertificateVerify` and ignores dates and names), the exporter value | None. The lock file has rustls built with both `ring` and `aws_lc_rs`, so no default provider can be chosen from features: every `ClientConfig` and `ServerConfig` is built with `builder_with_provider` on `aws_lc_rs::default_provider()` with its `kx_groups` set explicitly as "Post-quantum key exchange for agent–hub connections" says, and `with_protocol_versions(&[&rustls::version::TLS13])`, never `builder()`; a clippy `disallowed-methods` entry bans `ClientConfig::builder` and `ServerConfig::builder`, and no process default is installed |
| `tokio-rustls` | 0.26 | The server and the client over `tokio`, with the connection in reach for the exporter | `axum-server`, a new package, with less control over the connection |
| `rustls-platform-verifier` | 0.7.1, the version already in the tree through `reqwest`, a direct dependency of `tls-adapter` (approved by the maintainer 2026-11-19); `Verifier::new` takes the same provider | The agent's CA validation of a proxy's certificate, with the operating system's trust store | `webpki-roots` (a fixed list, no private CA) or `rustls-native-certs` |
| `rcgen` | 0.14 (`default-features = false`, feature `aws_lc_rs`, to match the provider; no `pem` or `x509-parser`). Brings `yasna`; `time` and `rustls-pki-types` are in the tree | Making the built-in listener's self-signed certificate once and again from the same key (ECDSA P-256, which every browser takes). Maintained by the rustls organization | Writing the ASN.1 by hand, `openssl` the program, or the person bringing a certificate (that is the proxy mode) |
| `tokio-tungstenite` | 0.29.0, the version `axum`'s `ws` uses, so a build has one copy; no TLS feature. The workspace pin moves with `axum` | The agent's WebSocket client over a stream `tokio-rustls` made, which stays in reach through `get_ref()` | `tokio-tungstenite`'s own `rustls` feature would also expose it, but picks its own provider and verifier; the newer 0.30 would leave two copies of the crate in a normal build |
| `unicode-normalization` | The version already in the tree (through `idna`) | Comparing hostnames in NFC, so "café" typed two ways is one name | Comparing bytes, which lets two forms of one name pair at once |
| `unicode-security` | 0.1.3 (MIT or Apache-2.0, from the `unicode-rs` organization), in the tree in `domain` since #1065, approved by the maintainer on 2026-11-19, and `just deny` passes with it. `no_std`, no unsafe code, Unicode 18.0's tables; it brings `unicode-script` 0.5.9 (MIT or Apache-2.0, `unicode-rs`, no dependencies) and the `unicode-normalization` already in the tree | The UTS #39 skeleton, so the hub flags a hostname that looks like another agent's or a tombstone's | Leaving it out: a lookalike then shows only the fingerprint comparison and the per-name note on non-ASCII names ([decision 121](121-pairing-a-machine-with-its-hub.md)), which catch it less often |

Already in the tree and used as they are: `sha2` (the payload digest, the public-key fingerprint, the token checksum), `base64`, `uuid` (v7), `axum` (`ws`) and `zeroize`. No HMAC crate is needed: the token's key signs, so no secret is stored on the hub or sent. The implementation PR extends `technology.md` §8.2's crypto row to name the crates above. The crates for QR codes and the camera belong to [decision 121](121-pairing-a-machine-with-its-hub.md).

**Where it lives.** The TLS crates stay in `adapters/tls` (`tls-adapter`), behind the `TlsKeys` port, which makes the hub's ECDSA P-256 key and gives its fingerprint; `app::hub_tls` takes the key at the hub's start and refuses one that is gone or another than the fingerprint recorded. The certificate is made again at each start, valid from the start of the day before `now`, so a client whose clock is a little behind still takes it, until the start of the day 397 days after `now`'s, with a random serial number, since certificates of one issuer can't share one (RFC 5280). The agent's pinned client turns off session resumption as well as early data, so every connection runs a full handshake that the verifier sees. The verifier refuses a chain that carries any intermediate certificate, takes `ECDSA_NISTP256_SHA256` alone for `CertificateVerify`, and refuses every TLS 1.2 signature.

## Tests that pin it

Each is a test that fails before the behavior exists (§13.1).

- **The messages.** A signature over the bare nonce, hub's or agent's, is refused. A proof made for another hub key or another agent ID is refused. A signature of one label presented as another, the hub proof as the agent proof and the reverse, is refused. A proof from an earlier connection (other nonces) is refused. Another binding value or kind is refused. The encoding has fixed vectors, so changing it is a decision, not an accident.
- **Ed25519.** A non-canonical `S`, a public key whose `y` is not below `p` (which `domain` refuses as well as the library) and each of the eight small-order keys are refused when `domain` parses a key (`PublicKey`), so none reaches the `Signing` port's verify, with fixed vectors; a forged `R = identity, S = 0` under the identity key is refused.
- **Signed messages, both directions.** An unsigned, a wrongly signed, an expired, a replayed, a skipped (a gap) and an out-of-order message are each refused, change nothing and close the connection. So are one signed for another agent or connection, one whose payload was altered, one with a padded or non-canonical encoding, and a `RunPlan` signature on a `RepoGrants` payload. A forged message doesn't advance the sequence. An unsigned or altered `PlansSnapshot` (retention rewritten to keep-last 1), `OpEvent` result, `Ack`, `ReposSnapshot`, `Heartbeat`, `Hello` or `ReleaseAck` is refused and changes no plan, outbox, lease or guard. The hub's forget never uses a plan it didn't receive signed.
- **Unknown kinds.** A validly signed message of a kind the receiver doesn't know is ignored and logged once, uses up its sequence number so the next message is accepted, and is refused if it has expired; one with a bad signature is refused like any other.
- **Refusals.** A refusal the hub signed under its key, both nonces and the binding is acted on. An unsigned one, one signed for another connection and one under the wrong key is a dropped connection: the agent tells its person nothing and stops no maintenance, so a proxy that sends `identity-conflict` changes nothing.
- **Time.** An agent whose clock is 8 minutes slow connects, warns and accepts a fresh message; a message expired by the hub's time is refused whatever the agent's clock says; an offset above 24 hours stops scheduled maintenance without stopping backups.
- **The hub's identity.** An agent refuses a hub whose key isn't the pinned one, and sends no proof and no `Hello`. A valid handover chain is followed, `Welcome` is checked against the key the chain reached, and the new pin is kept only after it; a broken, forged or over-long chain is refused. A server that presents the pinned certificate without its private key is refused. The pinned verifier accepts a lapsed or differently named certificate with the pinned key.
- **The key's life.** A hub that starts with its key missing and agents registered refuses `/sync/v1` and makes no new key; it makes one only on a first start. A rotation interrupted after each of its steps restarts and finishes or reverts, never forks the chain; a second handover from one key is refused; unused tokens are void after it.
- **Recovery.** `hub-recover` takes the seed only at a prompt with no echo or on standard input, and refuses one on the command line; it reads a seed in any case, with or without hyphens, spaces and line breaks, and with Crockford's aliases. It derives the public key from the seed, shows its fingerprint and compares it with `hub-recovery-key` before it changes anything, so a mistyped seed changes nothing. Afterwards the hub has a new hub key and a new recovery key and shows the new seed.
- **Following a recovery.** An agent follows a recovery handover from its pin, from a leaked key it was led onto and after a statement numbered at the maximum; it needs only its pinned recovery key; a handover whose number isn't above the highest followed is ignored. `hub-recovery-key` survives `db rebuild`; the seed file is left out of a plan's backup by default and is deleted only by a confirmation that typed back the group of the seed it asked for.
- **The Recovery Kit's exposure.** The kit page and its Download are refused with `kit-command-only` without sign-in on (also on loopback), without a fresh step-up, 7 days after setup or the last recovery, and on a built-in TLS session that didn't negotiate the hybrid; then only `ladderback recovery-kit` shows the seed. Each view, in the web page, a download or the command, raises `hub.recovery_kit_viewed` naming the way, the user and the peer address and never the seed, and Settings shows the last. The step-up that opened the kit covers its Download and ends after 10 minutes or on leaving the kit's pages; the confirmation takes a step-up of its own.
- **The confirmation.** It asks for a group picked at random when its page loads, with the group's number shown, accepts a pasted group in any case with Crockford's aliases, is a field apart from the step-up, is on a page that holds none of the seed, and works after the 7 days too.
- **Generation and addresses.** An agent refuses a `Welcome` or a `RepoGrants` with a generation below the one it holds for that key, from the first release, as `generation-lower`, and keeps backing up; following a recovery handover starts the floor afresh and an ordinary handover carries it. After a `Welcome` at the maximum generation from a leaked key, the real hub stays refused until a recovery handover is followed. A transport statement replaces the list only when its (generation, number) is higher; one numbered at the maximum doesn't remove the token's address or one the person set; after a recovery handover the list and its number are gone.
- **Key exchange.** A pinned-address agent offers only `X25519MLKEM768` and refuses a server that selects another group. A hub closes a kind 1 `Connect` on a session that negotiated X25519 with `binding-unavailable`, so a pinned peer that offers only X25519 is refused. A CA-token agent offers the hybrid first and X25519 after it, and connects to a proxy that speaks only X25519. A hub with built-in TLS refuses the kit page on an X25519 session. Every config is built from the one provider, with `kx_groups` set and TLS 1.3, which the lint ban on `builder()` pins.
- **Downgrade.** A `Welcome` with a lowered `protocol`, or an echo of a range the agent didn't send, is refused. A hub whose proof lists only an older handshake is connected to on it, with no click, no counter held back and no conflict. A 404 on the newer path that the hub's signed list contradicts is refused and raises `handshake-hidden`. A hub whose key isn't the pinned one is refused whatever it lists.
- **Binding.** Built-in mode: a relay between two TLS sessions is refused on both sides. A pinned-address agent signs kind 1 and refuses a hub proof under kind 0; a CA-token agent signs kind 0; the hub signs the kind named in `Connect` and refuses a kind outside the agent's recorded set; a token's TLS byte is stored and enforced at `Pair`. A hub serves a kind 0 agent and a kind 1 agent side by side, and one agent with both.
- **Pairing.** A token used twice, an expired one, a revoked one and one with a wrong checksum are refused; two pairings with one token at once give one success and one `token-used`. A `Pair` repeated with the same token, agent ID and key (a lost `Paired`) is answered again; with another key it is `token-used`. A taken hostname leaves the token unspent, and so does a name that differs only by case or by Unicode normalization; a name with the same `skeleton(casefold(NFC(name)))` as another's, a Cyrillic "lаptop" beside "Laptop", is flagged on the pending row.
- **Pairing, identities.** A known ID with another key is refused, except under a replacement token for that ID; a tombstoned ID is refused under an ordinary token, with a code that names the remedy (`agent-removed`). A wrong pairing key, a wrong agent key, or a mismatch of the token's hub key with the hub's, is refused, and the agent sends nothing of the token to a hub it didn't verify. Unused tokens list and revoke; a rotation voids them.
- **Pending.** A pending agent is granted nothing, its claim doesn't refuse another's, its deadline doesn't delay a prune, and it is sent no command (from M9); approving it applies what it reported. It can't rename itself. Approval is refused without a fresh step-up. More than 20 pending agents are refused, a pending replacement key among them. One not approved within 7 days is dropped with its hostname freed and no tombstone, and leaves a marker for 30 days: a good proof from it is answered with a signed `pending-expired`, one from a stranger with `proof-refused`, and pairing again drops the marker. A pending replacement key lapses the same way and leaves the record as it was.
- **Copies.** A counter equal to or below the stored one, after a good signature on a challenge newer than the one that set it, is `identity-conflict`, raises the alert and stops lease renewal (from M9); the same proof on an older challenge is `stale-proof` and changes nothing, also after the hub restarts, since challenge numbers are stored. Two attempts at once in one agent send two different counters or one waits, and the hub's compare-and-set lets one win. A copy connecting while the original is connected closes the original's session.
- **Resolving a conflict.** A conflict stays after the counters agree, and an ordinary token neither lowers the counter nor clears it. A replacement token's new key waits beside the record with a counter of its own; the old key keeps verifying, or stays refused if the record was in conflict, and a stranger who proves under the new key with a high counter closes no session and raises no conflict. Approval swaps the key, adopts its counter, clears the conflict, refuses the copy and closes every live connection of the old key; rejection discards only the new key and keeps the record, its approval and its conflict.
- **Commands.** A command older than 15 minutes, one whose `requested_at` is more than 15 minutes ahead, one without `command_id` or `requested_at`, and a command delivered again are refused or answered from the recorded result and never run twice; `RunPlan` beyond its rate is `rate-limited`. These run from M9.
- **A hostile hub.** A grant's `keep_delete` doesn't change an owner agent's deadline; a reported deadline beyond `index_read_at + keep_delete` is capped.
- **Who may pair.** Making a token, approving and pairing are refused without a session, with an API token, and without a fresh step-up (a session's age doesn't count). Beyond loopback they are refused with sign-in off or in proxy-header mode (`pairing-needs-sign-in`), by `Host` as well as by peer. The page refuses to show or take a token on a page that isn't a secure context.
- **Bounds and secrets.** An oversized frame, a `Connect` later than 2 s and a handshake later than 15 s close the connection; a full table evicts the oldest unauthenticated connection and accepts the new one; a fifth unauthenticated connection from one address in built-in mode evicts that address's oldest and is accepted; a 65th connection at the front door closes the oldest there, and one already handed on doesn't count. TLS early data is off on both ends. A canary repository password appears in no frame (decision 3, I4). A plan over Ladderback's data folder leaves out `hub-key`, `hub-tls-key`, `hub-recovery-seed` and the agent key by default.
- **Versions.** An agent and a hub one protocol apart, paired, keep syncing. A hub reinstalled as an older release keeps its agents syncing, with no click. Pairing works across releases that speak the handshake. An unknown message kind is ignored after its signature verifies and doesn't break the sequence; an unknown field of the handshake family is ignored; a newer `hub-pin` is read by an older agent.
- **Where they run.** The encoding and the checks in `domain` and `app` with fakes; the signing port has a contract suite over its fake and its real adapter; the TLS binding and the pairing run `@e2e` against a real listener.

## Appendix: the scenarios these files take

Both Rust runners fail a scenario with an undefined step (`fail_on_skipped`, §13.1), so a feature file can't be committed ahead of its steps. Appendix A is `fleet/pairing.feature`, and each of its scenarios needs nothing beyond #567's slice: the registry, the handshake, tokens, approval, tombstones, counters and recovery. #567's red `test:` commit adds it with these as its first scenarios, and decision 121's appendix as the next. What needs #568's snapshots, such as what a pending agent reports and what the hub receives when an agent syncs, or M9's leases and commands is in Appendices B and C, for the files of those milestones. The version scenarios run with a hub whose protocol range and handshake list are test parameters, so they need no second protocol or handshake to exist. The protocol's own refusals (a bare nonce, a proof for another hub) are unit tests above, not scenarios.

### Appendix A: `fleet/pairing.feature` (#567)

```gherkin
Feature: Pairing an agent with a hub

  Scenario: An agent pairs with a hub and waits for approval
    Given a hub with a pairing token
    When an agent pairs with the token
    Then the hub lists the agent under its hostname as waiting for approval
    And the token can't be used again
    And the agent keeps running its plans
    And the agent keeps the scheduled maintenance it runs unpaired

  Scenario: A person approves a pending agent
    Given an agent waiting for approval
    When the person compares its key fingerprint and approves it
    Then the hub lists the agent as approved
    And the agent says it is approved

  Scenario: A pending agent can't rename itself
    Given an agent waiting for approval under the hostname "laptop"
    When the agent reports the hostname "desktop"
    Then the hub keeps the agent as "laptop"
    And "desktop" is free for another agent

  Scenario: A person rejects a pending agent
    Given an agent waiting for approval
    When the person rejects it
    Then the hub no longer lists it
    And its agent ID can't be used again with an ordinary token

  Scenario: A pending agent that nobody approves is dropped after seven days
    Given an agent that has waited for approval for seven days
    Then the hub drops it and frees its hostname
    And the agent says its pairing lapsed and to pair again
    And the agent's ID is not a tombstone

  Scenario: Approving an agent needs the person's password
    Given an agent waiting for approval
    When the person approves it without confirming with their password
    Then the hub refuses until they do

  Scenario: A token that has lapsed is refused
    Given a hub with a pairing token that has lapsed
    When an agent pairs with the token
    Then the pairing is refused as expired
    And the hub lists no agent

  Scenario: A token that was revoked is refused
    Given a hub with a pairing token
    When the person revokes the token
    And an agent pairs with it
    Then the pairing is refused as revoked

  Scenario: A key rotation voids unused tokens
    Given a hub with a pairing token
    When the hub changes its key
    Then the token can't be used

  Scenario: A pairing whose answer was lost can be repeated
    Given an agent that paired but never received the hub's answer
    When it pairs again with the same token
    Then the hub answers it as before and lists one agent

  Scenario: A hostname that is taken is refused and the token stays good
    Given an agent paired under the hostname "laptop"
    And a pairing token
    When a second agent named "laptop" pairs with the token
    Then the pairing is refused because the hostname is taken
    And the refusal names the agent that holds it
    When the second agent is renamed "desktop" and pairs with the same token
    Then the hub lists both agents

  Scenario: A name that differs only by normalization is taken
    Given an agent paired under the hostname "café" in one Unicode form
    When another agent pairs under "café" in the other form
    Then the pairing is refused because the hostname is taken

  Scenario: A lookalike hostname is flagged for the person
    Given an agent paired under the hostname "laptop"
    When another agent pairs as "lаptop" with a Cyrillic letter
    Then the hub lists it as waiting for approval
    And the hub says the name looks like "laptop"

  Scenario: An agent paired again keeps its record
    Given an agent paired with the hub
    And a new pairing token
    When the agent pairs again
    Then the hub lists one agent, with its history and its approval

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

  Scenario: A removed agent's ID can't be reused with an ordinary token
    Given an agent that the hub removed
    When another machine pairs with an ordinary token under its ID
    Then the pairing is refused because the ID was removed
    And the agent is told to ask for a replacement token
    When the person makes a replacement token for that ID and the machine pairs with it
    Then the machine waits for approval

  Scenario: A replacement key waits beside the agent's record
    Given an approved agent
    When the person makes a replacement token for it and another machine uses it first
    Then the agent's own key still connects
    And the hub lists the new key as waiting for approval beside the agent

  Scenario: Rejecting a replacement key keeps the agent
    Given an approved agent with a replacement key waiting
    When the person rejects the new key
    Then the agent's own key still connects
    And the agent's record and approval are unchanged

  Scenario: A copied data folder is told apart from the original
    Given an agent paired with the hub
    And a second machine has a copy of its data folder
    When both machines connect to the hub
    Then the hub refuses the one whose connection count is behind
    And the hub raises an alert naming the agent
    And the refused machine keeps backing up and runs no maintenance

  Scenario: A copy made while the original is connected is caught
    Given an agent connected to its hub
    And a second machine has a copy of its data folder
    When the second machine connects to the hub
    Then the original's connection is closed
    And the original is refused when it connects again
    And the hub raises an alert naming the agent

  Scenario: A late proof is not a conflict
    Given an agent whose first connection attempt is held up on the way
    When its next attempt connects first and the held one arrives afterwards
    Then the hub refuses the late proof as stale
    And the agent stays connected and no conflict is raised

  Scenario: A late proof is still not a conflict after the hub restarts
    Given an agent whose first connection attempt is held up on the way
    And the hub restarts after its next attempt connects
    When the held attempt arrives
    Then the hub refuses the late proof as stale and raises no conflict

  Scenario: An ordinary token can't clear a conflict
    Given an agent whose identity is in conflict with a copy
    When a copy pairs again with an ordinary token
    Then the conflict stays and the counter doesn't go down

  Scenario: A conflict is resolved by pairing the real machine again
    Given an agent whose identity is in conflict with a copy
    When the person makes a replacement token for that agent
    And the real machine pairs with it
    And the person approves the new key
    Then the hub clears the conflict
    And the copy is still refused

  Scenario: A refusal that the hub didn't sign is a dropped connection
    Given an agent paired with a hub, connecting through a proxy
    When the proxy answers the agent's proof with an unsigned identity-conflict refusal
    Then the agent treats it as a dropped connection and tries again later
    And the agent tells its person nothing about a conflict
    And the agent keeps its scheduled maintenance

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

  Scenario: A hub whose key leaked is recovered with the recovery key
    Given an agent paired with a hub
    When the person recovers the hub with the recovery seed
    Then the agent trusts the new key from its next connection
    And the old key is no hub key to it
    And the hub shows a new recovery seed

  Scenario: A mistyped recovery seed changes nothing
    Given a hub with a recovery key
    When the person gives the hub's recovery command a seed that isn't the recovery seed
    Then the hub refuses and its key is unchanged

  Scenario: A recovery seed can't be given on the command line
    Given a hub with a recovery key
    When the person gives the recovery seed as an argument of the hub's recovery command
    Then the command refuses it and says how to give it

  Scenario: An agent led onto a leaked key follows the recovery
    Given an agent paired with a hub
    And someone with the leaked key has sent the agent a handover to their own key and a list of addresses
    When the person recovers the hub with the recovery seed
    And the agent reaches the real hub at the address it was given
    Then the agent trusts the real hub's new key
    And the agent drops the leaked key's list of addresses

  Scenario: A list of addresses can't remove the one the agent was given
    Given an agent paired with a hub at an address
    When someone with the leaked key sends a list with the highest possible number that holds only another address
    Then the agent still tries the address it was given

  Scenario: A hub's generation can't go back
    Given an agent that holds generation 2 from its hub
    When a hub answers with generation 1
    Then the agent refuses it

  Scenario: A leaked key can't keep an agent from the real hub for good
    Given an agent that holds generation 2 from its hub
    And someone with the hub's leaked key has sent the agent the highest possible generation
    When the real hub connects with generation 2
    Then the agent refuses it and says the generation went back
    And the agent keeps backing up
    When the person recovers the hub with the recovery seed
    Then the agent trusts the new key and the real hub's generation

  Scenario: A replacement key waiting for approval can't disturb the real agent
    Given an approved agent that is connected
    And the person has made a replacement token for it
    When another machine pairs with the token and proves itself with a high counter
    Then the agent's own connection stays open
    And the agent's own key still connects with no conflict
    When the person approves the new key
    Then the hub uses the new key's counter

  Scenario: A lookalike that differs by case is flagged
    Given an agent paired under the hostname "Laptop"
    When another agent pairs as "lаptop" with a Cyrillic letter
    Then the hub says the name looks like "Laptop"

  Scenario: A pinned address needs the hybrid key exchange
    Given an agent paired with a hub at a pinned address
    When a connection offers only the X25519 key exchange and names a pinned binding
    Then the hub closes it as unavailable for that binding

  Scenario: An agent offers only the hybrid exchange to a pinned hub
    Given an agent paired with a hub at a pinned address
    When the hub's listener speaks only X25519
    Then the agent refuses the connection and sends nothing

  Scenario: A hub whose key was reset is refused until its agents pair again
    Given an agent paired with a hub
    When the hub's key is reset without a handover
    Then the agent refuses the hub
    When the agent pairs again with a new token and confirms the new hub key
    Then the agent connects and keeps its record

  Scenario Outline: A message the agent can't trust is refused
    Given an agent connected to its hub
    When the hub sends a removal notice that is <fault>
    Then the agent refuses it and stays paired

    Examples:
      | fault                                |
      | unsigned                             |
      | signed with the wrong key            |
      | signed for another agent             |
      | signed for an earlier connection     |
      | past its expiry                      |
      | sent again after it was accepted     |
      | missing from the sequence            |
      | altered after it was signed          |

  Scenario: A message kind the agent doesn't know doesn't break the connection
    Given an agent connected to its hub
    When the hub sends a signed message of a kind the agent doesn't know, and then another message
    Then the agent ignores the first and accepts the second
    And the connection stays open

  Scenario: A downgrade of the protocol is noticed
    Given an agent and a hub that both speak two protocols
    When the agent's range is lowered on its way to the hub
    Then the agent refuses the hub's answer

  Scenario: An agent and a hub one protocol apart keep syncing
    Given an agent and a hub that share one protocol and differ by one
    And the agent is paired with the hub
    When the agent connects
    Then the agent and the hub sync on the protocol they share
    And the agent says nothing is out of date, except the advance warning

  Scenario: An agent pairs with a hub of another release
    Given a hub and an agent on releases that both speak the handshake
    When the agent pairs with the hub's token
    Then the pairing succeeds and the hub lists the agent

  Scenario: A hub reinstalled as an older release keeps its agents syncing
    Given an agent paired with a hub that proved a newer handshake
    When the hub is replaced by an older release that proves only an older one
    Then the agent connects on the older handshake with no confirmation
    And no identity conflict is raised and no pairing is asked for
    And the agent says which hub release it sees

  Scenario: A hidden newer handshake is refused
    Given an agent paired with a hub that proves a newer handshake
    When a proxy answers the agent's request for the newer handshake with a 404
    Then the agent refuses to connect and sends no proof
    And the agent raises an alert that the hub's newer interface was hidden

  Scenario: A hub's key files stay out of a backup by default
    Given a plan whose source is Ladderback's data folder
    Then the plan leaves out the hub key, the hub's TLS key, the recovery seed and the agent key

  Scenario: The kit isn't offered on a hub with sign-in off
    Given a hub with sign-in off and an unconfirmed recovery seed
    When a request that looks like loopback asks for the recovery kit
    Then the hub refuses it as available by the hub's own command only
    And the seed isn't sent

  Scenario: A proxy that rewrites the host doesn't open the kit
    Given a hub with sign-in off behind a proxy on the same machine that sends the host 127.0.0.1
    When a remote request asks for the recovery kit with the hub's key ID as the step-up
    Then the hub refuses it as available by the hub's own command only

  Scenario: The kit isn't sent on a session without the hybrid key exchange
    Given a hub with built-in TLS and an unconfirmed recovery seed
    When a signed-in browser that offers only X25519 asks for the recovery kit
    Then the hub refuses it as available by the hub's own command only

  Scenario: The confirmation asks for a random group and a new step-up
    Given the person opened the recovery kit with a step-up
    When the person goes to the confirmation
    Then the hub asks for the password again
    And the page names a group by its number and not always the last
    When the page is loaded again
    Then it may name another group

  Scenario: A confirmation is offered after the seven days
    Given a hub whose recovery seed has been unconfirmed for eight days
    When the person confirms with their password and the right group
    Then the hub deletes the seed file

  Scenario: A laptop asleep for eight days is told its pending record lapsed
    Given a new agent that waited for approval and was disconnected for eight days
    When it connects and proves its key
    Then the hub answers with a signed pending-expired and the agent says to pair again with a new token
    And a stranger that connects with another key gets proof-refused

  Scenario: A pending replacement key lapses and counts toward the limit
    Given an approved agent and a replacement key waiting for seven days
    Then the hub discards the new key and the agent's record is as it was
    And while it waited it counted toward the limit of twenty pending

  Scenario: Approving a replacement key ends the old key's session
    Given an approved agent connected under its old key and a new key waiting
    When the person approves the new key
    Then the old key's connection closes

  Scenario: A token for a proxied address is a CA token
    Given a hub with built-in TLS that the person also reaches through a proxy
    When the person makes a token for the proxy's address and chooses a CA certificate
    Then the token's TLS byte is 0
    And an agent that pairs through the proxy never sees a refusal to pair

  Scenario: A skewed hub clock pauses maintenance and never backups
    Given an agent paired with a hub whose Welcome is signed 25 hours off
    Then the agent runs no scheduled prune or check
    And it keeps backing up
```

### Appendix B: `fleet/sync_replay.feature` (#568)

These need the agent's snapshots and events, so they belong to #568, not #567.

```gherkin
  Scenario Outline: A report the hub can't trust is refused
    Given an agent connected to its hub through a proxy
    When the proxy <change>
    Then the hub refuses the report
    And the hub acts on nothing from it

    Examples:
      | change                                           |
      | changes the agent's list of repositories         |
      | rewrites a plan's retention in the agent's plans |
      | drops one of the agent's reports                 |

  Scenario: A pending agent's reports are held and not applied
    Given an agent waiting for approval that reports a repository
    Then the hub shows what the agent reported on its pending row
    And the hub's own list of repositories has nothing from it

  Scenario: The hub never receives a repository secret
    Given an agent with a repository whose password is a canary value
    When the agent pairs and syncs with the hub
    Then the canary appears in nothing the hub received
```

### Appendix C: M9's files (#571)

These need leases, owners, the prune guard and commands, which M9 builds.

`maintenance/ownership.feature`:

```gherkin
  Scenario: A lease moves only after the old owner releases it
    Given an agent that owns a repository by lease
    When the hub moves the repository to another owner
    Then the new owner gets no grant until the old owner's signed release arrives or the lease expires

  Scenario: A release the old owner didn't send is refused
    Given an agent that owns a repository by lease
    When a proxy forges the agent's acknowledgement of a release
    Then the hub refuses it and the new owner gets no grant until the lease expires

  Scenario: A pending agent gets no lease and refuses no other agent's claim
    Given an agent waiting for approval that claims a repository
    Then the hub grants it no lease
    And no other agent's claim is refused because of it

  Scenario: Approving an agent grants what the rules allow
    Given an agent waiting for approval that has reported a repository
    When the person approves it
    Then the hub grants it what the rules allow

  Scenario: A conflict stops an agent's leases until it is resolved
    Given an agent whose identity is in conflict with a copy
    Then the hub stops granting and renewing that agent's leases
    When the person resolves the conflict with a replacement token and approves the new key
    Then the hub renews the agent's leases

  Scenario: A hub can't stretch an owner's deadline
    Given an agent that maintains its own repository
    When the hub grants a keep-delete far longer than the agent's own
    Then the agent computes its deadlines from its own keep-delete
```

`maintenance/prune_guard.feature`:

```gherkin
  Scenario: A pending agent's running backup doesn't hold the hub's prune
    Given an agent waiting for approval that reports a running backup
    Then the hub's prune does not wait for it

  Scenario: A reported deadline is capped
    Given an approved agent that reports a deadline beyond its index read time plus keep-delete
    Then the hub holds the prune guard only until that cap
```

`fleet/commands.feature`:

```gherkin
  Scenario: A command that waited too long is not run
    Given an agent that was offline when the person asked it to run a plan
    When the agent reconnects after the command's age limit
    Then the agent doesn't run it and the hub says so

  Scenario: A command dated in the future is not run
    Given a hub whose clock was far ahead when the person asked for a run
    When the hub sends the command
    Then the agent doesn't run it

  Scenario: A command delivered twice runs once
    Given an agent connected to its hub
    When the hub sends the same command again after a disconnect
    Then the agent runs it once and answers with the first result
```
