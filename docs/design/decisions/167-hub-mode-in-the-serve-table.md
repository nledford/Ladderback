# Decision 167: Where hub mode lives in `config.toml`, and what turning it off does while machines are paired

**Hub mode, the listening address and the allowed host names are the `[serve]` table of `config.toml`: `hub`, `listen` and `allowed-hosts`, one key for each of `serve`'s flags `--hub`, `--listen` and `--allowed-host`. `serve` reads the table at start, and a flag given replaces its key: `--listen` the address, any `--allowed-host` the whole list, `--hub` or `--hub=false` hub mode. Hub mode turned off while machines are paired stays on: the daemon starts as the hub and says which machines keep it on, to be removed first** (recorded 2026-11-25, §6.1 and §10.1; settles the names decision 136 left "settled when built", and amends its point 5; #1202).

## Why

- **One table, one key per flag.** The keys say what `serve` starts with, so they sit apart from `[agent]`, whose keys describe the machine, and from a later `[hub]` table that a paired machine would use to name its hub. Decision 163 keeps "agent" out of new names. Each key has the flag's own name, so the guide's and `--help`'s words match.
- **A flag replaces, never merges.** `--allowed-host` given once replaces the list rather than adding to it, so the command line says the whole of what is in effect, as decision 136's point 4 wants the Settings form to show.
- **Read at start only, under the lock.** Who can reach the daemon is decided before it binds its listener (§10.1), so a change to the table takes effect at the next start, as the flags do. The table is read with the rest of the file, after the data folder is locked (§9), so a second `serve` reads nothing; who can reach the daemon is then set before it listens, and a refusal from here on closes the operation log first.
- **The address is checked where the file is read** (`ServeSettings::parse_listen`, a located error at `serve.listen`). The host names are checked at start by the same check `--allowed-host` gets, which lives in the HTTP adapter the config adapter doesn't depend on; a bad name stops the start and names `config.toml`.
- **Kept on, not refused.** A daemon that refuses to start stops every backup of its own machine and takes the hub away from every paired machine, and a service manager would start it again every 10 seconds, failing each time. Keeping hub mode on loses nothing: the machines keep their hub, this machine keeps backing up, and the start says, on stderr and in the log, which machines to remove (in Settings, on the Machines tab) before hub mode can turn off. A registry that can't be read keeps hub mode on only when the hub key in the data folder shows the machine was a hub; a machine that never was one stays off. Either way the start says so.
- **A waiting machine counts as paired** until its wait lapses: it holds a token the hub accepted, and turning hub mode off would strand it as surely as an approved one.
- **A build without the hub** (decision 122) reads `hub = true`, logs that it has no hub, and serves as a machine: the file stays valid for every build.

## Decided

1. `[serve]` with `listen` (an IP address with a port), `allowed-hosts` (a list of names or addresses without a port) and `hub` (a bool, default false). Left out, each takes the flag's default: `127.0.0.1:9898`, none, off.
2. `--listen` has no default of its own any more; `--hub` takes an optional value given with `=`, so `--hub` is on and `--hub=false` is off.
3. `install-service` and the service files are unchanged: the service runs `serve --service-managed`, and the table says the rest.
4. Hub mode resolved off while an approved machine, or a waiting one whose wait hasn't lapsed, is registered: the daemon starts as the hub and warns, one line per machine (`decide_hub_mode_off` in `domain::fleet`).

## What it costs

- A change made to the table while the daemon runs is read only at the next start, which is also when hub mode turned off is checked.
- Until a Settings form writes the table, a person edits `config.toml` by hand.
- The warning goes to the daemon's output and log at start; nothing on the page says hub mode was kept on yet.

## Rejected

- **Refusing to start with hub mode off while machines are paired** (decision 136's point 5 as first written): it stops the machine's own backups and every paired machine's hub, and loops under a service manager, all to report something a warning reports as well.
- **The keys in `[agent]`:** that table describes the machine, and would grow keys named for `serve`'s flags under an "agent" heading decision 163 retires.
- **A flag that adds host names to the file's list:** the command line would no longer say what is in effect.
- **Checking the host names in the config adapter:** it would need the HTTP adapter's check, a dependency the layering doesn't allow, or a second copy of it.
