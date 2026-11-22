# Ladderback

Ladderback runs [rustic](https://rustic.cli.rs) backups as a service on each machine you back up, with a web UI built in. In the web UI you make plans: what to back up, into which repository, how often, and how long to keep its snapshots. Ladderback runs them on their schedule, catches up on a run the machine slept through, and shows each backup's progress and result. You restore a file or a folder from the same web UI.

This guide is in two parts. **Using Ladderback** is for everyone: install it, then do everything else in the web UI.

- [Install](install.md): the recommended way for each system, each on a page that installs, updates and removes Ladderback; starting it, and where it keeps your files.
- [Getting started](getting-started.md): from a fresh install to a first scheduled backup, and a test restore that proves it works.

**Advanced** is for people at home in a terminal, and for those who already use rustic. It links to the technical design for how each part works.

- [Other ways to install](other-ways-to-install.md): what the install script does, the release's archive by hand, cargo-binstall and building from source.
- [Docker image](docker.md): running Ladderback in a container, which isn't supported yet, and what it can't do there.
- [Running as a service](running-as-a-service.md): running Ladderback in a terminal, the service, updating without interrupting a backup, and what macOS's keychain and code signing ask.
- [Plans in config.toml](first-plan.md): the configuration file, a whole example plan, and importing a rustic profile.
- [Moving a repository's password out of config.toml](secrets.md): the system keyring, a password file or an environment variable.
- [Sign-in and remote access](remote-access.md): setting an admin password, `--listen` and `--allowed-host`, and TLS behind a reverse proxy.
- [API tokens and AI agents](api-tokens.md): which permissions to give a program or an AI agent, and what AI agents may do without a token.

The [README](https://github.com/nledford/Ladderback/blob/main/README.md) has what Ladderback does today and what comes next.
