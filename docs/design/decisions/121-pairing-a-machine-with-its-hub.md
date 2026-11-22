# Decision 121: How a person pairs a machine with its hub

**A person pairs a machine by moving one pairing token from the hub to the agent: as text, as a QR code of the same text (in the web UI, or drawn in a terminal by `ladderback pairing-token`) or through a phone's share sheet. Both ends show the same key fingerprint, in one rendering, to compare. The agent then waits for a person to approve it on the hub, which tells the person that a machine is waiting. Every error says what to do next, and no step needs a camera, a mouse or color. The hub's recovery seed is shown as a Recovery Kit, one printable page the person stores and confirms** (recorded 2026-11-18, §7.1, §4.5, §13.7.5; #567, the maintainer's directions of 2026-11-18 on pairing by QR code, from a phone and from a headless machine, and on the Recovery Kit; revised the same day after independent review).

[Decision 120](120-agent-hub-connection-security.md) decides what is secure: the token's format, its server side, pending approval, the step-up, the keys, the recovery seed and the handshake. This decision decides what the person sees and does. It links 120 for every security rule rather than restating it; where the two seem to differ, 120 governs.

## Where each item is answered

| Item | Answered in |
|---|---|
| The token as text, a QR code and a share | "The hub's dialog", "The QR code" |
| The camera and the scanner | "Scanning" |
| A phone as either end | "From a phone" |
| A headless hub or agent over SSH; `pairing-token`, `pair` | "From a terminal" |
| What the person compares, and where | "The fingerprint" |
| A new agent waiting for a person's approval; what each side shows | "Waiting for approval" |
| The recovery seed, as a Recovery Kit | "The Recovery Kit" |
| What each error says | "Errors and their next step" |
| Where a token can leak | "Where a token can leak" |
| Keyboard, screen-reader and color-free use | "Accessibility" |
| Which shared component each screen is built from | "Shared components" |
| The QR crates | "Crates" |

## The fingerprint

One rendering of a key's fingerprint is shown wherever a person compares one: **20 Crockford base32 characters in five groups of four, upper case, with hyphens** (`7K3Q-M9XD-2RTV-8HFA-QW5N`), the first 100 bits of the SHA-256 of the key ([120, "The hub key"](120-agent-hub-connection-security.md)). The function is `domain`'s; this decision only says where it is shown.

- **The hub key's fingerprint** is in the hub's dialog, in `ladderback pairing-token`'s output, in Settings and its step-up with sign-in off, and on the agent's confirm step, always in full. The person compares the agent's screen with the hub's.
- **The agent key's fingerprint** is on the hub's pending row and approval, and on the agent after it has paired. The person compares those two.
- **Two IDs, each named for what it labels** ([120, "The hub key"](120-agent-hub-connection-security.md)). The **agent ID** is the UUIDv7 the agent made, shown in full on a pending row and in an agent's details. The **token ID** (the first 8 hex digits of a pairing key's SHA-256, `3F9A8B21`) names an unused token in the list and in `--revoke`. A key has no other ID than its fingerprint: no screen shows a key ID beside it, so a person never meets two names for one key (#1344).
- **No ID is a fingerprint, and none tells a lookalike name apart.** No screen asks a person to verify with one. An agent ID is chosen by the agent, so a rogue can make it look right. The comparison is always the full fingerprint.
- **The full 64 hex digits** are one control away ("Show all") and in the command's `--long` output, for a person who wants to check more than the groups.
- **A screen reader speaks a fingerprint character by character.** A fingerprint is a group named by its caller ("Hub key fingerprint"). Each character is its own span with `role="img"` and an `aria-label` that is that character alone, and the hyphen between groups is `aria-hidden` and drawn as a gap. So the reader pauses between groups, never says "dash", and never reads `M9XD` as a word. The text stays selectable, and a copy is the plain string. The full form behind "Show all" has the same markup.
- **A fingerprint wraps between groups, never inside one,** at 320 CSS px with 200 % text (1.4.10), as the token breaks anywhere. A test reads the accessibility tree of a fingerprint and expects one name per character.
- **Base32 stays, and words are not offered (the maintainer's decision on #1048, 2026-11-21).** Words would replace this rendering, which breaks the test vectors and the reflow test of decisions 120 and 121, or add a second view of the same bits that every screen would name, test and keep in step, with a reviewed word list for each language once Japanese arrives in M7. Ten words from a 2048-word list carry the same 100 bits and are no shorter to compare. The one trigger to revisit is the pairing test on the 0.7.0 release candidate (#1044) showing people misreading characters. Then an `en-US` word view may be added as an extra, never a replacement, with its vectors fixed in `domain`.

## The hub's dialog

**Settings → Agents → New pairing token** asks for a life (15 minutes by default, 5 minutes to 24 hours) and an address with its kind, then shows one token three ways from the same bytes: as text, as a QR code and, where the browser has a share sheet, a Share button. The kind is a choice beside the address: the hub's own certificate (pinned) or a certificate authority's, which is a proxy's. It defaults to the kind of the listener the page came in on, so a person who edits the address to a proxy's chooses the second, and the agent never meets a pinned token for an address it can't pin. Making a token is a high-impact action under [120, "Who may make a token, approve or re-key"](120-agent-hub-connection-security.md); this decision doesn't change its rules.

- **The dialog shows,** above the token: the hub key's fingerprint in full, the address, and when the token lapses, as a local time with the UTC offset and the time left ("14:47 UTC+01:00, in 15 minutes"). The lapse time and the seconds left come from the hub's response, and the page counts down on its own monotonic clock. It never compares the hub's time with the browser's wall clock, so a wrong clock can't show a live token as lapsed.
- **A loopback or plain `http://` address gets a warning.** The address is what the person was using and is editable, since agents may reach the hub another way. A loopback address (`localhost`, `127.0.0.1`, `[::1]`) can't be reached from another machine, and an agent refuses a plain `http://` address that isn't loopback ([120, "Binding to the TLS session"](120-agent-hub-connection-security.md)). Either way an Alert in the Warning tone says so and asks for the address agents will use. Making the token stays possible, for an agent on this machine. `ladderback pairing-token` warns the same way ("From a terminal").
- **The text** is one unbroken string, selected whole by one click or tap, and wrapped by breaking anywhere, so a copy has no added line breaks. Focus starts on Copy, as in the API-token dialog.
- **Copy** puts the text on the clipboard and says so in a status line. **Share** carries the text only.
- **Cancel this token** is in the dialog. **Escape and Close revoke an unused token** as well, and the dialog says so beside Close, because its text can't be shown again ([120, "Pairing"](120-agent-hub-connection-security.md): shown once). A token made by the command line is revoked from Settings or with `pairing-token --revoke`.
- **When the token is used or lapses,** the dialog replaces the token, the QR code, Copy and Share with a status line inside it (`role="status"`), so a screen reader hears it while the modal is open. When used, it names the machine and asks for the comparison, with the full fingerprint: "laptop paired at 14:32 with key 5H2R-QW5N-7K3Q-M9XD-2RTV. Compare it with the key on laptop's own screen, then approve or reject it below. If you didn't expect this machine, reject it." When lapsed: "Lapsed at 14:47. Make a new token."
- **Focus moves with the content.** After use it goes to the new agent's row in the dialog (a focusable group with its hostname, agent ID, key fingerprint, Approve and Reject); after a lapse or a cancel it goes to "Make a new token", as the shared `hand_focus_when_gone` does for any control that vanishes.
- **Approval is in the same place.** The pending row shows the agent's hostname, agent ID and key fingerprint in full, and the person compares them with the agent's own screen before approving, with the step-up of 120 ("Waiting for approval" says where each control stands).
- **A name outside ASCII is noted once, for the whole name.** The name is shown as it is, and a note beside it says it holds characters outside ASCII, with a "Show characters" disclosure that lists each one by code point (U+0430) and script. The note is information, not a warning, so a curly apostrophe (U+2019), which macOS puts in names, isn't flagged as dangerous; punctuation is listed as such. The warning is the hub's lookalike flag ([120, "Pairing"](120-agent-hub-connection-security.md)): "Looks like laptop, held by another agent, last seen 3 days ago", in Warning tone with words, on the pending row and in its approval. A terminal shows the same note and the code points after the name.
- **A list of unused tokens** is a Table in Settings → Agents: each with its token ID, who made it, its life and a Revoke button named for its row ("Revoke token 3F9A8B21"). Revoke opens a Review dialog, as an API token's does, named "Revoke token 3F9A8B21" and described by what follows (an agent that pairs with it is refused as revoked); it starts on Cancel and its confirm button reads "Revoke token 3F9A8B21". A rotation of the hub key voids them and the list says so.
- **A secure context is needed to show or take a token.** Where the page isn't one, it says so in text and shows nothing ([120, "Who may make a token, approve or re-key"](120-agent-hub-connection-security.md)).

**What is built so far (#567).** Settings → Machines is a tab only in a build with the hub ([122](122-hub-out-of-releases-until-m8.md)) and on a daemon serving as one. It makes a token, confirmed by the step-up of 120, and shows this dialog: the hub key's fingerprint in full, the address with its warning, the lapse time with the time left counted on the page's monotonic clock, the token in a read-only field, Copy, Cancel this token, and Escape and Close cancelling an unused token, which the page then says, as it says a cancel the hub refused.

A cancel still on its way when the dialog is left answers for it. A lapse replaces the token with its status line, and focus goes to "Make a new token"; closing the dialog returns it to "Make a pairing token". Leaving the page, or closing the browser, with the dialog open doesn't cancel the token: it works until it lapses.

The tab is named Machines; the sections above, written before it was built, call it Agents. A token the hub makes once Settings is left, which no one has seen, is canceled. Once a later token is shown, an earlier token's cancel is said only if it failed, as a cancel of an earlier unused pairing token. What a cancel came to once the dialog is closed is said through the announcer, politely, and shown in a line of text above the form that is no live region, so a screen reader is told it once (§4.5, #1291). The form that makes a token is no dialog, so its step-up field is no live region either: its "Copied." is said through the announcer, and a wrong step-up is the field's own error, which focus reads as it moves to the field (§4.5, "Step-up field", #1294).

The address's kind follows the hub's built-in TLS (#1210). When it serves its certificate, the dialog offers both kinds in a radio group, the hub's own certificate and a certificate authority's, and chooses the hub's own first on a page that came over the built-in TLS and a certificate authority's on one that came in plain, as through a proxy. Choosing the hub's own makes a token pinned to its certificate, rewrites an `http://` address to `https://` and says the token is pinned. When the hub serves no certificate (its TLS key can't serve, [120, "The hub key"](120-agent-hub-connection-security.md)), the kind is a certificate authority's only, and the page says no token can pin one.

With sign-in off, the tab warns in a warning Alert that a hub reached through a reverse proxy must turn sign-in on, with a link to the guide's section "A hub behind a reverse proxy", which says the same ([120, "Who may make a token, approve or re-key"](120-agent-hub-connection-security.md), #1227). The link is to the guide on `main`: only a build that isn't a release has a hub ([122](122-hub-out-of-releases-until-m8.md)), and its release's tag predates the section. The warning shows wherever the step-up is the typed fingerprint, the case a proxy that rewrites `Host` defeats; with sign-in on it doesn't show.

Approval is built on the running hub's Settings → Machines, above "Make a pairing token". A "Waiting for approval" Table lists each waiting machine's hostname, agent ID, key fingerprint in full and how long it has waited, with a note naming the other machine when its hostname looks like one ([120, "Lookalike hostnames"](120-agent-hub-connection-security.md)); an "Approved machines" Table lists the approved ones and how long ago. "Approve laptop" opens a Review dialog, "Approve laptop?", with the fingerprint in full, the lookalike warning and the Step-up field, asked for each time; it starts on Cancel. The page approves the agent ID under the key it listed, so a machine that paired again with another key in between is refused (`agent-unknown`), as is one approved already (`agent-approved-already`).

The hub sends the page no event when a machine starts to wait, so Settings reads the list again every 5 seconds while it is open, on any of its tabs. A "Pause live updates" button above the tables holds both still, rows and waiting times as they were at the press, while the reads go on and its banner counts the rows that changed (A3; decision 65); pressing it again shows the latest list at once and says how many machines started waiting meanwhile, and leaving Settings ends the pause. It shows no time left, since the hub keeps no lapse for a pending machine yet, and has no Reject. While an approval is on its way the review holds still. "laptop is approved." is said through the announcer, and focus goes to the "Waiting for approval" heading, not to the next row.

The QR code and Share, the used token's status line and the agent's row, Reject, a new key waiting beside an approved agent, the Banner and the announcer's arrival, the list of unused tokens, "Show all", the refusal on a page that isn't a secure context, and the agent's pairing page are not built yet.

## Waiting for approval

Every new agent is held pending until a person approves it ([120, "Pending approval"](120-agent-hub-connection-security.md)). This section is what each side shows while it waits.

- **Approving in the pairing dialog.** The new agent's row has Approve and Reject, named for it ("Approve laptop"). Approve opens the Step-up field in the same row, inside the same dialog, with no second modal on top of the first. Focus goes to the field, Escape or Cancel returns it to Approve, and a wrong password is a `role="alert"` in the dialog (`step-up-failed`). Reject needs no step-up: it turns the buttons into "Reject laptop" and "Keep waiting", the second focused. The dialog's status line says each outcome.
- **Approving in Settings → Agents.** A "Waiting for approval" section stands above the agents list. It is a Table, with the hostname, agent ID, key fingerprint in full, how long it has waited and how long is left, and row buttons named for the agent. Approve opens a Review dialog, "Approve laptop?", that holds the full fingerprint and the Step-up field and starts on Cancel; Reject is a Review dialog too. After either, focus goes to the next row or the section's heading.
- **A new key waiting for an agent that is already approved** shows as "New key waiting" on that agent's row, with both fingerprints and the same two actions. Approving swaps its key and closes the old key's live connections; rejecting discards the new key and leaves the agent as it was ([120, "Pairing"](120-agent-hub-connection-security.md)). A new key lapses after 7 days like a pending agent, counts toward the limit of 20, and is in the Banner's count.
- **A Banner on every hub page** says "3 machines are waiting for approval" while any wait, new agents and new keys counted, with a link to the section. Its edition carries the count and the arrival time of the newest, so dismissing it hides only that edition and a new arrival shows it again. It is Primary, and Warning in the last 24 hours of any agent's wait.
- **An arrival is said politely.** A `UiEvent::AgentPending` goes through the announcer, which says "laptop is waiting for approval", or a count when several arrive within 5 seconds. A Banner isn't a live region, so this is how a screen-reader user on another page hears of it. Inside the pairing dialog the announcer is inert, so the dialog's own status line says it.
- **The `agent.pending` event** (§11) goes to the person's notification rules. Channels are opt-in, so without a rule the Banner, the announcement and the list are all there is.
- **The hub's log says that a machine paired** (#1307), so a person watching it, a container's or the service's, learns of a pairing they didn't expect. One INFO line, written as the pairing is kept: `machine "laptop" paired as agent 01900000-0000-7000-8000-000000000001 with key 4HNE-XF2T-0K7N-0TPM-W9G0; it waits for approval`, the hostname, agent ID and key fingerprint a person compares with the machine's own screen. A machine that pairs again says `paired again` and whether it `still waits for approval` or `is approved already`; a pairing repeated on the token it spent says `nothing changed`. The line holds no token, pairing key or seed (I4).
- **The pairing line names the hostname the hub holds,** which its pages list, followed by the one asked for when they differ: `machine "laptop" (asked as "desktop") paired again as agent …` (#1342). Each is escaped with `escape_debug`, as the other-hosts warning escapes a host name, so a quote, a backslash and an apostrophe each show with a backslash before them.
- **The hub's log says when a person approves a machine, and when it first connects approved** (#1342). An approval on the page writes one INFO line naming the machine as the pairing line does: `machine "laptop" approved as agent … with key …`. The hub's terminal keeps no log: `pairing-token --approve` says the approval in its own words on standard error. The machine's first proof after its approval writes `machine "laptop" connected as agent … with key …, its first connection since it was approved`. Later connections log nothing, so a machine that reconnects all day adds no lines. The log says nothing of a refused pairing yet.
- **"First" needs no state of its own:** the record is approved and the hub hasn't seen the machine since the approval, and each proof sets when it was last seen. The registry reads that from the record in the step that keeps the proof, so an approval that lands while the hub checks the proof still counts.
- **A pending agent lapses after 7 days** ([120, "Pending approval"](120-agent-hub-connection-security.md)). Each row shows how long it has waited and how long is left, both from the hub's response. In the last 24 hours the row says so in words, and the Banner takes the Warning tone.

### What the agent shows

- **While it waits,** the agent's page shows a status Alert, "Waiting for approval", and the Steps of "Token, Connect, Approval" at the third. It names the hub's address, says a person must approve this machine under Settings → Agents on the hub, shows the agent's own key fingerprint in full to compare with the hub's row, the hub key's fingerprint and when the wait lapses, and says the machine keeps backing up on its own schedule.
- **On approval** an Alert in the Success tone says "Approved by the hub", and the announcer says it once, politely. Steps shows all three done. Focus doesn't move.
- **On rejection or removal** (`Removed`) an Alert in the Error tone says the hub rejected or removed this machine and that it keeps backing up on its own. An ordinary token is refused as `agent-removed` from then on, so the alert says to ask for a replacement token.
- **On expiry** (`pending-expired`) an Alert in the Warning tone says nobody approved this machine within 7 days and the hub dropped it. It keeps its identity: make a new token and pair again.
- **`ladderback pair` waits with the same words** ("From a terminal").

## The QR code

The QR is the token's text, drawn in the browser from the token the page already holds, by a pure-Rust encoder compiled to WebAssembly ("Crates"). No network service draws it, and the daemon is asked for nothing more. It adds no secret and no format.

- **The SVG is black on white,** whatever the theme, with its own white quiet zone of 4 modules, so a dark theme can't invert it. The elements set `forced-color-adjust: none`, because Windows high-contrast mode repaints `fill` and can invert a code.
- **The error-correction level is L,** unless M keeps the same version, since a screen isn't damaged and a denser module is harder for a webcam. For a 191-character token, L is version 8 (49 modules) and M version 10 (57 modules).
- **The code scales with text size, within bounds.** A module is never drawn smaller than 4 CSS px, so a code of 49 modules is at least 196 px, 228 px with its quiet zone, and the SVG sets `shape-rendering: crispEdges` so no edge blurs. Its `max-inline-size` is 100 % of the dialog, so at 320 CSS px with 200 % text it shrinks to fit and never scrolls sideways. Where that leaves a module under 4 px, the dialog says "The code is too small to scan on this screen; use the text" instead of drawing a code that can't be read. It prints sharply; a print rule hides the token, the QR code and Copy, so a printout of the dialog holds none of it ("Shared components" says how that differs from the kit).
- **A Wake Lock is held while the dialog is open,** where the browser has one, so a phone doesn't lock mid-scan. The browser releases it when the tab is hidden, so the page asks again on `visibilitychange` when it becomes visible, and releases it on close. A browser without one loses nothing else.
- **The QR's text alternative** says what it is, "a pairing code for this hub", and never holds the token.
- **The token's format is not changed for size.** A denser encoding (alphanumeric mode, with a longer upper-case token and a new prefix) gains at most one version and can't reach an 80×24 terminal; the only real shrinks are braille or sextant cells, which are too fragile to scan, or dropping the certificate pin, which is a security decision for 120.
- **It is never the only way.** The text, Copy and the paste field stay (A1, A2 and A5 of §0; §4.5). The QR is shared component work under §13.7.5, built in its own PR before the pairing page.

## Scanning

The agent's pairing page takes the token pasted or scanned. The scanner is a shared component, built in its own PR.

- **The camera is asked for only when the person presses "Scan the code",** never when the page opens, and is released at once on a read, a cancel, an error, "Stop scanning" and when the page is left. On a phone it asks for the rear camera (`facingMode: environment`).
- **Frames stay in the browser.** They go to a video element and a canvas, and a decoder in WebAssembly reads them; the page keeps none, and sends, stores and logs none. The page's policy (`default-src 'self'`) gives it nowhere else to send them, and no `Permissions-Policy` header on the pairing page may deny the camera to its own origin.
- **Decoding is throttled** to at most five frames a second, on a frame scaled to at most 640 pixels on its longer side, so Escape and typing never lag. A mirrored preview is a display setting only: the decoder reads the unmirrored frame.
- **What is read is checked before it is used:** the prefix, the checksum and the version. Only then does the page show the hub address, the hub key's fingerprint and the mode (CA or pinned) for the person to confirm; the token goes to the daemon only after that.
- **The status line always says something,** in a `role="status"` region: "Waiting for permission to use the camera" while the browser's prompt is open, with no timer running; "Looking for a code" once the camera is on; after 30 seconds of looking, "No code found. Move closer, raise the screen's brightness, or paste the token." (it keeps looking); "That code isn't a Ladderback pairing token" for a code that isn't one, at most once every 5 seconds; "Code read: hub 7K3Q-M9XD-2RTV-8HFA-QW5N" on a read, with the fingerprint marked up as "The fingerprint". After a read focus moves to the confirm panel's heading.
- **Where the camera can't be used the page says why,** in text, and the paste field stays: no camera, "the camera is in use by another app", permission declined, no secure context, no camera API.
- **A secure context is needed for the camera.** `localhost` and loopback are secure contexts, so an agent's UI on its own machine can scan; a remote UI needs HTTPS ("From a phone").
- **Scanning is optional and reachable by keyboard and touch:** a "Stop scanning" button, visible while the camera is on and named for what it does, which is the only way to stop on a phone; Escape also stops it. The live status is there, and no part of any task rests on seeing the picture.
- **The preview is decoration.** The `<video>` element is `aria-hidden="true"` and has no name, so a screen reader doesn't announce a feed it can't describe; the status line and the buttons carry everything.

## From a phone

The token still travels from the hub to the agent, and a phone may be either end, in the installed web app (#394) or a browser.

- **A phone signed in to the hub** makes the token and shows it as text, Copy and QR, as the desktop dialog does, and a Share button where `navigator.share` exists. The share carries the token as text only, so the system's share sheet (AirDrop, messages, a password manager) can take it to the agent's machine, where it is pasted or scanned off the phone's screen by a webcam.
- **A phone signed in to an agent** reads, with its camera, the QR on the hub's screen or another phone's, then pairs that agent. Paste stays.
- **The secure context.** `getUserMedia` and `navigator.share` need one, and a phone can't reach the loopback address of a computer, so the phone needs HTTPS to the hub or the agent. The setup is #394's: a reverse proxy with a certificate the phone trusts, sign-in on and `--allowed-host`. The built-in self-signed certificate is the open case.
- **What is verified.** From MDN: a document is a secure context when its URL is `https`, `localhost` or loopback, and `navigator.share` needs a secure context, the `web-share` permission policy and a user's click. From the WebKit bug tracker and a vendor's notes: on iOS a Home Screen web app can use the camera since iOS 13.3, but it asks for permission again on each launch and has had reliability bugs. From #394: web push on iPhone needs the app installed from Safari's Home Screen, from iOS 16.4.
- **What is inferred, not tested:** whether a browser treats an `https` page whose self-signed certificate the person clicked through as a secure context for the camera and the share sheet. Don't rely on it: the UI feature-detects `isSecureContext`, `mediaDevices.getUserMedia` and `navigator.share` and offers each only where it is there, and a self-signed hub on a phone falls back to paste. The red `test:` commit for the scan component records what real iOS and Android browsers do.
- **The fallback is always paste,** from Copy, from a share or from a password manager.

## From a terminal

A headless machine reached over SSH has no browser. The token still goes from the hub to the agent, and a phone can be the other end. Both commands run as the operating-system user that owns the data folder, and need no sign-in and no step-up, as `set-password` doesn't (decision 99, decision 70; [120, "Who may make a token, approve or re-key"](120-agent-hub-connection-security.md)).

**What is built so far (#567).** `pairing-token` writes the hub's store from its own process, so it runs only while the daemon is stopped, and refuses while the daemon holds the data folder (§9: one process per data folder); a control channel to the running daemon, or the web dialog, lifts that later. It always behaves as `--no-wait`. For an `https://` address it makes a token pinned to the certificate of the hub's built-in TLS, taking the TLS key as `serve --hub` does (making it at the first start), and with `--certificate-authority` a token of a CA-checked address; a loopback `http://` address pins nothing. A hub whose TLS key can't serve makes no pinned token and says how to restore the key.

The times `pairing-token` prints (the new token's "usable until", `--list`'s lapse time and `--pending`'s "since when") are written through the catalog in the zone this computer is in, in words, with that zone's offset beside them: `usable until November 22, 2026 at 6:59 AM (-05:00)`, as the web dialog does since #1271 (#1309). A computer whose zone has no IANA name reads UTC, with UTC's offset.

Since no command waits for the pairing yet, a machine that paired stays pending until a person approves it on the running hub's Settings → Machines (above), or at the hub's terminal with the daemon stopped as for a token. `pairing-token --pending` lists the machines waiting, one line each on standard output, its columns separated by two spaces: agent ID, key fingerprint, `waiting since` and the time in words, and, last because it alone may hold spaces, the hostname. A script splits a line at two spaces, at most three times. When none waits it says so on standard error.

`pairing-token --approve <hostname>` finds the machine whose hostname compares equal on the hub, names it as the waiting prompt below does ("laptop (agent ID …) is waiting with key 5H2R-…. Compare it with that machine's own screen."), asks `Approve it? [y/N]` and approves the record it named: its agent ID and key, so a machine that paired again with another key in between is refused rather than approved unseen. With standard input not a terminal, only `--yes` approves. An unknown hostname, or a machine approved already, is refused with exit 1 and nothing changes. The approval records when, not yet who ([120, "Pending approval"](120-agent-hub-connection-security.md)). The note on a name outside ASCII and the lookalike flags are not built at the terminal yet; Settings → Machines approves without the stop and start.

`pair` reads the token at the prompt or as standard input and, once the hub accepts it, says whether the hub holds the machine pending or approved (below) and returns. When the hub holds the machine under another hostname than the one it asked for, because it paired before under a different one, the line names the hostname held and the one asked for. Waiting for approval in the terminal, the QR code, `--token-file`, `--replace-pin`, a local-time expiry and the refusal of a token on argv are not built yet. `hub-status` is built as described below, and so is `pair <address>` on a machine that paired, with no `--kind` yet.

A machine the hub holds approved already, as one that lost its `hub-pin` and pairs again, is told so and that it needs no approval, under the hostname held (#1341). `Paired` carries the status the hub holds, unsigned like the hostname; a hub that predates it sends none, and a status the machine doesn't know reads as none, so the machine says pending as before, and a machine that predates it ignores it.

`serve --hub` serves its built-in TLS (#1210), but the messages for `hub-untrusted`, `plain-address`, `binding-unavailable` and `hub-tls-refused` still offer only the CA-checked way: a proxy that serves https with a certificate an authority signed. The table's built-in-TLS alternatives join them in a later change.

### `ladderback pairing-token` (on the hub)

- **It makes one token,** as the dialog does, with `--ttl` (5 minutes to 24 hours; 15 by default) and `--address`. With no address configured and none given, it asks for one.
- **It prints,** in this order: a line with the hub key's fingerprint ("Hub key: 7K3Q-M9XD-2RTV-8HFA-QW5N"), so a headless person has something to compare; the token as text; a line saying the token is one-time, its token ID and when it lapses, as a local time with the UTC offset and a relative time; and, when the code applies, the QR code. A loopback or plain `http://` address gets the dialog's warning first ("The address is loopback: another machine can't reach it. Use --address with the address agents will use").
- **`--list` and `--revoke <token ID>`** list unused tokens by token ID with their lives, and revoke one ([120, "Pairing"](120-agent-hub-connection-security.md)); the token ID is in the output above, so a revoke needs no listing first.
- **In a terminal it waits for the pairing,** until the token lapses, and says when an agent has paired: "laptop (agent ID 0194…) is waiting with key 5H2R-QW5N-7K3Q-M9XD-2RTV. Compare it with the agent's own screen. Approve this agent? [y/N]". `y` approves; anything else leaves it pending, to approve from the Agents list. The prompt names the hostname and the full fingerprint, notes a name outside ASCII as the dialog does ("The hub's dialog") and says what the hub flagged as a lookalike. `--no-wait` prints and returns. **Ctrl-C revokes the token** if it is unused, says so, and exits with 130; once the token is used it only stops waiting and the agent stays pending.
- **A pipe or a file gets the token and nothing else on standard output.** When standard output isn't a terminal, the command prints the token alone there, doesn't wait and doesn't prompt, so a script receives exactly the token. On standard error it still prints the hub key, the expiry and the token ID, so a person running it over SSH has something to compare and to revoke, and the loopback warning. `--qr` draws the code to the terminal even where `NO_COLOR` is set, and where only standard output is piped but standard error is a terminal it draws it to standard error.
- **It is never logged.** The command writes the token to the terminal only. The daemon's log and its own output never hold a token, a QR code or the pairing key; the hub's event for a new token names its token ID and when it lapses, never the token.

### The terminal's QR code

- **It is drawn with Unicode half blocks** (`▀`, two modules to a cell) from the same module matrix as the dialog's SVG, so one crate serves both. Its colors are explicit: black foreground and white background by the 256-color indices 16 and 231, which themes don't remap, reset at the end of every line, with a quiet zone of 4 modules drawn in white and the row count padded to an even number.
- **Level L, unless M keeps the version,** as in the dialog.
- **It is text only when the person asked for text or color can't be trusted:** with `--no-qr`, `--plain`, `NO_COLOR` set, `TERM=dumb` or a standard output that isn't a terminal. The command says so in a line before the token ("No QR code: NO_COLOR is set; paste the token instead, or run with --qr"). `--qr` forces the code in any of these cases and says so if the terminal can't be trusted to draw it, since a person who set `NO_COLOR` for text may still need the code and has no other way back to it. The QR's colors are its data, so it never goes through decision 38's color stripping, and a stripped code that prints as unreadable glyphs is worse than none.
- **With the code, a line before the token** says "A QR code follows; skip it with --no-qr", and a screen-reader user, who can't skip a code in time, runs with `--plain`.
- **The size is computed from the actual token,** not assumed. A 191-character token is a version 8 code, 57 columns by 29 rows with its quiet zone, and one of 226 characters is 61 by 31; the longest token (a 200-byte address) is 89 by 45. Columns are the hard limit, since a wrapped code is corrupt; rows only cost scrolling.
- **When the window is too small the command waits** rather than giving up and minting a new token on a re-run. It prints the text, then "This code needs 57 columns by 29 rows; the window is 80 by 24. This token lapses in 11 minutes. Enlarge the window or zoom out, then press Enter, or q to skip the code", and measures again on each Enter, until it fits or the person skips. If it can't read the size, or a keypress can't be read (a dumb terminal), it skips the code and says why.
- **A later option: the Kitty graphics protocol** (Ghostty, Kitty) would draw a crisper image where the terminal reports it, with the half blocks as its fallback. It isn't in the first version and needs no crate beyond the image's bytes.

### `ladderback pair` (on the agent)

- **It never takes the token as an argument.** A token on argv is refused with a message that says how to give it, as the MCP token's is (§10.5): at the prompt, as standard input, or from `--token-file`. The maintainer's sketch `ladderback pair <address> <token>` is therefore not offered. The token carries the hub address; `ladderback pair <address>` overrides it for an agent that reaches the hub another way.
- **The prompt reads a paste without echo,** as `set-password` does (decision 70), and is tolerant: it strips whitespace, line breaks, zero-width characters, surrounding quotes and angle brackets, accepts the prefix in any case with `-` for `_`, and reads until the checksum validates or a blank line arrives, so a token wrapped across lines or pasted with a trailing newline isn't truncated. It can't repair a token's letters whose case a chat app changed (base64 is case-sensitive): the checksum catches that, and the error says so.
- **It echoes what it read before it asks.** "Read 191 characters. Hub https://hub.example:9899, key 7K3Q-M9XD-2RTV-8HFA-QW5N, pinned certificate. Compare the fingerprint with the hub's. Pair? [y/N]". Nothing is sent before `y`.
- **Piped, with no terminal to ask on,** the first pairing refuses without `--yes`: standard input is the token, so there is nobody to answer `[y/N]`, and the refusal says "No terminal to ask on. Compare the hub key above with the hub's own screen, then run again with --yes". `--yes` is for a first pairing only, and the hub still holds the agent pending.
- **`--hostname <name>`** pairs under another hostname than the machine's, for `hostname-taken`.
- **`ladderback pair <address>` on a paired machine gives it the hub's new address,** as [120, "Addresses and the transport statement"](120-agent-hub-connection-security.md) keeps one, and reads no token: an address carries no trust. It asks nothing and takes no `--yes`. The machine connects at that address alone, with the pin's certificate kind and fingerprint, and keeps the address in `hub-pin` only once the hub there proves the pinned key and its `Welcome` holds. It then says "The hub proved the key this machine pinned at https://hub-new.example:9899. This machine reaches it there from now on, and still tries the address in its token when that one doesn't answer", and the hub's answer as `hub-status` says it, exit 0.
- **A failure at the new address changes nothing.** The machine says the failure as `hub-status` would, then "This machine kept the hub addresses it had", and exits 1. Another hub at the address, which can't prove the pinned key, gets no proof. `--kind` isn't built: the address takes the pin's kind, since the hub accepts only its token's kind at `Proof` until it publishes others (120).
- **A machine the hub no longer knows pairs again there.** When the hub at the address proves the pinned key but refuses the proof (`proof-refused`: removed, or lapsed while pending), or the machine holds a pin but no key, `pair` says so and asks for a token, as a first pairing does, at that address. Pairing again with the same hub keeps the pin, and keeps the token's address beside it when its kind is the pin's.
- **`--yes` isn't offered for a key that replaces a pin.** When the hub key in the token isn't the one the agent holds, it asks and names both fingerprints. For a standard input that isn't a terminal it needs `--replace-pin <fingerprint>` naming the new key, so a script can't replace a pin by accident. The fingerprint is read in any case, with or without hyphens and spaces, and with Crockford's aliases; one that isn't the new key's is refused and the pin stays.
- **`--token-file`** warns when the file's mode lets another user read or change it, and says to delete the file after pairing. One they can change could hold their token, with their hub's address and key, so the warning says so, as a secret file's does (security.md §10.2).
- **It waits with progress and ends clearly.** After "Pair?" it prints its own key's fingerprint ("Compare this with the hub: 5H2R-QW5N-7K3Q-M9XD-2RTV"), then the steps of "Token, Connect, Approval" as lines ("Token accepted", "Connected to the hub", "Waiting for a person to approve this agent on the hub: Settings → Agents"), and says the agent keeps backing up meanwhile ([120, "Pending approval"](120-agent-hub-connection-security.md)). In a terminal the last line updates in place with the time waited; with no terminal it prints a new line every 30 seconds. It waits until the agent lapses, and `--no-wait` returns at once.
- **Each outcome has its own line and exit code.** Approved: "Approved by the hub", exit 0. Rejected or removed: "The hub rejected this machine. It keeps backing up on its own. Ask for a replacement token to pair again" (`agent-removed`), exit 1. Lapsed: "Nobody approved this machine within 7 days; the hub dropped it. Make a new token and run pair again" (`pending-expired`), exit 1. A refusal of the errors below: its row's message, exit 1. `--no-wait`, after the pairing is accepted: "Paired; waiting for approval on the hub", exit 0. Ctrl-C stops waiting, says "Still paired and waiting for approval on the hub; nothing was undone", and exits with 130: the token is already spent and the hub holds the agent pending.
- **From a hub to a headless agent in one line:** `ladderback pairing-token | ssh agent ladderback pair --yes` works after the person has compared the hub key printed on standard error with the hub's, because a pipe carries the token alone and `pair` reads standard input. Without `--yes` it refuses, as above; the token goes through the ssh channel only, never argv.
- **A headless agent can't scan.** It pairs with the token pasted at the prompt, sent from a phone through Universal Clipboard, AirDrop or the share sheet to the machine whose terminal runs SSH. The scan is for a phone or laptop with a camera that pairs the agent through its browser. And a headless hub can still be paired from a phone: the phone scans the code in the hub's terminal and its browser, signed in to the agent, pairs it.

### `ladderback hub-status` (on the agent)

- **It checks the link to the pinned hub once.** It connects to the hub the machine pinned at pairing, proves who it is, and says what the hub answered: "Connected. The hub holds this machine waiting for approval", with the machine's own key fingerprint to compare with the hub's screen, or "Connected. The hub approved this machine". Either exits 0.
- **It tries the address the person gave, then the token's.** It moves on only while no hub has proved the pinned key there; once one has, its answer stands, so one check spends one connection counter. When no address leads to the hub, it says the first address's failure.
- **`--hostname <name>`** connects under another hostname than the machine's, as `pair --hostname` pairs under one.
- **It writes only what a connection needs.** On a machine that never paired it says "This machine isn't paired with a hub" and exits 1, and writes nothing to the data folder, not even an agent ID. On a paired machine it raises the connection counter before its proof, and keeps a higher generation the hub's `Welcome` names in the pin.
- **A hub that doesn't prove the pinned key** (`hub-key-mismatch`, `hub-proof-invalid`) gets a message of its own: the machine sent it nothing, don't pair again, check the address with whoever runs the hub. Pairing's advice to pair again doesn't fit while `pair` refuses a machine that holds a pin (`--replace-pin` isn't built). Every other refusal says its row's message below, and exits 1.

## Errors and their next step

Each is a catalog message in the web UI and the command line, with the code in the log. The text says what happened and what to do; none leaves the person at a dead end.

| Code | What the person is told |
|---|---|
| `token-used` | "This token was already used. If this machine paired a moment ago and the reply was lost, run pair again with the same token: the hub recognizes the same agent and key and answers as before. If it says this again, another machine may have used the token first: look at the agents list on the hub, reject any agent you don't know, then make a new token." |
| `token-expired` | "This token lapsed. Make a new one on the hub." |
| `token-revoked` | "This token was canceled on the hub. Make a new one." |
| `token-malformed` (prefix or checksum) | "That isn't a whole token. Copy all of it again: a wrapped line or a chat app may have cut or changed it." |
| `token-unsupported` | "This token is newer than this agent. Update the agent, then pair." |
| `hostname-taken` | "The hub already has an agent named laptop (last seen 3 days ago). If this is a different machine, pair under another name: add `--hostname`, or set it on this page. If it is the same machine and its data folder was lost, don't rename it: remove the old agent on the hub, then pair with this same token." |
| `agent-key-mismatch` | "The hub has another machine under this agent ID. Don't pair again; ask whoever runs the hub." |
| `token-not-for-this-agent` | "This is a replacement token for another agent. Make a replacement token for this one." |
| `hub-key-mismatch` | "This hub's key isn't the one this agent trusts. If you reset or reinstalled the hub on purpose, make a new token on the hub and pair again, and compare the hub key's fingerprint on both screens first. If you didn't, don't pair again: someone may be posing as it." |
| `hub-proof-invalid` | Same as `hub-key-mismatch`, with "the hub didn't prove its key" |
| `proof-refused` | "The hub refused this agent. It may have been removed, or its key may have changed. Look at the agents list on the hub; to bring it back, make a replacement token for it. If this machine was never approved, its wait may have lapsed: make a new token and pair again." One message for every cause, since the hub says no more |
| `identity-conflict` | "Another machine is using this agent's identity. This one keeps backing up and runs no maintenance. Make a replacement token for it on the hub." |
| `agent-removed` | "The hub removed this machine, or someone rejected it. It keeps backing up on its own. A new ordinary token is refused; ask whoever runs the hub for a replacement token for this agent." |
| `pending-expired` (signed, so shown as an instruction) | "Nobody approved this machine within 7 days, and the hub dropped it. Make a new token and pair again; the machine keeps its identity." Shown also to a machine that was off when the 7 days ended, a laptop asleep for 8 days among them: the hub keeps a marker for 30 days |
| `pending-limit` | "The hub already has 20 agents waiting for approval. Approve or reject some, then pair again with this token." |
| `pairing-needs-sign-in` | "Set an admin password on this machine before pairing from another computer." |
| `step-up-failed` | With sign-in on: "That password didn't match. Try again; the wait grows with each miss." With sign-in off, on this machine only: "That isn't right, so no token was made. Try again." beside the field, which shows the hub key's fingerprint to type or paste. |
| `hub-unreachable` (the connection failed) | "This machine couldn't reach the hub at https://hub.example:9899 (connection refused, or no answer in 10 seconds). Check the address and that the hub is running. It keeps backing up and tries again on its own. If the address is wrong, give the right one: `ladderback pair <address>`." |
| `hub-pin-mismatch` (before any `Connect`) | "The hub at https://hub.example:9899 showed a certificate that isn't the one its token names. Don't pair. Check the address with whoever runs the hub; if they replaced the hub's certificate key, make a new token and compare the fingerprint." |
| `hub-untrusted` (a CA-checked address, before any `Connect`) | "The hub at https://hub.example:9899 showed a certificate this machine doesn't trust for that name. Don't pair. Check the address with whoever runs the hub, or ask them for a token of the hub's built-in TLS, which pins its certificate." |
| `plain-address` | "This address starts with http://, which shows the token to the network. This machine pairs only over https:// or to localhost. Use the hub's https address (`ladderback pair https://…`), or run the hub with its built-in TLS." |
| `hub-connection-lost` (the connection ended, or the hub sent something out of turn) | "The connection to the hub ended before the pairing was answered. Check that the hub is running, then pair again with the same token: a token is used only once the hub answers." |
| `binding-unavailable` (before the hub's proof, so unsigned) | "Not connected: the hub closed the connection because it can't offer this machine's kind of connection. Reach the hub directly at its https address, not through a proxy that ends TLS, and update the hub if it is old." The log has the code. It is treated as a dropped connection and never tells a person to re-key |
| `clock-skew` | "This machine's clock is more than a day off the hub's. It keeps backing up but runs no scheduled maintenance until the clock is right. Turn on automatic time, then check it again." |
| `rate-limited` | "The agent refused a plan run the hub asked for: at most one in 5 minutes for a plan. Try again later; nothing was lost." |
| `sequence-error` | "The connection to the hub went out of step and was closed; something in the network path may be dropping or repeating messages. The agent reconnects by itself. If it keeps happening, connect to the hub directly and look at the log." |
| `stale-proof` | "The hub saw a late message from this agent and ignored it. Nothing is wrong and nothing was changed; the agent tries again on its own." It shows only in the log, and never as an alert |
| `handshake-hidden` | "The hub's reply hid a newer handshake that the hub says it speaks. A proxy that forwards only some of the hub's routes may do that: forward the whole `/sync/` path, not only `/sync/v1`. This agent didn't connect and sent no proof. If it persists, don't re-pair, and tell whoever runs the hub." |
| `generation-lower` | "The hub offered an older state than this agent has already seen, so the agent ignored it. It keeps backing up. If you restored the hub from a backup, or moved it, ask whoever runs it. If you didn't, the hub's key may be leaked: on the hub's machine run `ladderback hub-recover` with the recovery kit. A leaked key always needs a recovery or a reset of the hub key." |
| `epoch-lower` ([decision 137](137-a-hub-moves-by-handover-never-by-copy.md)) | "The hub offered an older lease on this repository than this machine has already seen, so the machine ignored it. It keeps backing up. If the hub was restored from a backup or moved, whoever runs it confirms the recovery on the hub." |
| `kit-command-only` | "The hub doesn't show the recovery kit in a browser here: sign-in is off, the connection can't be protected, or the 7 days are over. On the hub's machine run `ladderback recovery-kit`." |
| `hub-tls-refused` (the TLS handshake failed on its version or key exchange) | "The hub, or a proxy in front of it, doesn't speak TLS 1.3, or, for a pinned address, the hybrid key exchange X25519MLKEM768 this machine needs. Update the proxy or the hub, or reach the hub at its own address. It keeps backing up and tries again on its own. Don't pair over anything lower." |
| `hub-link-lost` (a paired machine connecting: the connection ended, or the hub sent something unsigned, out of turn or past its life) | "The connection to the hub ended, or the hub sent something this machine can't trust, so nothing it sent was acted on. This machine keeps backing up. Check that the hub is running, then try again." |
| `protocol-out-of-date` (the two share no protocol or handshake; [120, "Versions"](120-agent-hub-connection-security.md)) | "The hub is on an older release than this machine can use, or a newer one. This machine keeps backing up. Update whichever side is older." |

Messages are localized (Fluent). A code's text never includes a token.

## The Recovery Kit

The hub makes a recovery seed at its first start and after each `hub-recover` ([120, "The recovery key"](120-agent-hub-connection-security.md)). It is the one secret a person has to keep somewhere other than the hub, so the hub shows it the way a password manager shows its Emergency Kit: **one printable page, also a file, that holds everything needed to use it.**

- **The kit holds** the seed, as text in groups of four (`K7QM-…`, 13 groups) and as a QR code of the same text; the **recovery key's fingerprint**, which changes only when the hub is recovered, in the rendering of "The fingerprint"; the hub's address and the hub key's fingerprint, labeled "at the time this kit was made", since a routine rotation changes the hub key and keeps the seed; the date it was made, as `2026-11-18`; and three numbered steps.
- **The three steps.** 1, on the machine that runs the hub, run `ladderback hub-recover`. 2, type or paste the seed at its prompt, which does not echo it; case, hyphens, spaces and Crockford's aliases are read ("O" for "0", "I" and "L" for "1"). The command shows the recovery key's fingerprint it derived: check that it is the one on this kit. 3, the hub makes a new key and a new kit, and its agents follow it on their next connection, so print the new kit and destroy this one.
- **The kit is a route in the web app,** built from the shared components, not a page the daemon renders: the Step-up field opens it, the seed is a Text field set read-only, and errors are Alerts ("Shared components"). The route's address holds no seed and its title is "Ladderback recovery kit". The seed arrives in the response to the authenticated request that follows the step-up, with `Cache-Control: no-store`, so no cache or history keeps it and a reload or print never resubmits a form: after a reload the page asks for the step-up again.
- **One step-up opens the kit page, and its Download needs no second one.** The step-up that opens the kit page covers its Download, for at most 10 minutes and only while the person stays in the kit's pages ([120, "The recovery key"](120-agent-hub-connection-security.md)). The confirmation is another action and asks for a step-up of its own, and the page says so beside its link.
- **It is viewable in the browser for 7 days only, and only with sign-in on.** The kit page and its Download work for 7 days after the hub is set up and for 7 days after each `hub-recover`. After that, with sign-in off or in proxy-header mode, or with built-in TLS on a connection that didn't use the hybrid key exchange, the page refuses with `kit-command-only` and says the kit can only be shown by `ladderback recovery-kit` on the hub's machine. Each view, in the page, a download or the command, raises `hub.recovery_kit_viewed` (§11) naming the way, the user and the peer address, and the page says it does; Settings shows the last view.
- **The kit page prints as the kit and nothing else.** Its print rule hides the app's chrome, the Banner, Download, the confirmation link and every Alert but the leak warning, which is part of the sheet; the token's dialog hides its secret. "Shared components" names the two behaviors in one contract, so they don't contradict.
- **The file is the same kit.** "Download" gives `ladderback-recovery-kit-2026-11-18.html`, one file with no script, no external reference and the QR as inline SVG, so it prints the same later. It is sent with `Content-Disposition: attachment` and `no-store`.
- **`ladderback recovery-kit`** on the hub's machine, run by the user that owns the data folder, shows the kit as text and a code drawn as for a token ("The terminal's QR code"). **It refuses when standard output isn't a terminal,** unless `--out <path>` is given, since a redirect would write the root key with the shell's file mode and may overwrite a file; it warns that the terminal's scrollback, a `tmux` or `screen` log and a recording keep what it prints, and that the person can clear them. `--out` writes the file with mode 0600 and refuses to overwrite, and says to move the file off this machine and delete it. `ladderback recovery-kit --confirm` confirms the kit with a random group, as the web page does, and works at any time.
- **The seed file waits for the person.** Until they confirm they have stored the kit, the seed stays in `hub-recovery-seed` in the data folder, which the default backup exclusion leaves out of a plan ([120, "The recovery key"](120-agent-hub-connection-security.md)). Settings and a Banner on the hub's pages say the kit is waiting, and after the 7 days that `ladderback recovery-kit` shows it.
- **They confirm on a page that doesn't show the seed,** by typing back one group picked at random when the page loads, with its number ("Group 7 of 13"), never always the last, after a step-up of their own. It is offered at any time, also after the 7 days. The field is a Text field apart from any step-up, with a visible label and a description: "The four characters of group 7, from where you saved the kit. Paste works. Capital or small letters are fine, and O, I and L are read as 0, 1 and 1." The page doesn't claim more than it does: it says it catches a kit that is missing, cut short or mistyped, not that the kit is kept offline. A wrong group is an Alert and changes nothing; the right one deletes the seed file and the page says so.
- **After the confirmation the hub has nothing to show.** The page says the hub no longer holds the seed and can't show it. A person who has lost the kit has a hub with a recovery seed nobody knows; [120, "The recovery key"](120-agent-hub-connection-security.md) says what that costs, and Settings tells them so they can decide.
- **The QR is the seed's text and nothing more.** For 64 characters in alphanumeric mode it is version 3, 29 modules a side by the capacity tables (not rendered). It lets a person who keeps the kit on paper read the seed with a phone's camera and type or paste it at the prompt; no part of Ladderback scans it. It uses the crates of "Crates" and draws in the same module as the token's.
- **Where a kit can leak, and where to keep it.** A printer's spool, a download folder, a synced downloads directory, a screenshot, a photo of the screen, and the shoulder of anyone behind the person. The page says so in one line above the kit ("Print it, or store the file offline on paper or an encrypted drive; don't keep it in a folder that syncs to the cloud, or on this machine"), and the file's name carries no secret. A person who syncs it anyway has put the root key where the cloud's holder can read it; the choice is theirs, and `hub-recover` with a new kit ends the exposure.
- **Accessibility.** The seed's groups are marked up as a fingerprint is ("The fingerprint"): a span for each character with an `aria-label`, the hyphen `aria-hidden`, selectable text, wrapping between groups at 320 CSS px with 200 % text, and a test of the accessibility tree. The page has headings and numbered steps in order, the confirmation field has a visible label and description, and nothing is conveyed by color. The QR has a text alternative that doesn't hold the seed.

## Where a token can leak

A token is a one-time, short-lived credential: whoever holds it unused can pair one agent, and the agent is held pending until a person approves it ([120, "Pending approval"](120-agent-hub-connection-security.md)). These are the ways it leaves the hub's page.

- **The text and the QR are the same secret.** A screenshot, or a photo of the screen, holds the same token the text does. A QR on a phone in public can be read by anyone who can see the screen; the dialog hides it once used, lapsed or canceled. The default life stays 15 minutes, and a person about to show it in public can pick 5.
- **Copy** puts the token on the clipboard, where other apps, a clipboard history or a synchronized clipboard may read it. Ladderback can't clear another app's copy.
- **A share leaves Ladderback.** It lands in whatever app the person picks, which keeps it, may sync it and may index it, such as a notes app or a photo library that syncs to a cloud. The dialog says to share it only to the person's own devices. A share doesn't lengthen the token's life.
- **The terminal keeps what it shows.** The command prints the token and its code into the scrollback, a `tmux` or `screen` log, a session recording and the screen of an SSH session. The command says the token is one-time and short-lived, and the person can clear the screen or choose 5 minutes. A recording is outside Ladderback's control.
- **A leaked token can be revoked** from the dialog, Settings or `--revoke`, and a rogue that uses one first is held pending for a person to reject.
- **Shell history and process lists** hold nothing: neither `pair` nor `pairing-token` takes the token as an argument. `--ttl`, `--address` and `--hostname` are not secrets.
- **A scan that goes wrong holds nothing:** frames aren't stored, and a code that isn't a token is dropped.

## Shared components

Each screen is built from the components of §4.5 and draws no control of its own. The table names the component for each part; a part that no component covers is a gap to fill in the shared set, not a one-off.

| Screen | Built from |
|---|---|
| The hub's pairing dialog | **Dialog** (`starts_on` Copy, as the API-token dialog does); read-only **Text field** for the token; **Text field** for the address with an **Alert** in the Warning tone for a loopback or plain `http://` address; inline status lines (`role="status"`) for Copy, used and lapsed |
| The agent's row inside that dialog | A group with **Button**s; **Step-up field** opens in the row, with no second modal; an **Alert** (`role="alert"`) for `step-up-failed` |
| Settings → Agents, "Waiting for approval" | **Table**, with each row's buttons named for its agent ("Approve laptop"); **Review dialog** for Approve (with the **Step-up field**) and for Reject, which start on Cancel; a **Banner** for the count |
| Unused tokens | **Table**, with unique row-button names ("Revoke token 3F9A8B21"); **Review dialog** for Revoke |
| The agent's pairing page | **Steps** (#1042: "Token, Connect, Approval"); a paste **Text field**; the scanner; **Alert** for each outcome ("What the agent shows") |
| The Recovery Kit | **Step-up field** to open it; read-only **Text field** for the seed; a **Text field** for the confirmation; **Alert**; the print contract |

- **Steps (#1042)** speaks a change of step through the announcer, which is inert inside a modal. Wherever Steps stands in a dialog, the dialog's own status line says each change as well. The pairing page uses #1042's component and draws no steps of its own, so #567's page waits for it in M3b.
- **No Toast inside a modal.** A Toast speaks through the announcer, which a modal makes inert. Copy, used and lapsed are inline status lines inside the dialog, as in the API-token dialog.
- **The step-up never opens a nested modal.** In the dialog's row it is the Step-up field in place; in Settings → Agents it is the Review dialog's own field.
- **Print is one contract with two behaviors,** named for each surface in §4.5: the token's dialog **hides** its secret, so a printout holds none of it; the Recovery Kit is a **sheet**, so a printout holds the kit and nothing else.

## Accessibility

- **Every task has a keyboard and screen-reader path,** and the paste path needs no camera, no pointer and no color (§0 A1, A2, A5; §4.5).
- **State changes are announced in the dialog,** not in a live region outside it that a modal makes inert: the status lines above. Outside a dialog, a new waiting agent is said through the announcer as a polite `UiEvent`, since a Banner isn't a live region ("Waiting for approval").
- **Focus** is placed on arrival (Copy), kept when content vanishes (to the agent's row or to "Make a new token"), moved to the confirm panel after a scan, and to the next row or the section's heading after an approval or rejection.
- **Color is never the only signal.** The terminal QR's colors are data; every message around it is text, and `NO_COLOR` and `--plain` mean text only. A lookalike flag and a Banner's tone come with words.
- **Fingerprints and seed groups** are spoken a character at a time and break between groups ("The fingerprint"); a hostname with a character outside ASCII has a note in text, not a mark in color.
- **The shared components** (the QR and the scanner) are built to the checklist of §13.7.5 before the page uses them; the other components are the ones "Shared components" names.

## Crates

The maintainer approved these on 2026-11-18. The implementation PR (#567) adds them with `just deny` run on the result. They draw and read the QR code and are used for nothing the connection's security rests on.

| Crate | Version | Used for | Alternative, and why not |
|---|---|---|---|
| `qrcode` | 0.14.1, `default-features = false` (no `image` or `svg`: the code draws the SVG of the dialog and the half blocks of the command from the same module matrix). MIT or Apache-2.0; pure Rust; the last release is July 2024 | Making the QR code of a token, in the web UI's WebAssembly and in the command | `qrcodegen` (Nayuki's reference implementation, smaller and stricter); `fast_qr` 0.14.0 (faster, less used). `qrcode` is chosen for its record; swapping is a small change |
| `rqrr` | 0.11.0, `default-features = false` (no `img`). `(MIT OR Apache-2.0) AND ISC`, and `ISC` is in `deny.toml`'s allow list; depends on `g2p` and `lru` | Reading a QR code from a canvas frame, in WebAssembly, so one decoder behaves the same in every browser and no frame leaves it | The browser's `BarcodeDetector`, which isn't in every browser, so a fallback is needed anyway and two paths are two things to test. `rxing` 0.9.4 (a ZXing port, larger). `bardecoder` (last release 2023). A JavaScript decoder adds an npm package and code outside Rust |
| `web-sys` features | The existing `web-sys` (0.3.107 in the tree) gains `Navigator`, `MediaDevices`, `MediaStream`, `MediaStreamTrack`, `MediaStreamConstraints`, `HtmlVideoElement`, `HtmlCanvasElement`, `CanvasRenderingContext2d` and `ImageData`; `Clipboard`, the share call and the Wake Lock request go through `js-sys`'s `Reflect`, as web-sys has no stable binding | The camera, the canvas the decoder reads, Copy, Share and the Wake Lock | Writing the calls in JavaScript: a file outside Rust, outside the tests and the CSP's review |

**Verified:** `cargo check --target wasm32-unknown-unknown` passes for `qrcode` 0.14.1 and `rqrr` 0.11.0 with default features off (run 2026-11-18, in a scratch crate that encodes a code and prepares a greyscale image for decoding). **Not verified:** how well `rqrr` reads webcam frames, and the browsers' behavior on a phone ("From a phone"); the red `test:` commit of the scan component runs both. If `rqrr` reads poorly, `rxing` is next.

The security crates are in [120, "Crates"](120-agent-hub-connection-security.md).

If any part of QR pairing turns out to be a security hole that can't be closed cleanly, the QR is deferred until after 1.0 and the text token stays; no review of this decision found one.

## Tests that pin it

Each is a test that fails before the behavior exists (§13.1).

- **The fingerprint.** One function renders every fingerprint shown, at 100 bits, with fixed vectors; the dialog, the command, Settings and its step-up, the agent's confirm step and the pending row show the same string for one key, and `GET /api/v1/hub` answers no other ID for it; no agent ID or token ID is labeled a fingerprint or offered to verify with. A test reads the accessibility tree of a fingerprint and of the kit's seed and expects one name for each character, with the hyphens hidden, and the text wraps between groups at 320 CSS px with 200 % text.
- **The QR.** It decodes to exactly the token's text. The SVG is black on white with a 4-module quiet zone, `forced-color-adjust: none` and `crispEdges`, at level L or an M of the same version, with a module of at least 4 CSS px and a `max-inline-size` that fits 320 px. The QR goes with the token when it is used, lapsed or canceled, and its text alternative doesn't hold the token.
- **The camera.** It is asked for only on pressing "Scan the code", is released on a read, a cancel, "Stop scanning", an error and a page leave, and no frame is stored. The status line says "waiting for permission" while the browser asks. The preview is `aria-hidden`. A Wake Lock is asked for again when the page becomes visible. A scanned text that isn't a token is dropped with a status line. Scanning is offered only where the camera API and a secure context are.
- **The dialog.** A used token replaces the content with a status line naming who used it, with the full fingerprint, and focus goes to the agent's row; a lapse puts focus on "Make a new token"; Cancel and Close revoke an unused token; a list shows unused tokens and revokes them by a Review dialog whose buttons carry the token ID; the expiry shows a UTC offset and its remaining time comes from the hub's response, so a wrong browser clock changes nothing; a loopback or plain `http://` address is warned about; the address takes its kind with it, by default that of the listener the page came in on; a print rule hides the token; nothing is spoken through a Toast.
- **Waiting for approval.** Approve in the dialog's row opens the Step-up field in place with no second modal; Approve in Settings opens a Review dialog that starts on Cancel; a wrong password is `step-up-failed` and approves nothing; Reject needs no step-up. The Banner's edition carries the count, new keys included, so a dismissal hides only that count; the announcer says a new arrival politely, and the dialog's status line says it inside the dialog; `agent.pending` is raised once for each agent or new key; a row shows its age and time left from the hub's response; after 7 days an agent is dropped and told `pending-expired`, also when it connects later, and a new key is discarded; approving a new key closes the old key's connections. The agent's page shows each state's Alert and Steps.
- **The terminal's output.** `pairing-token` prints the hub fingerprint, the token, the one-time line with the token ID and a code whose half blocks decode, in a test, to exactly the token, with a 4-module quiet zone and explicit colors. It prints the token alone to a pipe, and the hub key, expiry and token ID to standard error. `NO_COLOR`, `--plain` and `--no-qr` print text and a line that says why, and `--qr` prints the code anyway. A loopback address is warned about.
- **The terminal's size and wait.** A window too small waits, measures again on Enter, shows the time left and never mints a second token. The size shown matches the computed one for tokens from 191 to 420 characters. In a terminal it waits for the pairing and prompts with the full fingerprint, `n` leaves the agent pending, and Ctrl-C revokes an unused token. The token and the code are in no log line, and the command refuses to run as another user than the one that owns the data folder.
- **`pair`.** A token on argv is refused with how to give it. A token wrapped across lines, quoted or with a trailing newline is read; one with letters changed fails the checksum with the message. It echoes what it read and asks before it sends anything. Piped with no terminal, a first pairing refuses without `--yes`. A changed hub key needs `--replace-pin` with a standard input that isn't a terminal, and the fingerprint is read in any case and with or without hyphens. A `--token-file` that others can read or change warns. `--hostname` pairs under that name. It waits with progress and ends with its own line and exit code for approval, rejection, removal and expiry, and Ctrl-C leaves the pairing as it is.
- **Errors.** Every code in the table, and the four failures with no code (unreachable, TLS pin mismatch, plain `http://`, and a TLS handshake that failed on its version or key exchange), has a message with a next step in the catalog, and none holds a token. A refusal that isn't signed is shown only as "not connected", never as an instruction to re-key. `handshake-hidden` names the proxy's routes as the likely cause, and the TLS row doesn't send a person to look for the hub that isn't running.
- **The Recovery Kit's page.** It is a route of the web app built from the shared components, and holds the seed in 13 groups and a QR that decodes to exactly the seed's text, the recovery key's fingerprint, the hub's address and key fingerprint labeled as of the kit's date, the date and the three steps. It is sent with `no-store`; its title and its address hold no part of the seed; a print rule leaves only the kit. The download is one file with no script and no external reference, and the same seed text.
- **The Recovery Kit's access.** One step-up opens the kit and its download; the confirmation takes a step-up of its own. The kit is refused with `kit-command-only` with sign-in off, after 7 days and on a built-in TLS connection without the hybrid exchange, and each view raises `hub.recovery_kit_viewed` naming the way, the user and the peer address, and Settings shows the last. The confirmation is on a page that holds none of the seed, asks for a group picked at random and shows its number, takes a paste, any case and Crockford's aliases, is a field apart from the step-up, and works after the 7 days. A wrong group deletes nothing and the right one deletes the seed file; after that the page shows nothing of the seed.
- **`recovery-kit`.** It refuses when standard output isn't a terminal unless `--out` is given, warns about scrollback, prints the text and a code that decodes to the seed, refuses a user other than the data folder's owner and refuses to overwrite a file.
- **`hub-recover`.** The seed is read in any case, with or without hyphens and spaces, and with Crockford's aliases; the derived recovery key's fingerprint is shown before anything changes.
- **Where they run.** The encoding and the checks in `domain` and `app` with fakes; the QR and the scanner `@ui` in the three engines, with a fake camera stream; the terminal flows `@e2e` against a pseudo-terminal.

## Appendix: the scenarios `fleet/pairing-experience.feature` takes

Both Rust runners fail a scenario with an undefined step (`fail_on_skipped`, §13.1), so the file can't be committed ahead of its steps. #567's red `test:` commit adds it after the protocol scenarios of [decision 120's appendix](120-agent-hub-connection-security.md).

```gherkin
Feature: Pairing a machine with its hub

  Scenario: A laptop pairs by scanning the code on a phone
    Given a phone signed in to the hub
    And the person confirms with their password
    When the phone shows a new pairing token as text, as a code and with a share button
    And the laptop's pairing page scans the code with its camera
    Then the page shows the hub's address and key fingerprint
    When the person confirms the fingerprint and their password
    Then the hub lists the laptop as waiting for approval

  Scenario: An agent is paired from a phone by scanning the hub's code
    Given a hub showing a pairing code
    And a phone signed in to an agent
    When the phone scans the code with its camera
    And the person confirms the hub's key fingerprint and their password
    Then the agent is paired with the hub and waits for approval

  Scenario: A token is shared from the phone and pasted on the agent
    Given a phone signed in to the hub that has a share sheet
    When the person makes a token and shares it
    And the text is pasted on the agent's pairing page
    And the person confirms the hub's key fingerprint and their password
    Then the agent is paired with the hub and waits for approval

  Scenario: The hub and the agent show the same fingerprint
    Given a hub showing a pairing token
    When the agent's pairing page reads the token
    Then the page shows the hub key's fingerprint in groups of four
    And it is the fingerprint the hub's dialog and Settings show

  Scenario: The person compares the agent's key before approving
    Given an agent that has paired and waits for approval
    Then the agent shows its key fingerprint in groups of four
    And the hub's pending row shows the same fingerprint, in full, with the agent's hostname and agent ID

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

  Scenario: A scan that finds no code says so
    Given an agent whose pairing page is scanning
    When thirty seconds pass without a code
    Then the page says no code was found and what to try
    And it keeps looking

  Scenario: A code that isn't a pairing token is ignored
    Given an agent whose pairing page is scanning
    When the camera sees a code that isn't a pairing token
    Then the page says it isn't a pairing token and uses nothing from it

  Scenario: The dialog says who used the token
    Given a hub showing a pairing code
    When an agent pairs with the token
    Then the dialog no longer shows the code, the text or the copy and share buttons
    And the dialog says which agent used it and when
    And it shows the agent's full key fingerprint, 5H2R-QW5N-7K3Q-M9XD-2RTV, and asks the person to compare it with the agent's own screen
    And the agent's row has focus

  Scenario: The dialog says when the token lapsed
    Given a hub showing a pairing code
    When the token's life runs out
    Then the dialog no longer shows the code, the text or the copy and share buttons
    And the dialog says when it lapsed and offers a new token

  Scenario: Closing the dialog cancels an unused token
    Given a hub showing a pairing code
    When the person closes the dialog
    Then an agent that pairs with the token is refused as revoked

  Scenario: An unused token is revoked from the list
    Given a token made on the hub's command line
    When the person revokes it from the list of unused tokens
    Then an agent that pairs with it is refused as revoked

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
    Then it prints the hub key's fingerprint and a pairing token as text
    And it prints a code of the same text that a camera can read
    And it says that the token is one-time and when it lapses, with the time zone
    And the hub's log holds neither the token nor the code

  Scenario: The hub's command waits for the pairing and asks for approval
    Given a hub whose command is run in a terminal
    When the person runs the hub's pairing-token command
    And an agent pairs with the token
    Then the command names the agent's hostname and its full key fingerprint, 5H2R-QW5N-7K3Q-M9XD-2RTV, and asks to approve it
    When the person answers y
    Then the hub grants the agent what the rules allow

  Scenario: The hub's command prints text only when its output is piped
    Given a hub whose command's output is piped to another program
    When the person runs the hub's pairing-token command
    Then the other program receives the token as text and nothing else

  Scenario: The hub's command prints no code when color is off
    Given a hub whose command is run in a terminal with NO_COLOR set
    When the person runs the hub's pairing-token command
    Then it prints a pairing token as text and no code
    And it says why there is no code

  Scenario: A terminal too small for the code waits instead of giving up
    Given a hub whose command is run in a terminal too small for the code
    When the person runs the hub's pairing-token command
    Then it prints the token as text and says what size the code needs
    When the person enlarges the window and presses Enter
    Then it draws the code
    And no second token was made

  Scenario: An agent is paired from a phone by scanning the code in the hub's terminal
    Given a hub on a headless machine reached over SSH
    And a phone signed in to an agent
    When the person runs the hub's pairing-token command in the terminal
    And the phone scans the code in the terminal with its camera
    And the person confirms the hub's key fingerprint and their password
    Then the agent is paired with the hub and waits for approval

  Scenario: A headless agent is paired with a token pasted from a phone
    Given a headless machine reached over SSH
    And a hub showing a pairing token on a phone
    When the person shares the token from the phone to the computer with the terminal
    And the person pastes it at the agent's pairing prompt
    Then the agent echoes what it read and asks to pair
    When the person answers y
    Then the agent is paired with the hub and waits for approval

  Scenario: A token pasted with line breaks is read whole
    Given a headless machine reached over SSH
    When the person pastes a pairing token that a terminal wrapped onto three lines
    Then the agent reads the whole token

  Scenario: The agent's command refuses a token given as an argument
    Given a headless machine reached over SSH
    When the person gives a pairing token as an argument of the agent's pairing command
    Then the command refuses it and says how to give it

  Scenario: A script can't replace a pinned hub by accident
    Given an agent paired with a hub and a token from another hub
    When a script gives the token on standard input without naming the new key
    Then the agent refuses and says to name the new key's fingerprint

  Scenario: A lost reply doesn't make the real agent a stranger
    Given an agent that paired and never received the hub's reply
    When the agent pairs again with the same token, agent ID and key
    Then the hub answers as before and the agent is waiting for approval

  Scenario: A used token tells the real agent what to do
    Given a stolen token that another machine has used
    When the real agent pairs with it
    Then the agent says the token was used and to look at the hub's agents list

  Scenario: Piped input with no terminal refuses a first pairing without --yes
    Given a headless machine whose standard input is the token and has no terminal
    When the person runs the agent's pairing command without --yes
    Then the command refuses and says there is no terminal to ask on
    When the person runs it again with --yes
    Then the agent is paired with the hub and waits for approval

  Scenario: A pinned key is replaced with its fingerprint in any case
    Given an agent paired with a hub and a token from another hub whose key is 7K3Q-M9XD-2RTV-8HFA-QW5N
    When a script gives the token on standard input and --replace-pin 7k3qm9xd2rtv8hfaqw5n
    Then the agent replaces its pin and pairs with the new hub
    When a script gives --replace-pin with a fingerprint that isn't the new key's
    Then the agent refuses and its pin stays

  Scenario: A taken hostname is paired under another name
    Given a hub that already has an agent named laptop
    When another machine named laptop pairs with a token
    Then the agent says the hub already has an agent named laptop and how to pair under another name
    When the person pairs again with --hostname laptop-2
    Then the agent is paired with the hub as laptop-2 and waits for approval

  Scenario: A token file other users can read or change is warned about
    Given a token file that other users on the machine can read or change
    When the person pairs with --token-file and that file
    Then the agent warns that other users can read or change the file
    And the agent says to delete the file after pairing

  Scenario: The pairing command waits and ends with a line for each outcome
    Given an agent that has paired and waits for approval
    Then the command shows the steps and the time waited
    When the person approves it on the hub
    Then the command says it was approved and exits with success
    When the person rejects it on the hub instead
    Then the command says the hub rejected it and exits with failure
    When nobody approves it for 7 days
    Then the command says the hub dropped it and to pair again, and exits with failure

  Scenario: Ctrl-C leaves a waiting agent paired
    Given an agent that has paired and waits for approval
    When the person presses Ctrl-C
    Then the command says the agent is still paired and waiting
    And the hub still lists the agent as waiting for approval

  Scenario: A piped token command prints the token alone and the rest to standard error
    Given a hub whose command's output is piped to another program
    When the person runs the hub's pairing-token command
    Then the other program receives the token and nothing else
    And standard error shows the hub's key fingerprint, when the token lapses and the token ID

  Scenario: Ctrl-C at the hub's command revokes an unused token
    Given a hub whose command is run in a terminal and waits for the pairing
    When the person presses Ctrl-C
    Then the command says it revoked the token
    And an agent that pairs with the token is refused as revoked

  Scenario: The code is forced where NO_COLOR is set
    Given a hub whose command is run in a terminal with NO_COLOR set
    When the person runs the hub's pairing-token command with --qr
    Then it prints a pairing token as text and a code a camera can read

  Scenario: A loopback address is warned about in the dialog and the command
    Given a hub the person reaches at localhost
    When the person makes a pairing token
    Then the dialog warns that other machines can't reach that address and asks for the one agents will use
    When the person runs the hub's pairing-token command
    Then it prints the same warning

  Scenario: The token's remaining time doesn't depend on the browser's clock
    Given a hub showing a pairing token and a browser whose clock is a day ahead
    Then the dialog shows the token as live, with its lapse time from the hub

  Scenario: A name outside ASCII is noted once for the whole name
    Given an agent paired under the hostname "Nick’s MacBook Pro" with a curly apostrophe
    Then the pending row says the name holds a character outside ASCII
    And the note lists each such character by code point and doesn't call it dangerous

  Scenario: A lookalike name is flagged in words
    Given an agent paired under the hostname "Laptop"
    When another agent pairs as "lаptop" with a Cyrillic letter
    Then the pending row says "Looks like Laptop, held by another agent" and when it was last seen
    And the approval compares keys and not names

  Scenario: A fingerprint is spoken a character at a time
    Given a hub showing its key fingerprint 7K3Q-M9XD-2RTV-8HFA-QW5N
    Then a screen reader finds one name for each character and none for the hyphens
    And the fingerprint breaks between groups at 320 pixels with text enlarged to 200 percent

  Scenario: Stopping a scan on a phone needs no Escape key
    Given an agent whose pairing page is scanning
    Then a "Stop scanning" button is visible
    When the person presses it
    Then the camera is released

  Scenario: A scan waits for the camera's permission and says so
    Given an agent whose pairing page is offered a camera
    When the person chooses to scan and the browser's prompt is open
    Then the page says it is waiting for permission and doesn't say no code was found

  Scenario: The camera preview is not announced
    Given an agent whose pairing page is scanning
    Then the camera preview is hidden from a screen reader

  Scenario: A phone that sleeps mid-scan keeps the screen on afterward
    Given a hub showing a pairing code on a phone
    When the page is hidden and shown again
    Then the dialog asks for the screen to stay awake again

  Scenario: The code stays readable on a small screen
    Given a hub showing a pairing code at 320 pixels with text enlarged to 200 percent
    Then the code fits without sideways scrolling and its modules are at least 4 pixels

  Scenario: The token's address carries its certificate kind
    Given a hub with built-in TLS that the person also reaches through a proxy
    When the person edits the address in the dialog to the proxy's and chooses a certificate authority's certificate
    Then the token is for a CA address
    And an agent that pairs through the proxy is never told not to pair
    When the person runs the hub's pairing-token command with that address and --certificate-authority
    Then it makes the same kind of token
```

### Waiting for approval

```gherkin
  Scenario: A new agent is approved inside the pairing dialog
    Given a hub showing a pairing code
    When an agent pairs with the token
    And the person chooses to approve it in the dialog
    Then the password field appears in the agent's row and no second dialog opens
    When the person enters their password
    Then the dialog says the agent was approved

  Scenario: A wrong password in the dialog approves nothing
    Given an agent waiting for approval in the pairing dialog
    When the person chooses to approve it and enters a wrong password
    Then the dialog says the password didn't match
    And the agent is still waiting

  Scenario: A new agent is approved from Settings
    Given an agent waiting for approval
    When the person opens Settings, Agents
    Then a section lists it with its hostname, agent ID, full key fingerprint, how long it has waited and how long is left
    When the person chooses "Approve laptop" and confirms with their password
    Then the agent is approved
    And focus goes to the next waiting agent or the section's heading

  Scenario: A new agent is rejected
    Given an agent waiting for approval
    When the person chooses "Reject laptop" and confirms
    Then the agent is removed and its identity is kept as a tombstone
    And an ordinary token for it is refused as removed

  Scenario: A banner counts the machines that wait
    Given two agents waiting for approval
    Then every hub page shows "2 machines are waiting for approval"
    When the person dismisses the banner
    Then the banner stays away
    When a third agent pairs
    Then the banner returns and says 3

  Scenario: A new arrival is announced politely
    Given a person on another page of the hub with a screen reader
    When an agent pairs and waits for approval
    Then the screen reader says that laptop is waiting for approval without moving focus

  Scenario: A new arrival is said in the dialog's own status line
    Given a hub showing a pairing code
    When an agent pairs with the token
    Then the dialog's status line says it, since the announcer is inert in a dialog

  Scenario: A waiting agent raises an event
    Given a hub with a notification rule for agents that wait
    When an agent pairs
    Then the hub raises one agent.pending event naming the hostname, agent ID and key fingerprint and no token

  Scenario: A new key waits beside an approved agent
    Given an approved agent and a replacement token for it that another machine has used
    Then the agent's row shows a new key waiting with both fingerprints
    When the person rejects the new key
    Then the agent is as it was

  Scenario: A waiting agent lapses after seven days
    Given an agent waiting for approval for six days
    Then its row says about one day is left
    When a seventh day passes
    Then the hub drops it and frees its hostname
    And the agent says nobody approved it and to pair again

  Scenario: The agent's page says what it waits for
    Given an agent that has paired and waits for approval
    Then its page shows "Waiting for approval", the third step of the pairing, its own key fingerprint and the hub's
    And it says a person must approve it under Settings, Agents on the hub and that backups continue
    When the hub approves it
    Then the page says it was approved and the announcer says so once
    When the hub removes it
    Then the page says the hub removed it and to ask for a replacement token

  Scenario: A new key waiting is counted and lapses
    Given an approved agent and a replacement token for it that another machine has used
    Then the Banner counts the new key among the machines that wait
    When seven days pass
    Then the new key is discarded and the agent is as it was

  Scenario: A laptop asleep for eight days is told to pair again
    Given an agent that waited for approval and was asleep for eight days
    When it wakes and connects
    Then it says nobody approved it within seven days and to pair again with a new token
    And it keeps backing up
```

### Errors and their next step

```gherkin
  Scenario: A hub that can't be reached says what to check
    Given an agent whose token names a hub that isn't running
    When the person pairs
    Then the agent says it couldn't reach the hub at that address and to check the address and the hub
    And it keeps backing up

  Scenario: A certificate that isn't the token's is refused with a next step
    Given a token for a hub whose certificate key the server doesn't present
    When the agent pairs
    Then the agent sends nothing, says the certificate isn't the one the token names and not to pair

  Scenario: A plain http address is refused with a next step
    Given a token whose address starts with http:// and isn't loopback
    When the agent pairs
    Then the agent refuses and says to use the hub's https address

  Scenario: A hub that closes before proving itself isn't a reason to re-key
    Given a hub that closes the connection with binding-unavailable
    Then the agent shows that it isn't connected and the log has the code
    And it never tells the person to make a replacement token

  Scenario: A clock a day off stops maintenance and says how to fix it
    Given an agent whose clock is two days off the hub's
    Then the agent says its clock is off and keeps backing up
    And it runs no scheduled maintenance until the clock is right

  Scenario: A plan run asked for too often is refused in words
    Given a hub that asks an agent to run a plan twice in a minute
    Then the second run is refused as asked too often and nothing was lost

  Scenario: A message out of order is a dropped connection
    Given a hub whose connection repeats a message
    Then the agent closes it, reconnects by itself and says to look at the log if it repeats

  Scenario: A late proof shows only in the log
    Given an agent whose proof reaches the hub after a newer one
    Then the hub changes nothing and the person sees no alert

  Scenario: A hidden newer handshake is refused in words
    Given a hub whose signed list names a newer handshake that an answer of 404 hides
    When the agent connects
    Then the agent sends no proof and says the hub's reply hid a newer handshake

  Scenario: A removed agent is told what to ask for
    Given an agent the hub removed
    When it pairs with an ordinary token
    Then it says the hub removed it and to ask for a replacement token

  Scenario: A lower generation is refused with a next step
    Given an agent that holds generation 2 from its hub
    When a hub answers with generation 1
    Then the agent ignores it, keeps backing up and says the hub may be a restored copy or its key may be leaked
    And it says to run hub-recover on the hub with the recovery kit

  Scenario: A hub that was reset on purpose can be paired again
    Given an agent paired with a hub whose key was reset
    When the agent connects
    Then it says the hub's key isn't the one it trusts
    And it says to make a new token and to compare the hub key's fingerprint on both screens

  Scenario: A step-up without a password asks for the hub key's fingerprint
    Given a hub on this machine with sign-in off
    When the person types a fingerprint that doesn't match, or the hub key's former key ID
    Then the hub says it isn't right, and no token is made

  Scenario: A TLS version the agent can't use says to update the proxy
    Given a hub behind a proxy that speaks only TLS 1.2
    When the agent connects
    Then the agent says the proxy doesn't speak TLS 1.3 and to update it
    And it doesn't tell the person to check that the hub is running

  Scenario: A pinned hub whose listener lacks the hybrid exchange is named
    Given an agent paired with a hub at a pinned address whose listener speaks only X25519
    When the agent connects
    Then the agent says the hub doesn't offer the hybrid key exchange and sends nothing

  Scenario: The kit isn't shown in a browser with sign-in off
    Given a hub with sign-in off and an unconfirmed recovery seed
    When the person opens the recovery kit's page
    Then the page says the kit can be shown only by the recovery-kit command on the hub's machine
    And the seed isn't sent
```

### The Recovery Kit

```gherkin
  Scenario: The hub shows the recovery seed as a kit
    Given a hub that has just made its recovery seed
    When the person opens the recovery kit after confirming with their password
    Then the kit shows the seed in groups of four and as a code
    And the kit shows the recovery key's full fingerprint
    And it shows the hub's address and its full key fingerprint labeled as of the time the kit was made
    And the kit shows today's date and lists three numbered steps for recovering the hub
    And the page isn't cached and its title and address don't hold the seed

  Scenario: The kit is a page of the web app made of its shared parts
    Given the recovery kit is open
    Then its password step, its seed field and its messages are the same components the rest of the web app uses

  Scenario: The kit's seed is spoken a character at a time
    Given the recovery kit is open
    Then a screen reader finds one name for each character of the seed and none for the hyphens

  Scenario: Printing the kit prints nothing but the kit
    Given the recovery kit is open
    When the person prints the page
    Then the printout holds the kit and no menu, button or notice
    And printing a pairing token's dialog holds no token and no code

  Scenario: The kit can be downloaded as a file
    Given the recovery kit is open
    When the person downloads it
    Then the file holds the same seed, fingerprints, address, date and steps
    And the file asks for no script and no other file

  Scenario: One password opens the kit, its download and its confirmation
    Given the person confirmed with their password to open the kit
    When the person downloads the kit and then goes to the confirmation
    Then neither asks for the password again
    When ten minutes pass or the person leaves the kit's pages
    Then the next visit asks for the password

  Scenario: Every view of the kit raises an event
    Given a hub with an unconfirmed recovery seed
    When the person opens the kit's page, downloads it and runs the hub's recovery-kit command
    Then the hub raises hub.recovery_kit_viewed three times naming each way and holding no seed

  Scenario: The kit can't be viewed in the browser after seven days
    Given a hub whose recovery seed has been unconfirmed for seven days
    When the person opens the kit's page
    Then the page says the kit can be shown only by the recovery-kit command on the hub's machine
    And the seed isn't sent
    When the hub is recovered
    Then the kit can be viewed in the browser for seven days again

  Scenario: The seed file stays until the kit is confirmed
    Given a hub that has just made its recovery seed
    Then the seed is still in the hub's data folder
    And the hub's pages say the recovery kit is waiting
    And a plan over the data folder leaves the seed file out

  Scenario: The confirmation is on a page that doesn't show the seed
    Given the recovery kit is open
    When the person goes to the confirmation
    Then the page holds no part of the seed
    And its field is separate from the password field and says that pasting, any case and the letters O, I and L are accepted

  Scenario: A group that isn't the one asked for doesn't confirm the kit
    Given the person is on the confirmation page, which asks for group 7
    When the person types a group that isn't the seed's seventh
    Then the hub refuses and the seed file stays

  Scenario: The confirmation asks for a new password and a random group
    Given the person opened the recovery kit with their password
    When the person goes to the confirmation
    Then the page asks for the password again
    And it shows the number of the group it asks for, which may differ the next time the page loads

  Scenario: The kit is confirmed with a pasted group in small letters, and the seed file is then deleted
    Given the person is on the confirmation page, which asks for group 7
    When the person pastes the seed's seventh group in small letters with an O where a zero belongs
    Then the hub deletes the seed file
    And the hub's pages no longer say the kit is waiting
    And the kit page says the hub no longer holds the seed and can't show it

  Scenario: The kit is confirmed on the web after the seven days
    Given a hub whose recovery seed has been unconfirmed for eight days
    When the person confirms with their password and the group asked for
    Then the hub deletes the seed file

  Scenario: A headless hub prints the kit in its terminal
    Given a hub with an unconfirmed recovery seed and no web browser
    When the person runs the hub's recovery-kit command in a terminal
    Then it prints the kit's text and a code that a camera can read
    And it warns that the terminal's scrollback keeps what it printed
    And it writes no file unless it is given a path

  Scenario: The recovery-kit command refuses to print to a pipe or a file
    Given a hub with an unconfirmed recovery seed
    When the person runs the hub's recovery-kit command with its output redirected to a file
    Then it refuses and writes nothing
    When the person runs it with --out and a new path
    Then it writes the kit to that path with only the owner able to read it
    When the person runs it with --out and a path that exists
    Then it refuses to overwrite

  Scenario: Recovery reads a seed as it was typed from paper
    Given a hub with a recovery seed
    When the person gives the seed in small letters, with spaces instead of hyphens and an O for a zero, from a file
    Then the hub shows the recovery key's fingerprint it derived before it changes anything
    And it recovers the hub
```
