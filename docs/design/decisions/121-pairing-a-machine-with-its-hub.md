# Decision 121: How a person pairs a machine with its hub

**A person pairs a machine by moving one pairing token from the hub to the agent: as text, as a QR code of the same text (in the web UI, or drawn in a terminal by `ladderback pairing-token`) or through a phone's share sheet. Both ends show the same key fingerprint, in one rendering, to compare. The agent then waits for a person to approve it on the hub. Every error says what to do next, and no step needs a camera, a mouse or color** (recorded 2026-11-18, §7.1, §4.5, §13.7.5; #567, the maintainer's directions of 2026-11-18 on pairing by QR code, from a phone and from a headless machine; revised the same day after independent review).

[Decision 120](120-agent-hub-connection-security.md) decides what is secure: the token's format, its server side, pending approval, the step-up, the keys and the handshake. This decision decides what the person sees and does. It links 120 for every security rule rather than restating it; where the two seem to differ, 120 governs.

## Where each item is answered

| Item | Answered in |
|---|---|
| The token as text, a QR code and a share | "The hub's dialog", "The QR code" |
| The camera and the scanner | "Scanning" |
| A phone as either end | "From a phone" |
| A headless hub or agent over SSH; `pairing-token`, `pair` | "From a terminal" |
| What the person compares, and where | "The fingerprint" |
| What each error says | "Errors and their next step" |
| Where a token can leak | "Where a token can leak" |
| Keyboard, screen-reader and color-free use | "Accessibility" |
| The QR crates | "Crates" |

## The fingerprint

One rendering of a key's fingerprint is shown wherever a person compares one: **20 Crockford base32 characters in five groups of four, upper case, with hyphens** (`7K3Q-M9XD-2RTV-8HFA-QW5N`), the first 100 bits of the SHA-256 of the key ([120, "The hub key"](120-agent-hub-connection-security.md)). The function is `domain`'s; this decision only says where it is shown.

- **The hub key's fingerprint** is in the hub's dialog, in `ladderback pairing-token`'s output, in Settings, and on the agent's confirm step. The person compares the agent's screen with the hub's.
- **The agent key's fingerprint** is on the hub's pending row and approval, and on the agent after it has paired. The person compares those two.
- **The 8-digit ID** (the first 8 hex digits) labels a key beside a name, in lists, in a step-up that asks for it and in alerts. It is always called "ID", never "fingerprint", and no screen asks the person to verify with it.
- **The full 64 hex digits** are one control away ("Show all") and in the command's `--long` output, for a person who wants to check more than the groups.
- **A screen reader** reads a fingerprint by group, with the full form behind "Show all". The text is selectable, so it can be copied.

## The hub's dialog

**Settings → Agents → New pairing token** asks for a life (15 minutes by default, 5 minutes to 24 hours) and an address, then shows one token three ways from the same bytes: as text, as a QR code and, where the browser has a share sheet, a Share button. Making a token is a high-impact action under [120, "Who may make a token, approve or re-key"](120-agent-hub-connection-security.md); this decision doesn't change its rules.

- **The dialog shows,** above the token: the hub key's fingerprint, the address, and when the token lapses as a local time with the UTC offset and a relative time ("14:47 UTC+01:00, in 15 minutes"). The address is what the person was using and is editable, since agents may reach the hub another way.
- **The text** is one unbroken string, selected whole by one click or tap, and wrapped by breaking anywhere, so a copy has no added line breaks. Focus starts on Copy, as in the API-token dialog.
- **Copy** puts the text on the clipboard and says so in a status line. **Share** carries the text only.
- **Cancel this token** is in the dialog. **Escape and Close revoke an unused token** as well, and the dialog says so beside Close, because its text can't be shown again ([120, "Pairing"](120-agent-hub-connection-security.md): shown once). A token made by the command line is revoked from Settings or with `pairing-token --revoke`.
- **When the token is used or lapses,** the dialog replaces the token, the QR code, Copy and Share with a `role="status"` line inside the dialog, so a screen reader hears it while the modal is open. When used, it names who: "Used by laptop (ID 3F9A8B21) at 14:32. If you didn't expect this, look at the pending agent below and reject it." When lapsed: "Lapsed at 14:47. Make a new token."
- **Focus moves with the content.** After use it goes to the new agent's row in the dialog (a focusable group with its hostname, ID, key fingerprint, Approve and Reject); after a lapse or a cancel it goes to "Make a new token", as the shared `hand_focus_when_gone` does for any control that vanishes.
- **Approval is in the same place.** The pending row shows the agent's hostname, ID and key fingerprint, and the person compares them with the agent's own screen before approving, with the step-up of 120. A name with any non-ASCII character is marked, and its ID shows beside it, so a lookalike name is told apart.
- **A list of unused tokens** is in Settings → Agents: each with its ID, who made it, its life and a Revoke button. A rotation of the hub key voids them and the list says so.
- **A secure context is needed to show or take a token.** Where the page isn't one, it says so in text and shows nothing ([120, "Who may make a token, approve or re-key"](120-agent-hub-connection-security.md)).

## The QR code

The QR is the token's text, drawn in the browser from the token the page already holds, by a pure-Rust encoder compiled to WebAssembly ("Crates"). No network service draws it, and the daemon is asked for nothing more. It adds no secret and no format.

- **The SVG is black on white,** whatever the theme, with its own white quiet zone of 4 modules, so a dark theme can't invert it. The elements set `forced-color-adjust: none`, because Windows high-contrast mode repaints `fill` and can invert a code.
- **The error-correction level is L,** unless M keeps the same version, since a screen isn't damaged and a denser module is harder for a webcam. For a 191-character token, L is version 8 (49 modules) and M version 10 (57 modules).
- **The code scales with text size** and prints sharply; a print rule hides the token, the QR code and Copy, so a printout of the dialog holds none of it.
- **A Wake Lock is held while the dialog is open,** where the browser has one, so a phone doesn't lock mid-scan; it is released on close. A browser without one loses nothing else.
- **The QR's text alternative** says what it is, "a pairing code for this hub", and never holds the token.
- **The token's format is not changed for size.** A denser encoding (alphanumeric mode, with a longer upper-case token and a new prefix) gains at most one version and can't reach an 80×24 terminal; the only real shrinks are braille or sextant cells, which are too fragile to scan, or dropping the certificate pin, which is a security decision for 120.
- **It is never the only way.** The text, Copy and the paste field stay (A1, A2 and A5 of §0; §4.5). The QR is shared component work under §13.7.5, built in its own PR before the pairing page.

## Scanning

The agent's pairing page takes the token pasted or scanned. The scanner is a shared component, built in its own PR.

- **The camera is asked for only when the person presses "Scan the code",** never when the page opens, and is released at once on a read, a cancel, an error and when the page is left. On a phone it asks for the rear camera (`facingMode: environment`).
- **Frames stay in the browser.** They go to a video element and a canvas, and a decoder in WebAssembly reads them; the page keeps none, and sends, stores and logs none. The page's policy (`default-src 'self'`) gives it nowhere else to send them, and no `Permissions-Policy` header on the pairing page may deny the camera to its own origin.
- **Decoding is throttled** to at most five frames a second, on a frame scaled to at most 640 pixels on its longer side, so Escape and typing never lag. A mirrored preview is a display setting only: the decoder reads the unmirrored frame.
- **What is read is checked before it is used:** the prefix, the checksum and the version. Only then does the page show the hub address, the hub key's fingerprint and the mode (CA or pinned) for the person to confirm; the token goes to the daemon only after that.
- **The status line always says something,** in a `role="status"` region: "Looking for a code" while it looks; after 30 seconds, "No code found. Move closer, raise the screen's brightness, or paste the token." (it keeps looking); "That code isn't a Ladderback pairing token" for a code that isn't one, at most once every 5 seconds; "Code read: hub 7K3Q-M9XD-2RTV-8HFA-QW5N" on a read. After a read focus moves to the confirm panel's heading.
- **Where the camera can't be used the page says why,** in text, and the paste field stays: no camera, "the camera is in use by another app", permission declined, no secure context, no camera API.
- **A secure context is needed for the camera.** `localhost` and loopback are secure contexts, so an agent's UI on its own machine can scan; a remote UI needs HTTPS ("From a phone").
- **Scanning is optional and reachable by keyboard:** a button, the live status, Escape to stop, and no part of any task rests on seeing the picture.

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

### `ladderback pairing-token` (on the hub)

- **It makes one token,** as the dialog does, with `--ttl` (5 minutes to 24 hours; 15 by default) and `--address`. With no address configured and none given, it asks for one.
- **It prints,** in this order: a line with the hub key's fingerprint ("Hub key: 7K3Q-M9XD-2RTV-8HFA-QW5N"), so a headless person has something to compare; the token as text; a line saying the token is one-time and when it lapses, as a local time with the UTC offset and a relative time; and, when the code applies, the QR code.
- **`--list` and `--revoke <id>`** list unused tokens by ID with their lives, and revoke one ([120, "Pairing"](120-agent-hub-connection-security.md)).
- **In a terminal it waits for the pairing,** until the token lapses, and says when an agent has paired: "laptop (ID 3F9A8B21), key 5H2R-…, is waiting. Approve this agent? [y/N]". `y` approves; anything else leaves it pending, to approve from the Agents list. The prompt names the hostname and the key fingerprint, and marks any non-ASCII character. `--no-wait` prints and returns.
- **A pipe or a file gets the token and nothing else.** When standard output isn't a terminal, the command prints the token alone, doesn't wait and doesn't prompt, so a script receives exactly the token.
- **It is never logged.** The command writes the token to the terminal only. The daemon's log and its own output never hold a token, a QR code or the pairing key; the hub's event for a new token names its ID and when it lapses, never the token.

### The terminal's QR code

- **It is drawn with Unicode half blocks** (`▀`, two modules to a cell) from the same module matrix as the dialog's SVG, so one crate serves both. Its colors are explicit: black foreground and white background by the 256-color indices 16 and 231, which themes don't remap, reset at the end of every line, with a quiet zone of 4 modules drawn in white and the row count padded to an even number.
- **Level L, unless M keeps the version,** as in the dialog.
- **It is text only when the person asked for text or color can't be trusted:** with `--no-qr`, `--plain`, `NO_COLOR` set, `TERM=dumb` or a standard output that isn't a terminal. The command says so in a line before the token ("No QR code: NO_COLOR is set; paste the token instead"). The QR's colors are its data, so it never goes through decision 38's color stripping, and a stripped code that prints as unreadable glyphs is worse than none. With the code, a line before it says "A QR code follows; skip it with --no-qr", for a screen reader's user.
- **The size is computed from the actual token,** not assumed. A 191-character token is a version 8 code, 57 columns by 29 rows with its quiet zone, and one of 226 characters is 61 by 31; the longest token (a 200-byte address) is 89 by 45. Columns are the hard limit, since a wrapped code is corrupt; rows only cost scrolling.
- **When the window is too small the command waits** rather than giving up and minting a new token on a re-run. It prints the text, then "This code needs 57 columns by 29 rows; the window is 80 by 24. Enlarge the window or zoom out, then press Enter, or q to skip the code", and measures again on each Enter, until it fits or the person skips. If it can't read the size, or a keypress can't be read (a dumb terminal), it skips the code and says why.
- **A later option: the Kitty graphics protocol** (Ghostty, Kitty) would draw a crisper image where the terminal reports it, with the half blocks as its fallback. It isn't in the first version and needs no crate beyond the image's bytes.

### `ladderback pair` (on the agent)

- **It never takes the token as an argument.** A token on argv is refused with a message that says how to give it, as the MCP token's is (§10.5): at the prompt, as standard input, or from `--token-file`. The maintainer's sketch `ladderback pair <address> <token>` is therefore not offered. The token carries the hub address; `ladderback pair <address>` overrides it for an agent that reaches the hub another way.
- **The prompt reads a paste without echo,** as `set-password` does (decision 70), and is tolerant: it strips whitespace, line breaks, zero-width characters, surrounding quotes and angle brackets, accepts the prefix in any case with `-` for `_`, and reads until the checksum validates or a blank line arrives, so a token wrapped across lines or pasted with a trailing newline isn't truncated. It can't repair a token's letters whose case a chat app changed (base64 is case-sensitive): the checksum catches that, and the error says so.
- **It echoes what it read before it asks.** "Read 191 characters. Hub https://hub.example:9899, key 7K3Q-M9XD-2RTV-8HFA-QW5N, pinned certificate. Compare the fingerprint with the hub's. Pair? [y/N]". Nothing is sent before `y`.
- **`--hostname <name>`** pairs under another hostname than the machine's, for `hostname-taken`.
- **`--yes` isn't offered for a key that replaces a pin.** When the hub key in the token isn't the one the agent holds, it asks and names both fingerprints; for a standard input that isn't a terminal it needs `--replace-pin <fingerprint>` naming the new key, so a script can't replace a pin by accident.
- **`--token-file`** warns when the file's mode lets another user read it, and says to delete the file after pairing.
- **It tells the person what happens next:** it shows its own key's fingerprint ("Compare this with the hub: 5H2R-…") and "Waiting for a person to approve this agent on the hub", and says the agent keeps backing up meanwhile ([120, "Pending approval"](120-agent-hub-connection-security.md)).
- **From a hub to a headless agent in one line:** `ladderback pairing-token | ssh agent ladderback pair` works, because a pipe carries the token alone and `pair` reads standard input.
- **A headless agent can't scan.** It pairs with the token pasted at the prompt, sent from a phone through Universal Clipboard, AirDrop or the share sheet to the machine whose terminal runs SSH. The scan is for a phone or laptop with a camera that pairs the agent through its browser. And a headless hub can still be paired from a phone: the phone scans the code in the hub's terminal and its browser, signed in to the agent, pairs it.

## Errors and their next step

Each is a catalog message in the web UI and the command line, with the code in the log. The text says what happened and what to do; none leaves the person at a dead end.

| Code | What the person is told |
|---|---|
| `token-used` | "Someone else may have used this token. Look at the agents list on the hub; reject any agent you don't know, then make a new token." |
| `token-expired` | "This token lapsed. Make a new one on the hub." |
| `token-revoked` | "This token was canceled on the hub. Make a new one." |
| `token-malformed` (prefix or checksum) | "That isn't a whole token. Copy all of it again: a wrapped line or a chat app may have cut or changed it." |
| `token-unsupported` | "This token is newer than this agent. Update the agent, then pair." |
| `hostname-taken` | "The hub already has an agent named laptop (last seen 3 days ago). If this is a different machine, pair under another name: add `--hostname`, or set it on this page. If it is the same machine and its data folder was lost, don't rename it: remove the old agent on the hub, then pair with this same token." |
| `agent-key-mismatch` | "The hub has another machine under this agent's ID. Don't pair again; ask whoever runs the hub." |
| `token-not-for-this-agent` | "This is a replacement token for another agent. Make a replacement token for this one." |
| `hub-key-mismatch` | "This hub's key isn't the one this agent trusts. If you didn't reinstall or reset the hub, don't pair again: someone may be posing as it." |
| `hub-proof-invalid` | Same as `hub-key-mismatch`, with "the hub didn't prove its key" |
| `proof-refused` | "The hub refused this agent. It may have been removed, or its key may have changed. Look at the agents list on the hub; to bring it back, make a replacement token for it." One message for every cause, since the hub says no more |
| `identity-conflict` | "Another machine is using this agent's identity. This one keeps backing up and runs no maintenance. Make a replacement token for it on the hub." |
| `pending-limit` | "The hub already has 20 agents waiting for approval. Approve or reject some, then pair again with this token." |
| `pairing-needs-sign-in` | "Set an admin password on this machine before pairing from another computer." |
| `step-up-failed` | "That password didn't match. Try again." |
| out of date ([120, "Versions"](120-agent-hub-connection-security.md)) | "The hub is on an older release. This agent keeps backing up. Update the hub, or accept the older hub here" |

Messages are localized (Fluent). A code's text never includes a token.

## Where a token can leak

A token is a one-time, short-lived credential: whoever holds it unused can pair one agent, and the agent is held pending until a person approves it ([120, "Pending approval"](120-agent-hub-connection-security.md)). These are the ways it leaves the hub's page.

- **The text and the QR are the same secret.** A screenshot, or a photo of the screen, holds the same token the text does. A QR on a phone in public can be read by anyone who can see the screen; the dialog hides it once used, lapsed or canceled. The default life stays 15 minutes, and a person about to show it in public can pick 5.
- **Copy** puts the token on the clipboard, where other apps, a clipboard history or a synchronized clipboard may read it. Ladderback can't clear another app's copy.
- **A share leaves Ladderback.** It lands in whatever app the person picks, which keeps it, may sync it and may index it, such as a notes app or a photo library that syncs to a cloud. The dialog says to share it only to the person's own devices. A share doesn't lengthen the token's life.
- **The terminal keeps what it shows.** The command prints the token and its code into the scrollback, a `tmux` or `screen` log, a session recording and the screen of an SSH session. The command says the token is one-time and short-lived, and the person can clear the screen or choose 5 minutes. A recording is outside Ladderback's control.
- **A leaked token can be revoked** from the dialog, Settings or `--revoke`, and a rogue that uses one first is held pending for a person to reject.
- **Shell history and process lists** hold nothing: neither `pair` nor `pairing-token` takes the token as an argument. `--ttl`, `--address` and `--hostname` are not secrets.
- **A scan that goes wrong holds nothing:** frames aren't stored, and a code that isn't a token is dropped.

## Accessibility

- **Every task has a keyboard and screen-reader path,** and the paste path needs no camera, no pointer and no color (§0 A1, A2, A5; §4.5).
- **State changes are announced in the dialog,** not in a live region outside it that a modal makes inert: the status lines above.
- **Focus** is placed on arrival (Copy), kept when content vanishes (to the agent's row or to "Make a new token"), and moved to the confirm panel after a scan.
- **Color is never the only signal.** The terminal QR's colors are data; every message around it is text, and `NO_COLOR` and `--plain` mean text only.
- **Fingerprints** read by group; hostnames with non-ASCII characters are marked in text, not by color.
- **The shared components** (the QR and the scanner) are built to the checklist of §13.7.5 before the page uses them.

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

- **The fingerprint.** One function renders every fingerprint shown, at 100 bits, with fixed vectors; the dialog, the command, Settings, the agent's confirm step and the pending row show the same string for one key; the ID is never labeled a fingerprint.
- **The QR and the camera.** The QR decodes to exactly the token's text. The SVG is black on white with a 4-module quiet zone and `forced-color-adjust: none`, at level L or an M of the same version. The camera is asked for only on pressing "Scan the code", is released on a read, a cancel, an error and a page leave, and no frame is stored. A scanned text that isn't a token is dropped with a status line. Scanning is offered only where the camera API and a secure context are. The QR goes with the token when it is used, lapsed or canceled. The QR's text alternative doesn't hold the token.
- **The dialog.** A used token replaces the content with a status line naming who used it, and focus goes to the agent's row; a lapse puts focus on "Make a new token"; Cancel and Close revoke an unused token; a list shows unused tokens and revokes them; the expiry shows a UTC offset; a print rule hides the token.
- **The terminal.** `pairing-token` prints the hub fingerprint, the token, the one-time line and a code whose half blocks decode, in a test, to exactly the token, with a 4-module quiet zone and explicit colors. It prints the token alone to a pipe. `NO_COLOR`, `--plain` and `--no-qr` print text and a line that says why. A window too small waits, measures again on Enter and never mints a second token. The size shown matches the computed one for tokens from 191 to 420 characters. In a terminal it waits for the pairing and prompts, and `n` leaves the agent pending. The token and the code are in no log line, and the command refuses to run as another user than the one that owns the data folder.
- **`pair`.** A token on argv is refused with how to give it. A token wrapped across lines, quoted or with a trailing newline is read; one with letters changed fails the checksum with the message. It echoes what it read and asks before it sends anything. A changed hub key needs `--replace-pin` with a standard input that isn't a terminal. A `--token-file` that others can read warns. `--hostname` pairs under that name.
- **Errors.** Every code in the table has a message with a next step in the catalog, and none holds a token.
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
    And the hub's pending row shows the same fingerprint with the agent's hostname and ID

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
    Then the command names the agent's hostname and key fingerprint and asks to approve it
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

  Scenario: A used token tells the real agent what to do
    Given a stolen token that another machine has used
    When the real agent pairs with it
    Then the agent says the token was used and to look at the hub's agents list
```
