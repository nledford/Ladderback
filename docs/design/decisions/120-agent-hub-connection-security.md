# Decision 120: How an agent and its hub prove who they are

**At connect, the hub and the agent each prove their identity with an Ed25519 signature over a domain-separated message: a fixed label, the hub's key, the agent ID, both sides' fresh nonces and, when the hub serves TLS itself, the TLS 1.3 session's exporter value (RFC 9266). The hub has a key pair of its own that every agent pins at pairing, carried in the pairing token beside the certificate's fingerprint. Every hub message that gives an agent authority or makes it act is signed with that key, for one agent and one connection, with a sequence number and an expiry; the agent's messages the hub decides on are signed the same way.**

**The pairing token travels from the hub to the agent as text, as a QR code of the same text (in the web UI or drawn in a terminal by `ladderback pairing-token`), or through a phone's share sheet. Making a token and pairing an agent in the web UI need a signed-in session and a fresh step-up; the hub's command line, run by the user that owns its data folder, needs neither, as `set-password` doesn't. Mutual TLS, a required tunnel and peer-to-peer are not needed** (recorded 2026-11-18, §3.1, §7.1, §7.2, §10.3; #567, items 1–4 of the maintainer's comment of 2026-11-14, and the maintainer's directions of 2026-11-18 on pairing by QR code, from a phone and from a headless machine).

The decision also fixes the order of the handshake, which decision 88 left open. The maintainer pre-approved security crates on 2026-11-18; "Crates" names them, and names apart the ones it didn't pre-approve. A headless machine reached over SSH pairs with a phone through `ladderback pairing-token` and `ladderback pair` ("Pairing from a terminal").

## Where each item is answered

| Item | Answered in |
|---|---|
| #567 comment, 1: what the agent signs; the binding to TLS in built-in mode and why not behind a proxy | "The signed messages", "Binding to the TLS session" |
| 2: why not mutual TLS, a required tunnel or peer-to-peer; running over a tunnel stays supported | "Why not the alternatives" |
| 3: tests for the bare nonce, another hub, another agent ID | "Tests that pin it", first bullet |
| 4: the hub key pair: pinned at pairing, both sides prove themselves, signed commands, backup and rotation | "The hub key", "Signed messages after the handshake", "Tests that pin it" |
| Pairing token, hostname clash, re-pairing, a lost data folder, #296 | "Pairing" |
| QR codes beside the text token; the camera | "Pairing by QR code" |
| Pairing from a phone; remote UIs; the share sheet; the secure context | "Pairing from a phone", "Who may make a token or pair" |
| Where a token can leak | "Where a token can leak" |
| A headless hub or agent over SSH; the hub's command; the terminal's QR code | "Pairing from a terminal", "Where a token can leak" |
| A short typed code (PAKE); an agent-started approval (RFC 8628) | "Why not the alternatives" |
| The crates | "Crates" |

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
  | Hello { ... }, signed by the agent      |
  |---------------------------------------->|
  |   Welcome, signed by the hub (§7.2)     |
  |<----------------------------------------|
```

- **The agent proves nothing until the hub has.** The key in `Challenge` must be the agent's pinned hub key, or reach it through the hub's handovers ("The hub key"), and `hub_proof` must verify. Otherwise the agent closes, sends no `Proof` and no `Hello`, and says `hub-key-mismatch` or `hub-proof-invalid`.
- **The hub answers every `Connect`.** An agent ID it doesn't know gets a `Challenge` like any other, so a stranger can't list the registry. The refusal comes at `Proof`, one code (`proof-refused`) for an unknown agent, a deleted one and a wrong signature.
- **Nonces.** Each is 32 bytes from the `Random` port, fresh for each connection attempt and good for that connection only. The hub forgets its nonce when the proof is checked or after 15 s.
- **Bounds before the proofs.** A frame over 4 KiB and a handshake over 15 s are refused (§13.5). At most 32 connections may be unauthenticated at once, and in built-in TLS mode at most 4 from one address, so one host can't fill them. Behind a proxy every connection has the proxy's address, so only the total applies (see "What this doesn't defend"). A `Hello` before a good `Proof` is a protocol error.
- **The order is new.** §7.1 had the hub send a nonce first. The hub can only sign the agent's nonce if the agent speaks first. No hub has shipped, so nothing is broken. From here decision 88's rule holds, as [amended by this decision](88-hub-and-agent-version-compatibility.md): the handshake is the same in every protocol, so two releases can always begin it.
- **No fallback between handshakes.** The label and the path name the handshake and never change in place. A different handshake is a new path and a decision, and an agent that speaks the new one never falls back to this one on a failure, since a party in the middle could cause the failure. Nothing in the handshake is negotiated, so nothing in it can be lowered.

## The signed messages

Every signature is Ed25519 (RFC 8032) over bytes built one way, by a pure function in `domain` that the tests pin with fixed vectors.

- **Encoding.** One byte for the label's length, the label in ASCII, then the fields in the order below, with no separator. Integers are big-endian unsigned 64-bit; a time is whole seconds since the Unix epoch. A key is its 32 raw bytes, an agent ID the 16 bytes of its UUID, a nonce 32 bytes. Only a hostname and a message kind are variable: each is one length byte, then UTF-8 (a hostname is at most 255 bytes). Every other field has a fixed width, so no two messages with different labels or fields encode to the same bytes.
- **The binding field** is 33 bytes: a kind byte (0, none, or 1, `tls-exporter`) and 32 bytes that are zero for kind 0.
- **Every label starts `ladderback-sync-v1 `.** The `v1` names the handshake, as `/sync/v1` does, not a protocol of the range `Hello` carries. The rest names the message, so a signature made for one message never verifies as another.

| Message | Signed by | Label ends | Fields, in order |
|---|---|---|---|
| Hub proof | the hub key | `hub-proof` | hub key, agent ID, agent nonce, hub nonce, binding |
| Agent proof | the agent key | `agent-proof` | hub key, agent ID, agent key, agent nonce, hub nonce, binding, counter |
| Pairing | the token's pairing key and the agent key, two signatures over the same bytes | `pair` | hub key, agent ID, agent key, agent nonce, hub nonce, binding, counter, hostname |
| Paired | the hub key | `paired` | hub key, agent ID, agent key, agent nonce, hub nonce, binding |
| Hub message | the hub key | `hub-signed` | hub key, agent ID, agent nonce, hub nonce, binding, sequence, expires at, kind, SHA-256 of the payload |
| Agent message | the agent key | `agent-signed` | hub key, agent ID, agent nonce, hub nonce, binding, sequence, kind, SHA-256 of the payload |
| Handover | the old hub key | `handover` | old key, new key, made at |

Why each field is there:

- **The label** keeps one message from standing in for another, and keeps any of them from being a signature over something else the key signs. A signature over the bare nonce verifies as none of them.
- **The hub's key** is the hub's identity in every message, in both TLS modes ("The hub key"). A signature made for another hub is refused, because that hub's key is another value.
- **The agent ID and the two nonces** tie a signature to one agent and one connection. A recorded proof or command can't be replayed on another connection, and a hub's signature for one agent is no good to another.
- **Both nonces in both proofs.** Each side signs a value the other side drew, so neither can have a signature made in advance. The hub signing for a stranger's nonce gives the stranger nothing: the hub's own fresh nonce is in what it signs, and no other message has the label `hub-proof`.
- **Reflection.** The two proofs differ in label and signing key, so a hub proof presented back to the hub as an agent proof, or the reverse, verifies as neither. The label alone separates them: the two share a field prefix.
- **The binding in every message** makes each signature good on one TLS session only, where the mode has one.
- **The counter** is the agent's connection counter ("A copied data folder").

## Binding to the TLS session

**Built-in TLS.** Every signed message carries the connection's `tls-exporter` value (RFC 9266): the TLS 1.3 exporter with the label `EXPORTER-Channel-Binding`, an empty context and 32 bytes. Each side computes it from its own end of the session (rustls `export_keying_material`), so the signatures hold only on the TLS session they were made for. The listener accepts TLS 1.3 only, which keeps the exporter free of RFC 7627's conditions.

What the binding adds to the pinned certificate: a party that holds the certificate's private key (a copy in a backup, say) but neither Ed25519 key can terminate an agent's TLS and relay to the hub, and each side's signature then covers a different session, so both refuse. The agent's pinned-certificate verifier must also check the TLS 1.3 `CertificateVerify`, so that a server that presents the pinned certificate without its private key is refused (a test below).

**Behind a TLS-terminating proxy** there is no shared session. The agent's TLS ends at the proxy; the hub sees another connection, so the two ends compute different values and every honest connection would fail. The binding is kind 0, and the hub's key carries the whole of the hub's identity.

- **The kind is never negotiated, and is a property of the agent.** The token's TLS byte decides it: an agent paired with a pinned certificate (byte 1) always signs kind 1, and one paired with a CA certificate (byte 0) always signs kind 0. The kind is in the signed pairing message, and the hub records it with the agent and requires that kind at every proof. A mismatch is a signature that doesn't verify, never a quiet fallback.
- **So one hub can serve both:** agents on the LAN pinned to its certificate, and others through a proxy. A proxy that re-encrypts to the hub's own listener gives the hub a TLS session the agent doesn't share; those agents are paired as kind 0 and the hub accepts that of them. A plain loopback `http://` connection is kind 0.
- **Changing how the hub is served costs a re-pairing** of the agents whose kind or pinned certificate changes. The same TLS key may be reissued in a new certificate without it ("The hub key").
- **What a proxy can still do.** It can relay a handshake it didn't start. It can't act as the hub to an agent without the real hub (no hub proof), act as the agent to the hub without the agent, or make an agent act, or make the hub decide, with a signed message. It can read and drop what isn't signed, and cut the connection ("What this doesn't defend").
- **A plain `http://` address is refused** by an agent that pairs or connects, unless it is loopback, as the MCP token's address is (§10.5).

## The hub key

- **Made once.** The hub makes an Ed25519 key pair the first time it starts as a hub with no registered agent and no recorded key. The seed is 32 bytes from the `Random` port, which also lets a test seed it. It is not the hub's own agent key: the hub is an agent as well, and a signature made as an agent must never be one made as the hub.
- **Kept in the data folder, never the database**, as the agent's key is: a file `hub-key` of one line, the seed as 64 lower-case hex digits, mode 0600, written whole under a private name and linked into place, as the agent ID file is. `db rebuild` can't change it. Other operating-system users can't read it; a program running as the person can, which decision 99 accepts. In memory the seed is wiped when dropped. A `hub-key` file that exists is never overwritten, whatever the database holds.
- **The certificate's key is kept beside it.** In built-in mode the hub makes an ECDSA P-256 key once (`hub-tls-key`, as private as `hub-key`) and a self-signed certificate for it. The fingerprint in the token is the SHA-256 of the certificate's public key (SubjectPublicKeyInfo), so the hub may reissue the certificate from the same key when it lapses without any agent pairing again. A new TLS key changes the fingerprint and costs a re-pairing, as a lost hub key does.
- **Pinned at pairing.** The token carries the public key, so each agent holds it before it sends anything. The agent keeps its pin in its data folder (`hub-pin`), never its database. The hub shows the key's SHA-256 in Settings in groups of four hex digits, and the agent's pairing page shows it before the person confirms, for a person to compare.
- **The hub never makes a replacement by itself.** The database records the key's fingerprint when the key is made. A hub that starts with the file missing or another key refuses `/sync/v1`, says so on its pages and in its log, and offers the two ways out: restore the file, or reset the key, which names every agent that will need to pair again. The same holds for the TLS key in built-in mode.
- **Backup.** Copy `hub-key`, `hub-tls-key` and `hub-handovers` (below). They sit in the data folder a person backing up the hub already includes, so there is no export command; Settings shows their paths and the fingerprint, so a restored key can be checked. Anyone holding the files can pose as the hub to every agent, so they are kept like a password. A `hub-key` left in a backup after a rotation can still impersonate the hub to an agent that hasn't connected since: delete old backups, or treat the rotation as a leak.
- **Rotation is a handover the old key signs.** The hub makes a new key, signs `old key, new key, time` with the old one, stores that handover in `hub-handovers` (public) and replaces the key, deleting the old seed. It closes every open connection, since the old seed signed them. At connect it sends its handovers in `Challenge`, at most 8, oldest first. An agent whose pin is the `old key` of a link in the chain follows it, checking each signature with the key before.
- **The agent keeps the new key once the whole handshake has succeeded,** `Welcome` included. Until then it verifies every message of that connection, `Welcome` too, against the key it reached through the chain, and the pin on disk still holds the old one. A chain that doesn't reach the agent's pin is `hub-key-mismatch`. An agent that missed more than 8 rotations pairs again.
- **What losing it costs.** Every agent refuses the hub (`hub-key-mismatch`), and each pairs again with a new token on the person's confirmation. Agents keep backing up and, as with a version mismatch (decision 88), their leases lapse in at most 7 days, after which they run no scheduled maintenance. The hub's registry survives, and an agent that pairs again with its own ID and key keeps its record.
- **A leaked key can't be recovered by rotating.** Whoever holds the old key can sign a handover to a key of their own. The remedy is a reset, then pairing every agent again.

## Signed messages after the handshake

A hub message is signed when it gives an agent authority or makes it act: `Welcome`, `Paired`, each `RepoGrants` (a lease renewal is its own signed `RepoGrants`, not part of a heartbeat reply) and, from M9, `RunPlan`, `CancelOp` and `RestoreRequest`, and `UpdatePlan` the day it ships. The agent obeys none of them unsigned. `Ack`, `FetchLog` and heartbeat replies are not signed. Payloads are not encrypted a second time: the link carries no secrets (decision 3, I4).

- **The envelope** holds `kind`, `seq`, `expires_at` (integer seconds, hub messages only), the `payload` as the unpadded base64url of its JSON bytes and the `signature`. The digest is over the decoded bytes, and the decoding is strict: a padded, mixed-alphabet or otherwise non-canonical encoding is refused. The agent parses the payload only after the signature holds, from the same bytes the digest covered, so nothing is canonicalized.
- **The key and the connection.** The agent builds the signed message from the envelope, its own agent ID, the hub key this connection verified (the key in `Challenge`, reached from the pin through any handovers) and the two nonces of this connection, so a message made for another agent, hub or connection fails to verify. A hub key changed mid-connection is no hub key this connection verified.
- **Sequence.** The hub counts from 1 on each connection; the agent refuses any `seq` not above the last it accepted on this connection. A recorded message can't be played again on its connection, and the nonces keep it from another. Gaps are allowed. The agent records `seq` before it acts.
- **Expiry.** `expires_at` is at most 10 minutes after the hub signs. The agent refuses a message whose time has passed by its clock, or that expires more than 15 minutes ahead, so a hub bug can't mint a message that never lapses. The hub signs when it sends, never when it queues, so a command waiting for an offline agent gets a fresh signature and a fresh sequence on connect. A refused `Welcome` closes the connection with `clock-skew` and the offset; any other expired message is refused alone.
- **Order of checks.** Signature first, with nothing recorded for a message that fails; then sequence; then expiry; then the agent acts. A forged or replayed message also closes the connection, and the refusal is a code in the log.
- **`Welcome` carries `agent_protocols`,** the range the hub saw in `Hello`. The agent refuses a `Welcome` whose echo isn't the range it sent, or whose `protocol` isn't the highest both ranges share. A proxy that lowers either side's range, to push the pair onto an older protocol (decision 88), is caught. `Hello` is also signed by the agent, so the hub sees the range the agent sent.
- **`RepoGrants` keep their own `expires_at`,** the lease's (§7.3). The message's expiry is how long it may be accepted, not how long the lease runs.
- **The agent signs what the hub decides on:** `Hello`, `ReposSnapshot` and `Heartbeat`, with its key, a `seq` above the last the hub accepted on the connection and no expiry. The hub refuses one that is unsigned, wrongly signed or not above the last. Without this a proxy could change a claim or a list of running operations that the hub turns into leases and prune waits (§7.3). The hub identifies an agent by the hostname registered at pairing, never the one in `Hello`; a rename comes in the signed `Hello` and is applied only if no other agent holds the new name.
- **Everything else is not signed:** plans, events and logs an agent sends, `Ack`, `FetchLog` and status. These change what a person sees, never what an agent does or what the hub grants.
- **Decisions 2 and 12 stand.** The hub still sends only the request and the agent runs it, a restore only into a new folder and only where `allow-hub-restore` is set, and `UpdatePlan` stays reserved. The signature is one more condition on these, never a way round them.

## Pairing

- **The token** is `ladderback_pair_` and the unpadded base64url of: a version byte (1), the hub key (32), a TLS byte (0 CA, 1 pinned), the certificate's public-key fingerprint (32, zeros for CA), the seed of a one-time pairing key (32), the hub address (one length byte, then at most 200 bytes of UTF-8: `https://host:port`) and the first 4 bytes of the SHA-256 of all before them. It is 103 bytes plus the address. The checksum only catches a mistyped token before any connection is made; it is no security. The prefix lets a secret scanner find a leaked token.
- **The address is what the person was using** when the hub's page made the token, and the dialog lets them edit it, since agents may reach the hub another way. It needs no secrecy and no trust: a changed address leads to a host that can't give the hub's proof, and the agent then sends nothing of the token.
- **The hub keeps only the pairing key's public half,** with its expiry and whether it was used, so a stolen database holds no usable token. The token is shown once. Its life is 15 minutes by default, 5 minutes to 24 hours on request, and at most 20 are unused at once. It is spent in the same transaction that records the agent, so two agents pairing with one token at once get one success and one `token-used`.
- **Pairing is the handshake, with a different last step.** The agent starts with `Connect` and checks the hub's proof against the key in the token, so the token's seed never leaves an agent that hasn't verified the hub, and the seed itself never crosses the wire at all. It sends `Pair`, naming the public half of the pairing key so the hub finds the token, with the pairing message signed twice: by the pairing key, which is the hub's authorization, and by the agent key, which proves it holds the key it registers. The hub answers with a signed `Paired`.
- **`ladderback pair` never takes the token as an argument.** It reads the token at a prompt (a paste, without echo, as `set-password` reads a password, decision 70), as one line of standard input, or from `--token-file`, never from argv, where another user of the machine could read it with `ps` and the shell would keep it in its history. The hub address is in the token; `ladderback pair <address>` overrides it, for an agent that reaches the hub another way. A token on argv is refused with a message that says how to give it (§10.5 rules the same for the MCP token). The command shape the maintainer sketched, `ladderback pair <address> <token>`, is therefore not offered; this is the decision's one departure from it, for the maintainer to confirm.
- **A hostname taken by another agent** is refused with `hostname-taken`, naming the holder and when it was last seen. Names compare ignoring case, as DNS names do. The token isn't spent, so the person changes the agent's hostname, or removes the old agent on the hub, and pairs again with the same token.
- **Pairing again as the same identity** (the same ID and key) reuses its record, and sets the hub's stored counter to the agent's. An agent ID that arrives with another key is refused (`agent-key-mismatch`): a token authorizes a new agent, never taking over a registered one. The one exception is a replacement token ("A copied data folder"). An agent that pairs with a token whose hub key isn't the one it has pinned asks its person to confirm, naming both fingerprints, before it replaces the pin.
- **An agent that lost its data folder** has no ID or key to show, so it pairs as a new agent. Its old record still holds the hostname: the hub says which agent that is, and the person removes it, which orphans its snapshots for the adoption of §7.3, then pairs with the token that is still unspent.
- **A stolen unused token** lets one rogue agent register under a free hostname until the token lapses or is used. The real agent then sees `token-used`. The rogue agent is a record in the hub's list that anyone can remove; it can't take over a registered agent, and it is sent nothing but its own grants, which it gets only for a repository no other agent uses (§7.3). Having a person approve each new agent on the hub would remove even that, at the cost of a step in every pairing; not chosen, and the maintainer can revisit it.

## A copied data folder (#296)

Two machines with one ID and one key can both prove themselves. The agent's own check is first: the machine identifier stored beside the ID, checked at start (the maintainer's decision on #296), which stops maintenance on the copy. The hub adds a second.

- **The connection counter.** Each agent keeps one in its data folder and writes it before each proof, atomically and with an fsync of the file and its folder, so a power loss can't roll it back. The hub takes only a counter above the one it holds, after the signature verifies. An agent that has an ID but no counter file starts at 0, like a restore from a backup.
- **One live connection per agent ID.** A good proof for an ID that already has a live connection closes the older one, since a half-open connection after a network change looks the same. So the original and a copy that both stay up take turns, and the one behind is refused `identity-conflict` at its next proof. The case of a copy made while the original is connected is the one that needs this rule: without it both would hold sessions and leases.
- **A conflict stays until a person resolves it.** The hub raises an alert naming the agent, refuses every proof for that ID with `identity-conflict`, stops granting and renewing its leases, and keeps its other state. An agent told so keeps backing up and runs no maintenance, as #296 decided. It doesn't clear when the counters happen to agree.
- **Resolving it takes a replacement token.** The person makes it from the alert, for that agent. The real machine pairs with it and takes a new agent key under its old ID; the hub registers that key and clears the conflict. The copy's key no longer verifies, whatever its counter, and it stays refused. A copy that should be a machine of its own takes a new ID ("this is a new machine", #296).
- **What this can't catch.** A disk image cloned with its machine identifier, whose original is switched off while the copy connects first, looks like one machine until the original returns; both then hold a lease for up to 7 days. The prune guard's other layers (§7.3, I3) and rustic's repository lock still apply. A data folder restored from a backup looks the same and gets the same remedy.

## Pairing by QR code

The hub's "New pairing token" dialog shows the token three ways, from the same bytes: as text with a Copy button, as a QR code of the same `ladderback_pair_…` string, and, where the browser has one, a Share button. The agent's pairing page takes the token pasted, or scanned with the computer's or phone's camera. The QR adds no secret and no format: it is the token's text.

- **The QR is drawn in the browser,** by the web UI from the token it already holds, with a pure-Rust encoder compiled to WebAssembly ("Crates"). No network service draws it, and the daemon is asked for nothing more. It is an inline SVG, so it scales with text size and prints sharply.
- **Camera frames stay in the browser.** The camera is asked for only when the person presses "Scan the code", never when the page opens. Frames go to a video element and a canvas, a decoder in WebAssembly reads them, and the page keeps none: no frame is sent, stored, logged or kept, and the camera is released at once when a code is read, when the person cancels and when the page is left. The page's policy (`default-src 'self'`) gives the page nowhere else to send them, and no `Permissions-Policy` header on the pairing page may deny the camera to its own origin.
- **What is scanned is checked before it is used.** The prefix, the checksum and the version must hold. Only then does the page show the hub address, the hub key's fingerprint and the TLS mode for the person to confirm. The token is not sent to the daemon until they do.
- **A secure context is needed for the camera.** `localhost` and loopback are secure contexts, so the agent's UI on its own machine can scan. A remote UI needs HTTPS ("Pairing from a phone"). Where the page isn't a secure context, or has no camera API, or the person declines, the page says why in text and the paste field stays.
- **The QR lives as long as the token.** It shows only while the dialog is open, and the dialog hides it, the text, Copy and Share when the token is used or lapses (the events stream tells it, §7.1). Closing the dialog drops the page's copy of the text, as far as a browser's memory can be wiped, which §10.2 lists among its known limits.
- **It is never the only way** (A1, A2, A5 of §0; §4.5). The text, Copy and the paste field stay. The QR has a text alternative that says what it is, a pairing code for this hub, and not the token. Scanning is fully optional and reachable by keyboard: a button, a live status ("Looking for a code", "Code read: hub 1a2b 3c4d"), Escape to stop, and no part of a task rests on seeing the picture.
- **The QR and the scanner are shared components,** each built in its own PR before the pairing page, to the checklist of §13.7.5. The page never builds them itself.

## Pairing from a phone

The token still travels from the hub to the agent, and a phone may be either end, in the installed web app (#394) or a browser.

- **Phone signed in to the hub.** It makes the token and shows it as text, Copy and QR, as the desktop dialog does, and a Share button where `navigator.share` exists. The share carries the token as text only, so the system's share sheet (AirDrop, messages, a password manager) can take it to the agent's machine. The laptop agent scans the phone's screen with its webcam, or the person pastes what was shared.
- **Phone signed in to an agent.** Its pairing page's "Scan the code" reads, with the phone's camera, the QR on the hub's screen or on another phone, then pairs that agent. Paste stays.
- **The secure context.** `getUserMedia` and `navigator.share` need one, and a phone can't reach the loopback address of a computer, so the phone needs HTTPS to the hub or the agent. The setup is #394's: a reverse proxy with a certificate the phone trusts, sign-in on and `--allowed-host`. The built-in self-signed certificate is the open case.
- **What is verified.** From MDN: a document is a secure context when its URL is `https`, `localhost` or loopback, and `navigator.share` needs a secure context, the `web-share` permission policy and a user's click. From the WebKit bug tracker and a vendor's notes: on iOS a Home Screen web app can use the camera since iOS 13.3, but it asks for permission again on each launch and has had reliability bugs. From #394: web push on iPhone needs the app installed from Safari's Home Screen, from iOS 16.4.
- **What is inferred, not tested:** whether a browser treats an `https` page whose self-signed certificate the person clicked through as a secure context for the camera and the share sheet, and whether such a page may install or register a service worker. Don't rely on it. The UI feature-detects `isSecureContext`, `mediaDevices.getUserMedia` and `navigator.share` and offers each only where it is there; a self-signed hub on a phone falls back to paste. The red `test:` commit for the scan component records what real iOS and Android browsers do.
- **The fallback is always paste,** from Copy, from a share, or from a password manager. A phone that can't scan can still pair by pasting the token.

## Pairing from a terminal

A headless machine reached over SSH (in Ghostty, say) has no browser. The token still goes from the hub to the agent, and a phone can be the other end.

- **The hub's command.** `ladderback pairing-token` makes one token, as the dialog does, with the same life by default (15 minutes) and `--ttl` to set it (5 minutes to 24 hours), and `--address` for the address the agents reach the hub at (default: the address configured for the hub; with neither the command asks for one). It prints the token as text first, then, when standard output is a terminal and the code fits, a QR code of the same text, and a line that says the token is one-time and when it lapses.
- **The QR code in a terminal** is drawn with Unicode half blocks (`▀`, two modules to a character cell, one above the other) from the QR's module matrix, in explicit colors so a dark or a light theme can't turn it into something a camera won't read: foreground black and background white, set with the 256-color indices 16 and 231 (themes remap the 16 named colors, not those), and reset at the end of every line. A quiet zone of 4 modules surrounds it on every side, and the row count is padded to an even number. With the module matrix the command draws both this and the web dialog's SVG, so one crate serves both and none is added for the terminal.
- **Text only when it can't be a QR.** When standard output isn't a terminal (a pipe, a file, `$(…)`), the command prints the token and nothing else, so a script gets only the token. It does the same with `--no-qr`, when `TERM` is `dumb` or the terminal reports a size too small for the code. The code at error-correction level L (a screen isn't damaged) is about 29 to 31 rows and 57 to 61 columns with its quiet zone, more than an 80×24 window: the command says so, and says to enlarge the window or paste the text. It never wraps or scrolls a code.
- **It is never logged.** The command writes the token to the terminal only. The daemon's log, and its own output, never hold a token, a QR code or the pairing key; the hub's event for a new token names the token's id (the first 8 hex digits of its public key's SHA-256) and when it lapses, never the token.
- **Who may run it: the operating-system user that runs the hub,** as `set-password` is (decision 99, decision 70): the command opens the data folder, so whoever can do that can also read the hub key and edit the database. There is no sign-in and no step-up on the command line, because a typed password adds nothing against a person who has the files. The command writes the token's public half to the database and the hub finds it there.
- **A later option: the Kitty graphics protocol** (Ghostty, Kitty) would draw a crisper image where the terminal reports it. It isn't required, isn't in the first version and has to fall back to the half blocks; it needs no crate beyond the image's bytes.
- **A headless agent can't scan.** It has no camera. `ladderback pair` takes the pasted token: from the phone through Universal Clipboard, AirDrop or the share sheet to the machine whose terminal runs SSH, then pasted at the prompt. The scan is for a phone or laptop with a camera that pairs the agent through its browser ("Pairing from a phone").
- **Pairing an agent from a phone, when the hub is headless:** the phone scans the code in the hub's terminal, and the phone's browser signed in to that agent pairs it with the scanned token. A hub with no browser can still pair: the phone is the camera and the agent's page the other end.

## Who may make a token or pair

Making a token (on the hub) and pairing an agent are high-impact actions: a token authorizes a machine, and pairing points an agent at a hub that can send it signed commands. Both are for a session only, never an API token, and have no MCP tool (§10.5). On a phone, a laptop and the machine itself the rule is the same.

- **A signed-in session and a fresh step-up, every time** (§10.4): the password re-entered, its attempts sharing the sign-in waits, one step-up for each token made and each pairing. A session that is old or recent proves nothing here. The refusal is `step-up-failed`, as for a restore.
- **Beyond loopback, sign-in must be on.** A request is beyond loopback when its `Host` isn't a loopback name or its peer isn't a loopback address, so a proxy on the same machine doesn't make a remote request look local. With sign-in off or in proxy-header mode there is no password to re-enter, and the short ID that stands in for it only guards against misclicks (§10.4). Those requests are refused with `pairing-needs-sign-in`; the person sets an admin password.
- **On loopback with sign-in off,** the step-up is the typed short ID, as for a restore: the first eight hex digits of the hub key's fingerprint. Decision 99 trusts the person and the programs they run.
- **The allowed hosts apply as always** (§10.1): a `Host` that isn't a configured name or loopback is refused before any route runs.
- **HTTPS can't be enforced by the daemon behind a proxy,** since forwarded headers are never read. Built-in TLS is HTTPS by construction. Otherwise Settings and the docs say that making or pairing over plain `http://` beyond loopback shows the token to the network, and the web UI, which can see `isSecureContext`, refuses to show or take a token on a page that isn't one. That protects a person from a mistake, not from an attacker, who needn't use the page.
- **An agent's pairing page refuses a hub address that is plain `http://` and not loopback,** whatever the page is served over.
- **The hub's command line is the exception to all of the above:** `ladderback pairing-token` needs no sign-in and no step-up, and runs only as the user that owns the data folder. A request to the web UI never reaches it.

## Where a token can leak

A token is a one-time, short-lived credential: whoever holds it unused can pair one agent, once ("Pairing", a stolen unused token). These are the ways it leaves the hub's page, and what each costs.

- **The text and the QR are the same secret.** A screenshot, or a photo of the screen, holds the same token the text does, for the same short time. A QR on a phone in public can be read by anyone who can see the screen: it is the same token, and the dialog hides it once used or lapsed. The default life stays 15 minutes, and a person who is about to show it in public can pick 5.
- **Copy** puts the token on the clipboard, where other apps, a clipboard history or a synchronized clipboard may read it. Ladderback can't clear another app's copy.
- **A share leaves Ladderback.** What goes through the share sheet lands in whatever app the person picks, which keeps it, may sync it and may index it. It is still one-time and short-lived, and the dialog says so: share it only to the person's own devices. A share doesn't lengthen the token's life.
- **The terminal keeps what it shows.** `ladderback pairing-token` prints the token and its QR code into the terminal's scrollback, a `tmux` or `screen` log, a terminal's session recording and the screen of an SSH session, which another person or a shoulder surfer may read. The token is one-time and short-lived, and the command says so; the person can clear the screen or choose a life of 5 minutes. A recording of the session is outside Ladderback's control.
- **Shell history and process lists** hold nothing: neither `ladderback pair` nor `pairing-token` takes the token as an argument, and the token is printed, not passed. `--ttl` and `--address` are not secrets.
- **A used or lapsed token is useless,** and the hub holds none of it ("Pairing").
- **A scan that goes wrong holds nothing:** frames aren't stored, and a code that isn't a token is dropped.

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
| **Encrypting every message after the handshake** | A session key from an authenticated key exchange (X25519), which has a key schedule to design and review. | Signing needs no key exchange and covers what matters: what moves an agent or the hub. What is left is status the link carries in the clear, with no secret (I4). Under "What this doesn't defend" |
| **A short typed code by a PAKE** (SPAKE2, CPace), like `7-purple-lantern` | A PAKE crate, a handshake step to design and review, and a code short enough to guess unless attempts are limited. | **Not in the first version; a later option.** Conditions: a well-reviewed crate (not our own protocol), few attempts per code and then it dies, a short life, and the pinned hub key resting on the PAKE (the agent learns the hub key from the exchange, so the exchange must authenticate it). The QR and the text cover the first version |
| **The agent shows a code, and the person approves it on the hub from a phone** (a device-authorization flow, RFC 8628, started by the agent) | An unauthenticated request path on the hub that any host can start, and a code a person copies between two screens. | **Not chosen, and recorded as a possible later option.** Its appeal is real: no token travels, and a phone signed in to the hub approves in one tap. Its weakness is that the agent learns the hub's key on first contact, from whoever answers at the address, so a host in the middle is trusted unless the person compares fingerprints. It would be safe with: a short authentication string, derived from both sides' keys, that the person compares on both screens before approving; a rate limit and a short life on requests; the requesting hostname and address shown on the hub; the same step-up as a token. Those make it a PAKE-like design, so it waits with the PAKE |

**Running Ladderback over an existing tunnel stays supported.** Nothing in the design depends on the network. Over a tunnel the hub still serves built-in TLS, pinned, so nothing changes; the plain `http://` refusal above applies to a tunnel address as to any other.

## What this doesn't defend

- **A hostile proxy and what isn't signed.** A TLS-terminating proxy the operator doesn't trust, or a party with a certificate wrongly issued for the hub's name, can read what agents send and change plans, events and logs they report. It can drop messages and cut the connection. It can't make an agent prune, restore, run a plan or accept a lease, can't forge the hub or an agent, and can't alter a claim or a running-operation list the hub decides on (those are signed).
- **A proxy can still withhold a signed message,** so the hub may act on a stale view: for example, a dropped `ReposSnapshot` leaves the hub unaware that a second agent uses a repository. I1 and I3 then rest on §7.3's other layers (the tags of other agents' snapshots, the deadline, the prune guard and rustic's lock), which are unchanged. Run built-in TLS, or a tunnel, if the proxy isn't yours.
- **Denial of service before the proofs.** Anyone who can reach the hub can open connections and hold them for 15 s. The per-address cap stops one host in built-in mode; behind a proxy every connection has the proxy's address, so only the total of 32 applies, and a proxy's own rate limits are the defense.
- **A leaked hub key,** or a leaked agent key, until it is replaced by a reset or the agent is removed.
- **A person's own account.** A program running as the person reads the keys (decision 99).
- **The clock.** A hub or agent clock far off makes signed messages expire or look too far ahead; `Hello` already warns above 5 minutes (§7.2).

## Crates

The maintainer pre-approved security crates on 2026-11-18, to be chosen for review and track record, not for being in the tree. None is added by this decision; the implementation PR (#567) adds them, with `just deny` run on the result.

| Crate | Version | Used for | Alternative, and why not |
|---|---|---|---|
| `ed25519-dalek` | 3.0.0 (default features `fast` and `zeroize`; no `rand_core`, `serde`, `pem` or `batch`). Brings `curve25519-dalek` 5 and `ed25519` 3; `sha2` 0.11 and `subtle` are in the tree | Signing and verifying with `verify_strict`, which refuses non-canonical signatures and small-order keys, and constant-time arithmetic. The signing key wipes itself on drop. The seed comes from the `Random` port. `technology.md` §8.2 already names it | `ring` or `aws-lc-rs`, both in the tree and sound, but picked for TLS, not as this project's signature library, and their keys' wiping isn't what the maintainer asked for by name. `ed25519-dalek` 2.2.0 has the longer record in use; take it if the maintainer prefers that line to 3.0 |
| `rustls` | 0.23, a direct dependency, with the provider the workspace already builds it with (the lock file has `ring` and `aws-lc-rs`) | TLS 1.3 only, the pinned-key verifier (which checks `CertificateVerify`), the exporter value | None: it is already how `reqwest` does TLS |
| `tokio-rustls` | 0.26 | The server and the client over `tokio`, with the connection in reach for the exporter | `axum-server`, a new package, with less control over the connection |
| `rustls-platform-verifier` | 0.7 (in the tree through `reqwest`) | The agent's CA validation of a proxy's certificate, with the operating system's trust store, as `reqwest` does | `webpki-roots` (a fixed list, no private CA) or `rustls-native-certs` |
| `rcgen` | 0.14 (`default-features = false`, feature `aws_lc_rs` or `ring`, to match the provider; no `pem` or `x509-parser`). Brings `yasna`; `time` and `rustls-pki-types` are in the tree | Making the built-in listener's self-signed certificate once and again from the same key (ECDSA P-256, which every browser takes). Maintained by the rustls organization | Writing the ASN.1 by hand, `openssl` the program, or the person bringing a certificate (that is the proxy mode) |
| `tokio-tungstenite` | The version `axum`'s `ws` uses (0.29), no TLS feature; the workspace's 0.26 serves tests today | The agent's WebSocket client over a stream `tokio-rustls` made, which stays in reach through `get_ref()` | `tokio-tungstenite`'s own `rustls` feature would also expose it, but picks its own provider and verifier; keeping 0.26 leaves two copies of the crate in a normal build |

Already in the tree and used as they are: `sha2` (the payload digest, the public-key fingerprint, the token checksum), `base64`, `uuid` (v7), `axum` (`ws`) and `zeroize`. No HMAC crate is needed: the token's key signs, so no secret is stored on the hub or sent. The implementation PR extends `technology.md` §8.2's crypto row to name the crates above.

### Needs approval (not security crates)

The maintainer pre-approved security crates only, so these wait for an answer. They draw and read the QR code in the web UI's WebAssembly and are not used for anything the connection's security rests on.

| Crate | Version | Used for | Alternative, and why not |
|---|---|---|---|
| `qrcode` | 0.14.1, `default-features = false` (no `image` or `svg`: the code draws the SVG of the web dialog and the Unicode half blocks of `ladderback pairing-token` from the same module matrix). MIT or Apache-2.0; pure Rust; 26 million downloads; the last release is July 2024 | Making the QR code of a token, in the web UI's WebAssembly and in the command; error correction level M for the dialog and L for the terminal (the token is about 190 characters) | `qrcodegen` (Nayuki's reference implementation, no dependencies, smaller and stricter); `fast_qr` 0.14.0 (faster, a more recent release, less used). `qrcode` is chosen for its record; swapping is a small change |
| `rqrr` | 0.11.0, `default-features = false` (no `img`). `(MIT OR Apache-2.0) AND ISC`, and `ISC` is in `deny.toml`'s allow list; depends on `g2p` and `lru`; released September 2026 | Reading a QR code from a canvas frame, in WebAssembly, so one decoder behaves the same in every browser and no frame leaves it | The browser's `BarcodeDetector`, where it exists: it isn't in every browser, so a fallback is needed anyway, and two paths would be two things to test. `rxing` 0.9.4 (a ZXing port, larger). `bardecoder` (last release 2023). A JavaScript decoder (`jsQR`) adds an npm package and code outside Rust |
| `web-sys` features | The existing `web-sys` (0.3.107 in the tree) gains `Navigator`, `MediaDevices`, `MediaStream`, `MediaStreamTrack`, `MediaStreamConstraints`, `HtmlVideoElement`, `HtmlCanvasElement`, `CanvasRenderingContext2d` and `ImageData`; `Clipboard` and a `Navigator` share call (through `js-sys`'s `Reflect`, as web-sys has no stable binding) for Copy and Share | The camera, the canvas the decoder reads, Copy and Share | Writing the calls in JavaScript: a file outside Rust, outside the tests and the CSP's review |

**Not yet verified:** neither `qrcode` nor `rqrr` has been built for `wasm32` here. Both are pure Rust with no system dependency, which should be enough, but the red `test:` commit of the component builds each for `wasm32-unknown-unknown` first; if `rqrr` doesn't build, `rxing` is next.

## Tests that pin it

Each is a test that fails before the behavior exists (§13.1).

- **The messages.** A signature over the bare nonce, hub's or agent's, is refused. A proof made for another hub key or another agent ID is refused. A signature of one label presented as another, the hub proof as the agent proof and the reverse, is refused. A proof from an earlier connection (other nonces) is refused. Another binding value or kind is refused. The encoding has fixed vectors, so changing it is a decision, not an accident.
- **Signed messages.** An unsigned, a wrongly signed, an expired, a replayed (same or lower `seq`) and a too-far-ahead message are each refused and change nothing. So are one signed for another agent or connection, one whose payload was altered, one with a padded or non-canonical encoding, and a `RunPlan` signature on a `RepoGrants` payload. A forged message doesn't advance the sequence. An unsigned, wrongly signed or replayed `ReposSnapshot`, `Heartbeat` or `Hello` is refused by the hub.
- **The hub's identity.** An agent refuses a hub whose key isn't the pinned one, and sends no proof and no `Hello`. A valid handover chain is followed, `Welcome` is checked against the key the chain reached, and the new pin is kept only after it; a broken, forged or over-long chain is refused. A server that presents the pinned certificate without its private key is refused.
- **The key's life.** A hub that starts with its key missing and agents registered refuses `/sync/v1` and makes no new key; it makes one only on a first start, and never overwrites a `hub-key` file.
- **Downgrade.** A `Welcome` with a lowered `protocol`, or an echo of a range the agent didn't send, is refused. A hub whose new handshake fails never makes the agent use this one.
- **Binding.** Built-in mode: a relay between two TLS sessions is refused on both sides. An agent recorded as kind 1 is refused with kind 0, and an agent paired with a pinned certificate never signs kind 0. A hub serves a kind 0 agent and a kind 1 agent side by side.
- **Pairing.** A token used twice, an expired one and one with a wrong checksum are refused; two pairings with one token at once give one success and one `token-used`. A taken hostname leaves the token unspent. A known ID with another key is refused, except under a replacement token. A wrong pairing key, a wrong agent key, or a mismatch of the token's hub key with the hub's, is refused, and the agent sends nothing of the token to a hub it didn't verify.
- **Copies.** A counter equal to or below the stored one, after a good signature, is `identity-conflict`, raises the alert and stops lease renewal. A copy connecting while the original is connected closes the original's session, and the original is then refused. A conflict stays after the counters agree, and a replacement token with a new key clears it and refuses the copy.
- **Who may pair.** Making a token and pairing are refused without a session, with an API token, and without a fresh step-up (a session's age doesn't count). Beyond loopback they are refused with sign-in off or in proxy-header mode (`pairing-needs-sign-in`), by `Host` as well as by peer. The page refuses to show or take a token on a page that isn't a secure context.
- **The QR and the camera.** The QR decodes to exactly the token's text. The camera is asked for only on pressing "Scan the code", is released on a read, a cancel and a page leave, and no frame is stored. A scanned text that isn't a token is dropped. Scanning is offered only where the camera API and a secure context are; paste works without them. The QR goes with the token when it is used or lapses. The QR's text alternative doesn't hold the token.
- **The share and the command line.** The share carries the token as text only. `ladderback pair` refuses a token on argv. `ladderback pairing-token` prints text only when standard output isn't a terminal and with `--no-qr`; with a terminal it prints the text and a code whose half blocks decode, in a test, to exactly the token's text, with a 4-module quiet zone and explicit colors; the token and the code are in no log line of the daemon; and the command refuses to run as another operating-system user than the one that owns the data folder.
- **Bounds and secrets.** An oversized or late pre-authentication frame closes the connection; a fifth unauthenticated connection from one address in built-in mode is refused. A canary repository password appears in no frame (decision 3, I4).
- **Where they run.** The encoding and the checks in `domain` and `app` with fakes; the signing port has a contract suite over its fake and its real adapter; the TLS binding and the pairing run `@e2e` against a real listener; the QR and the scanner run `@ui` in the three engines, with a fake camera stream.

## Appendix: the scenarios `fleet/pairing.feature` takes

Both Rust runners fail a scenario with an undefined step (`fail_on_skipped`, §13.1), so the file can't be committed ahead of its steps. #567's red `test:` commit adds it with these as its first scenarios. The protocol's own refusals (a bare nonce, a proof for another hub) are unit tests above, not scenarios.

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

  Scenario: A copy made while the original is connected is caught
    Given an agent connected to its hub
    And a second machine has a copy of its data folder
    When the second machine connects to the hub
    Then the original's connection is closed
    And the original is refused when it connects again
    And the hub raises an alert naming the agent

  Scenario: A conflict is resolved by pairing the real machine again
    Given an agent whose identity is in conflict with a copy
    When the person makes a replacement token for that agent
    And the real machine pairs with it
    Then the hub clears the conflict and renews the agent's leases
    And the copy is still refused

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

  Scenario: A hub whose key was reset is refused until its agents pair again
    Given an agent paired with a hub
    When the hub's key is reset without a handover
    Then the agent refuses the hub
    When the agent pairs again with a new token and confirms the new hub key
    Then the agent connects and keeps its record

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

  Scenario: A report the hub decides on can't be altered on the way
    Given an agent connected to its hub through a proxy
    When the proxy changes the agent's list of repositories
    Then the hub refuses the report
    And the hub grants nothing from it

  Scenario: A downgrade of the protocol is noticed
    Given an agent and a hub that both speak two protocols
    When the agent's range is lowered on its way to the hub
    Then the agent refuses the hub's answer

  Scenario: The hub never receives a repository secret
    Given an agent with a repository whose password is a canary value
    When the agent pairs and syncs with the hub
    Then the canary appears in nothing the hub received

  Scenario: A laptop pairs by scanning the code on a phone
    Given a phone signed in to the hub
    And the person confirms with their password
    When the phone shows a new pairing token as text, as a code and with a share button
    And the laptop's pairing page scans the code with its camera
    Then the page shows the hub's address and key fingerprint
    When the person confirms with their password
    Then the hub lists the laptop

  Scenario: An agent is paired from a phone by scanning the hub's code
    Given a hub showing a pairing code
    And a phone signed in to an agent
    When the phone scans the code with its camera
    And the person confirms the hub's key fingerprint and their password
    Then the agent is paired with the hub

  Scenario: A token is shared from the phone and pasted on the agent
    Given a phone signed in to the hub that has a share sheet
    When the person makes a token and shares it
    And the text is pasted on the agent's pairing page
    And the person confirms with their password
    Then the agent is paired with the hub

  Scenario: Pairing by pasting when there is no camera
    Given an agent whose pairing page can't use a camera
    When the person pastes a pairing token
    Then the page says scanning isn't available and why
    And the agent pairs with the hub

  Scenario: Pairing by pasting when the person declines the camera
    Given an agent whose pairing page is offered a camera
    When the person chooses to scan and declines the camera
    Then the page says the camera was declined
    And the person can still paste the token

  Scenario: The code goes away when the token is used
    Given a hub showing a pairing code
    When an agent pairs with the token
    Then the dialog no longer shows the code, the text or the copy and share buttons

  Scenario: The code goes away when the token lapses
    Given a hub showing a pairing code
    When the token's life runs out
    Then the dialog no longer shows the code, the text or the copy and share buttons

  Scenario: A token can't be made without a recent confirmation
    Given a phone signed in to the hub
    When the person asks for a new pairing token without confirming with their password
    Then the hub refuses until they do
    And no token exists

  Scenario: A remote page can't pair when sign-in is off
    Given an agent with sign-in off that is reached from another computer
    When the person asks to pair it
    Then the agent refuses and says to set an admin password

  Scenario: The hub's command prints a token and a scannable code to a terminal
    Given a hub whose command is run in a terminal
    When the person runs the hub's pairing-token command
    Then it prints a pairing token as text
    And it prints a code of the same text that a camera can read
    And it says that the token is one-time and when it lapses
    And the hub's log holds neither the token nor the code

  Scenario: The hub's command prints text only when its output is piped
    Given a hub whose command's output is piped to another program
    When the person runs the hub's pairing-token command
    Then the other program receives the token as text and nothing else

  Scenario: The hub's command prints no code when it is turned off
    Given a hub whose command is run in a terminal
    When the person runs the hub's pairing-token command with the code turned off
    Then it prints a pairing token as text and no code

  Scenario: An agent is paired from a phone by scanning the code in the hub's terminal
    Given a hub on a headless machine reached over SSH
    And a phone signed in to an agent
    When the person runs the hub's pairing-token command in the terminal
    And the phone scans the code in the terminal with its camera
    And the person confirms the hub's key fingerprint and their password
    Then the agent is paired with the hub

  Scenario: A headless agent is paired with a token pasted from a phone
    Given a headless machine reached over SSH
    And a hub showing a pairing token on a phone
    When the person shares the token from the phone to the computer with the terminal
    And the person pastes it at the agent's pairing prompt
    Then the agent is paired with the hub

  Scenario: The agent's command refuses a token given as an argument
    Given a headless machine reached over SSH
    When the person gives a pairing token as an argument of the agent's pairing command
    Then the command refuses it and says how to give it
```
