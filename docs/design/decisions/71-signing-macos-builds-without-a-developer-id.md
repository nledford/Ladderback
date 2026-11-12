# Decision 71: Signing macOS builds without a Developer ID

**A build from source is signed with a self-signed certificate the person names; Developer ID signing and notarization wait until macOS archives are published** (recorded 2026-11-12, §12; #281). Ladderback has no paid Apple Developer account, so no Developer ID certificate and no notarization. In M3, `just install` on macOS signs the installed binary with a code-signing identity from the person's own keychain, a self-signed certificate made in Keychain Access, when they name one. Without one it installs as today, with the linker's ad-hoc signature. The Full Disk Access check of §12 (a bare CLI binary refused by the settings pane) runs on that signed build.

Why:

- **The grant follows the signature.** TCC ties a Full Disk Access grant to the binary's code signature (§12). An ad-hoc signature is derived from the binary's own contents, so every rebuild is a different program to TCC and the grant is lost on each upgrade. A certificate's signature names the certificate rather than the contents, so a grant given to one build should hold for the next build signed with the same certificate. M3's test confirms that on the current macOS release before the README tells anyone to rely on it.
- **Notarization matters only for downloads.** Gatekeeper checks files that carry the quarantine flag a browser or other download adds. A binary built on the machine has none, so a source build needs no notarization; a downloaded archive does. Archives aren't published until M5 (decision 54), so Developer ID signing and notarization become a separate issue, not scheduled, for when they are.
- **Nothing secret enters the build.** The identity stays in the person's keychain, and `codesign` reaches it there; no certificate or password is passed to the recipe, stored in the checkout or used in CI.

Cost: until M3, and afterwards for anyone who names no identity, a macOS upgrade means granting Full Disk Access again. A published macOS archive needs the paid account first.
