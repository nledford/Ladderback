# Ladderback

Ladderback runs [rustic](https://rustic.cli.rs) backups as a service on each machine you back up, with a web UI built in. You write plans: what to back up, into which repository, how often, and how long to keep its snapshots. Ladderback runs them on their schedule, catches up on a run the machine slept through, and shows each backup's progress and result.

These guides are for running it. The [README](https://github.com/nledford/Ladderback/blob/main/README.md) has the short version and what Ladderback does today, and the [technical design](https://github.com/nledford/Ladderback/blob/main/docs/design/README.md) says how it works.

- [Install](install.md): the recommended way for each system (the Linux packages, the install script, signing on macOS), the other ways, updating, running it as a service, and what macOS asks for.
- [A first plan](first-plan.md): connect a repository, add a plan to `config.toml`, and read its keep policy.
- [Moving a repository's password out of `config.toml`](secrets.md): the system keyring, a password file or an environment variable.
- [Sign-in and remote access](remote-access.md): setting an admin password, `--listen` and `--allowed-host`, and TLS behind a reverse proxy.
