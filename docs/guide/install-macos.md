# Install on macOS

On macOS the install script builds the newest release on your Mac, since releases have no download for macOS yet, and signs it with a certificate you make once, so that macOS keeps its permissions from one update to the next. The build takes a while.

## Signing your build

Sign the build with a certificate from your own keychain, so that macOS sees every update as the same program and keeps its Full Disk Access and Photos permissions. No Apple Developer account is needed. Make the certificate once:

1. In Keychain Access, choose Keychain Access → Certificate Assistant → Create a Certificate. Give it a name, such as `Ladderback Code Signing`, set Identity Type to Self Signed Root and Certificate Type to Code Signing, and create it. It goes into your login keychain.
2. Open the new certificate, expand Trust, and set Code Signing to Always Trust.
3. Check that macOS lists it as valid: `security find-identity -v -p codesigning` names it.

## Run the install script

The script asks once before it installs the build tools it can install by itself: Rust, through rustup, and just and trunk. A C compiler, git and Node.js 26 or newer it never installs: it names the missing ones with the command that installs them, and stops without changing anything. Name your certificate in the command:

```sh
curl -fsSL https://raw.githubusercontent.com/nledford/Ladderback/main/scripts/install.sh | LADDERBACK_SIGN_IDENTITY="Ladderback Code Signing" bash
```

The first signing asks whether `codesign` may use the key: choose Always Allow, and later installs don't ask. Without `LADDERBACK_SIGN_IDENTITY` the build is signed ad hoc, and after every update macOS drops its Full Disk Access, without a word, and asks again for Photos: until you grant Full Disk Access again, backups can't read the folders they protect. [Running as a service](running-as-a-service.md#macos-signing-and-the-keychain) has what signing changes and what to do when it fails, and [The install script in detail](other-ways-to-install.md#the-install-script-in-detail) how to read the script before it runs, and its options.

When it finishes, the script says what to run next. If it says to open a new terminal, or that your PATH doesn't lead to `ladderback`, do that first, or the next command isn't found. Then grant [Full Disk Access](#full-disk-access-and-photos), below, and [start Ladderback](install.md#start-ladderback).

## Full Disk Access and Photos

macOS lets a program read your Documents, Desktop, Photos and other protected folders only with your permission. Grant the installed `ladderback` Full Disk Access by hand, in System Settings → Privacy & Security → Full Disk Access, so that it can back them up: press **+**, then in the file dialog press ⌘⇧G and type `~/.cargo/bin/ladderback`, where the install script puts it. A [signed build](#signing-your-build) keeps the permission through updates; an unsigned one loses it with each update, so then grant it again after each one.

The first time a backup reads your Photos library, macOS may ask whether `ladderback` may use Photos. Allow it. Left unanswered while nobody is at the Mac, the question leaves that backup without those files; declined, macOS doesn't ask again, and later backups leave them out until you turn `ladderback` on in System Settings → Privacy & Security → Photos.

From macOS 27.3, System Settings may offer to grant access to some folders only, or to ask each time. Grant access to the whole disk: Ladderback backs up on a schedule, when nobody is there to answer, and how it fares with the other kinds hasn't been tested yet ([#940](https://github.com/nledford/Ladderback/issues/940)). With some folders only, its start page may say that Full Disk Access is missing even while backups of those folders succeed.

## Updating

Run the same command again, with `LADDERBACK_SIGN_IDENTITY` again, then run `ladderback install-service` again, which restarts Ladderback on the new release. The web UI's header says when a new version is available.

A repository whose password is in the keychain needs one more answer after an update: run `ladderback check-keyring` in a terminal. Before each read it says which keychain item it reads; macOS may then ask whether `ladderback` may use that item, in a window that can open behind your other windows. Choose Always Allow. The command waits 5 minutes for your answer. Do it before the next scheduled backup, which otherwise fails while the question waits for an answer. [Running as a service](running-as-a-service.md#macos-signing-and-the-keychain) says why.

## Removing

Stop Ladderback and remove its service, then the program:

```sh
ladderback remove-service
rm ~/.cargo/bin/ladderback
```

The build leaves more behind, each of which you can remove once nothing else needs it:

- the source the script built from, in `~/.local/share/ladderback/source`;
- the build tools it installed: `rustup self uninstall` removes Rust and the whole `~/.cargo` folder, just and trunk with it, and any other program installed there;
- `ladderback`'s entry under Full Disk Access in System Settings: select it and press **−**;
- the signing certificate, in Keychain Access, if you make no more builds with it.

Your settings, history and backups stay where they are: [What removing leaves behind](install.md#what-removing-leaves-behind) lists them, and how to delete them too.
