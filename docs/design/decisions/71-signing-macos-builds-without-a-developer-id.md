# Decision 71: Signing macOS builds without a Developer ID

**A build from source can be signed with a self-signed certificate the person names; a macOS archive is published only once it can be signed with a Developer ID and notarized** (recorded 2026-11-12, §12; #281, #344). Ladderback has no paid Apple Developer account, so no Developer ID certificate and no notarization.

- **M3:** on macOS, `just install` signs the installed binary with a code-signing identity from the person's own keychain when they name one, always with the same fixed identifier (`codesign -i`). Without one it installs as before, with the ad-hoc signature the linker gives every arm64 binary. The Full Disk Access checks of §12 run on the signed build.
- **From 0.3.1:** each release publishes the Linux archives (decision 54, amended by #582). A macOS archive waits for #344 (Developer ID signing and notarization, not scheduled); until then macOS users build from source.

**Amended 2026-11-16 (the maintainer, #281):** how `just install` signs.

- **The name:** the person names the identity in `LADDERBACK_SIGN_IDENTITY`, by its name or its SHA-1 hash, which `just install` and so `just install-restart` read; not a recipe argument, as the recipes' arguments go to `cargo install`.
- **The identifier:** `io.github.nledford.ladderback`, the launchd agent's label.
- **Refused before the build:** a name that `security find-identity -v -p codesigning` doesn't list as a valid identity's whole name or hash, or lists more than once, so a mistyped or untrusted certificate installs nothing. After the install it signs with that identity's hash, as `codesign` would also take a name as part of a longer one, then runs `codesign --verify` (scripts/install-signing.ts). A signing that fails then leaves the new binary installed, says how it is signed, and fails the recipe.
- **The note:** after a restart, `install-restart` warns that the keychain, Photos and Full Disk Access ask again for a build signed ad hoc, and says for a signed one that those grants carry over.

Making the certificate is in the [install guide](../../guide/install-macos.md#signing-your-build).

**Tested on a Mac, 2026-11-16 (macOS 27.3; #281):** with a self-signed certificate, a rebuild of an empty commit had a new code hash and the same designated requirement, and macOS asked for nothing on it:

- the Full Disk Access grant given to the first signed build held, and the next backup of the whole home folder ended `succeeded`, with no `source-unreadable`;
- the grant on the launchd daemon reaches its `worker` child, which needs none of its own (§17, question 2);
- a keychain item's "Always Allow", answered once for the first signed build, carried over, though the certificate has no Apple team ID (corrected below: it doesn't);
- System Settings accepts the bare binary, `~/.cargo/bin/ladderback`.

**Amended 2026-11-16 (#892, the maintainer approved the command):** the keychain asks once for each new build, signed or not. A keychain item's partition list lets a program in by its Apple Team ID, which a self-signed certificate lacks, so "Always Allow" adds only that build's code hash; the #281 test most likely read an item whose partition list already held that build's hash. The certificate still keeps Full Disk Access, Photos and the item's decrypt access list.

- **`ladderback check-keyring`** reads each keyring entry the configuration names once (a repository's password, a notification channel's address, token or password), with the program that runs it, so the keychain asks while someone is at the terminal (§12).
- **`install-restart`** runs it with the new build before it waits and restarts, and stops when an entry can't be read unless `--ignore-keyring-failure`. Its note after a restart says for a signed build that Full Disk Access and Photos carry over and the keychain asks once more.

Why:

- **The grant follows the signature.** TCC ties a Full Disk Access grant to the binary's designated requirement (§12). An ad-hoc signature's requirement is the binary's code hash (`cdhash H"…"`), so every rebuild is a different program to TCC and the grant is lost each time Ladderback is upgraded. A certificate's requirement is `identifier <id> and certificate leaf = H"…"`, the same for every build signed with that certificate under that identifier; checked with `codesign -d -r-` on two builds. That the grant then holds across an upgrade depends on TCC, not on `codesign`, so M3 confirms it on the current macOS release before the install guide tells anyone to rely on it. The identifier is fixed because `codesign` otherwise takes it from the file name, and a renamed copy would lose the grant.
- **Notarization matters only for downloads.** Gatekeeper assesses files that carry the quarantine attribute a browser or other download adds. A binary built on the machine has none, so a build from source needs no notarization; a downloaded archive does, and an unnotarized one is blocked unless each user overrides Gatekeeper. Hence no macOS archive without #344.
- **Nothing secret enters the build.** The identity stays in the person's keychain, and `codesign` finds it there; no certificate or password is passed to the recipe, stored in the checkout or used in CI.

Setting it up, for the install guide in M3: a certificate of the Code Signing type, made with Keychain Access's Certificate Assistant, in a keychain on the search list (normally the login keychain), unlocked. The first signing asks whether `codesign` may use the key; "Always Allow" answers it for later installs. Until then, a run that can't show the prompt, over SSH or with the keychain locked, fails to sign.

Cost: until M3, and afterwards for anyone who names no identity, each Ladderback upgrade on macOS means granting Full Disk Access again. A published macOS archive needs the paid account first.
