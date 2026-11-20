# Moving a repository's password out of config.toml

The Repositories page saves the password you type into `config.toml`, and the page and the log warn that it is there. To move it, keep it in one of these places, then replace the repository's `password = "…"` line with the line that names it; a repository has exactly one:

- **The system keyring**, on a desktop or laptop. Store the password under the service `ladderback` and the repository's name; each command asks for it, so it stays out of your shell's history:

  ```sh
  # macOS: the login keychain
  security add-generic-password -s ladderback -a nas -w
  # Linux: the Secret Service, such as oo7, GNOME Keyring or KWallet
  secret-tool store --label="Ladderback: nas" service ladderback username nas
  ```

  Then write `password-keyring = "ladderback/nas"`, the service and the name.

  On Linux the Secret Service starts with your desktop session: oo7 (`oo7-daemon`) on Fedora 45, GNOME Keyring, or KDE's KWallet, which hasn't been tested with Ladderback. Where there is none, the install script's closing notes say so. A service that keeps running while you are logged out (`loginctl enable-linger`) starts at boot, before you log in, and can't read the keyring until you do, which `ladderback install-service` points out. On a machine that backs up while you are logged out, use a password file instead.

  The service can also start a little after Ladderback at login. A run that starts on its own in those first moments, such as a scheduled run or a catch-up run for one the machine slept through, then finds no keyring. In the first 2 minutes after Ladderback starts, it reads the keyring again every 10 seconds until the service answers, and Ladderback's log says it is waiting. A locked keyring, or an unlock prompt you dismissed, isn't asked again: that run fails at once, as does a run or a test you start yourself. For a locked keyring its message says so and how to unlock it; for a dismissed prompt it says the keyring is unavailable. Running that plan again by hand once you are logged in and the keyring is unlocked backs it up.

  On macOS, the keychain asks you in a prompt whether Ladderback may read the entry, the first time it does and again after you install a new build, even one you sign yourself ([#892](https://github.com/nledford/Ladderback/issues/892) says why). A run waits up to 60 seconds for the keyring to answer. When nobody answers the prompt in that time, such as for a scheduled run while you are away, the run fails, and its message says the keyring didn't answer in time. Answer the prompt with **Always Allow**, then run the plan again.

  After you install a new build, run `ladderback check-keyring` in a terminal: it reads each keyring entry your configuration names once, so the prompt comes while you are there, and says which entries it could read, never what they hold. `just install-restart` runs it for you before it restarts the service.
- **A password file** that only you can read. Ladderback reads its first line, and warns when others can read it:

  ```sh
  touch ~/.config/ladderback/nas.password && chmod 600 ~/.config/ladderback/nas.password
  ```

  Write the password on its first line with an editor, then write `password-file = "/home/demo/.config/ladderback/nas.password"`: the path is absolute, since nothing expands `~`. A file that doesn't answer within 60 seconds, such as one on a network share that stopped answering, fails the run, and its message says so. On macOS, keep the file in the config folder there, and quote the path, since it holds a space:

  ```sh
  touch "$HOME/Library/Application Support/ladderback/nas.password" && chmod 600 "$HOME/Library/Application Support/ladderback/nas.password"
  ```
- **An environment variable**: `password-env = "NAS_PASSWORD"`. The service doesn't get your shell's variables, so this suits `ladderback serve` started by hand, or a container.

The copies of earlier versions of `config.toml` in the `history` folder beside it may still hold the password: delete those that do. [Technical design §10.2](https://github.com/nledford/Ladderback/blob/main/docs/design/security.md#102-repository-secrets) has every source, a password command among them.

Keep passwords and tokens out of every command line too: a `password-command`, an `rclone-command`, a `post-create-command`, a `post-delete-command`, a hook and a notification channel's command. While a command runs, its whole line is visible to other users of the computer in the process list (`ps` on Linux; on macOS only root sees another user's arguments). A `password-command` should print the password, never hold it, and an rclone remote's password or token belongs in rclone's own configuration file. Ladderback warns when an `rclone-command` gives a flag a credential, such as `--pass=…`, naming the flag and never the credential. A flag that names a command or a file, such as `--password-command` or `--sftp-key-file`, doesn't bring the warning, and the warning never reads that command or file: it must hold no password either.
