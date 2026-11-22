# Running as a service

[Install](install.md#start-ladderback) starts Ladderback with `ladderback install-service`. This page says what that service is, how it restarts and stops, how to run Ladderback in a terminal instead, and what macOS's keychain and code signing ask of an update.

## Running it in a terminal

`ladderback serve` runs Ladderback until you stop it, and prints the address it listens on, `listening on http://127.0.0.1:9898`; `--listen` picks another address or port (beyond loopback only with `--allowed-host`; [Remote access](remote-access.md) has the details). Its configuration and data live in your platform's config and data folders, which [Install](install.md#where-ladderback-keeps-your-files) lists; `LADDERBACK_CONFIG_DIR`, `LADDERBACK_DATA_DIR` and `LADDERBACK_CACHE_DIR` move them, for a second `ladderback serve` beside the service ([technical design §4.2](https://github.com/nledford/Ladderback/blob/main/docs/design/components/daemon.md#42-daemon-agent), "Config").

## The service

To run it in the background instead, from each login on, run `ladderback install-service` as yourself, not under `sudo`. On Linux it writes a systemd user unit and has systemd start it; on macOS it writes a launchd agent, which logs to `~/Library/Logs/ladderback.log`. The service runs `ladderback serve --service-managed`, so it listens on `http://127.0.0.1:9898`, and Settings can restart it. On Linux a user's services stop when the user logs out, and unless you already linger the command says to run `loginctl enable-linger` to keep it running. A service that lingers starts before you log in, when it can't read a password in the keyring yet, so the command says so too when a repository keeps its password there, and suggests a [password file](secrets.md) instead.

**Restart Ladderback** in Settings restarts the service, now or once the running operations finish, and the page reloads once it is back (systemd waits 10 seconds before it starts the service again). A service installed by an earlier version can't be restarted from there: run `ladderback install-service` again first.

If the service stops without its service manager asking it to, after a crash or a `kill` from another program, the manager starts it again: systemd after 10 seconds, launchd within 10 seconds. So a `kill` doesn't stop it, nor free its data folder for another `ladderback serve`. To stop it until your next login, run `systemctl --user stop ladderback` on Linux or `launchctl bootout gui/$(id -u)/io.github.nledford.ladderback` on macOS. A service installed by an earlier version stays stopped after another program's `kill`: run `ladderback install-service` again.

## Updating without interrupting a backup

After an update, run `ladderback install-service` again: it replaces the service and starts it on the new binary. From a checkout, `just install-restart` does both without interrupting a backup: it builds with `just install` (its other arguments go there) into a staging folder beside the installed `ladderback`, waits until the service's operation log shows no operation running or queued (up to 2 hours; `--wait <minutes>` changes that, `--now` skips the wait), then moves the new binary into place, restarts the service at once, and checks that it runs a new process of that build. Until the restart the service's binary stays in place, so a backup that starts during the wait can still read a keyring password. It doesn't restart a service when it can't tell whether it is busy, as when the service isn't running, unless `--now`.

`INSTALL_RESTART_ADDRESS=<host:port>` names the address of a service whose file you edited to listen elsewhere than `127.0.0.1:9898`. A hidden `.ladderback-install-` folder beside `ladderback` is one a run that was killed left behind: delete it. `ladderback remove-service` stops the service and removes its file. [Decision 64](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/64-installing-the-agent-as-a-user-service.md) says what the service file holds.

## macOS: signing and the keychain

[Signing your build](install-macos.md#signing-your-build) and [Full Disk Access](install-macos.md#full-disk-access-and-photos) say what to do on macOS; this section says why, and what can go wrong. A build from source carries the linker's ad-hoc signature, which changes with every build, so macOS sees each update as a new program, drops its Full Disk Access grant and asks again for Photos and the keychain ([decision 71](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/71-signing-macos-builds-without-a-developer-id.md)). [Technical design §12](https://github.com/nledford/Ladderback/blob/main/docs/design/operations.md#12-packaging-and-deployment) says what a prompt you decline or leave unanswered costs. Signed with your certificate under one fixed identifier, every build has the same signature to macOS, so its Full Disk Access and Photos grants carry over to the next update (tested on macOS 27.3, [#281](https://github.com/nledford/Ladderback/issues/281)).

The keychain still asks once for each new build, signed or not, before it lets the build read a keyring entry: a self-signed certificate has no Apple Team ID, so "Always Allow" lets in only the build that was answered ([#892](https://github.com/nledford/Ladderback/issues/892), [technical design §12](https://github.com/nledford/Ladderback/blob/main/docs/design/operations.md#12-packaging-and-deployment)). Left unanswered, the prompt fails the first scheduled backup, or notification, after an update. Any of these avoids that:

- `just install-restart` reads each keyring entry once with the new program, a repository's password and a notification channel's address, token or password, in your terminal, before it restarts the service, so the keychain asks while you are there. Before each read the terminal says which entry it reads: look for the keychain's question then, as it can open behind your other windows, and answer "Always Allow". A single "Allow" is for that one read, and the service is asked again. At a terminal each read waits 5 minutes for your answer.
- When an entry can't be read, `just install-restart` keeps the new build and, at a terminal, offers one more check: answer the question, or fix the entry, then press Enter. If that check fails too, or there is no terminal, it stops before the restart; run it again once the entry can be read, or with `--ignore-keyring-failure` to restart all the same. After any other update, run `ladderback check-keyring` yourself, which says and waits the same way.
- Read the entry with Apple's `security` program, which a new build of Ladderback doesn't change: `password-command = "security find-generic-password -s ladderback -a nas -w"` for the entry `ladderback/nas`. The keychain may ask once for `security` itself, so run that command once in a terminal and answer "Always Allow".
- Keep the password in a `password-file` only you can read (`chmod 600`).

Name the certificate in `LADDERBACK_SIGN_IDENTITY` for `just install` or `just install-restart` in a checkout too:

```sh
LADDERBACK_SIGN_IDENTITY="Ladderback Code Signing" just install-restart
```

The install script passes the variable on to the `just install` it runs. It builds the newest release, and every release from 0.4.0 on signs its build; releases up to 0.3.1 sign nothing.

`just install` checks the name before it builds, and installs nothing when `security` lists no valid certificate of exactly that name, or lists more than one: then name one by the SHA-1 hash it shows, or, when it is one certificate kept in two keychains, delete a copy. After installing, it signs the binary with the certificate under the identifier `io.github.nledford.ladderback` and checks the signature. Only the name passes through; the key stays in your keychain.

Over SSH, or with the keychain locked, nobody can answer the first signing's question whether `codesign` may use the key, so the signing fails. After `just install`, the new binary is installed by then, still with its ad-hoc signature (or, when the signature fails its check, with that signature), and the command says which. To sign the installed binary without building again, run `node scripts/install-signing.ts sign` in the checkout with the variable and the arguments you gave `just install`. `just install-restart` signs in its staging folder, so there a failed signing installs nothing and the service keeps running as it was: run it again once the keychain can answer. The first signed build is still new to macOS, so grant it Full Disk Access once more.
