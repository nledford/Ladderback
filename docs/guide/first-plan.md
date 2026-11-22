# Plans in config.toml

Ladderback keeps its repositories and plans in one file, `config.toml`, in its config folder: `~/.config/ladderback` on Linux, `~/Library/Application Support/ladderback` on macOS (`$XDG_CONFIG_HOME/ladderback` on Linux when that is set to a full path, and `$LADDERBACK_CONFIG_DIR` on either when that is set). The Repositories and Plans pages write it for you, as [Getting started](getting-started.md) shows, and keep its comments. You can write it by hand too: Ladderback reads the file again by itself, with no restart, and its start page lists each plan with its next run.

By hand, a plan takes keys the Plans page doesn't offer yet, such as a schedule of `every = "6h"`, tags, a time zone, retries, and a keep policy by length of time. A plan backs up to a repository by its name, so connect the repository on the Repositories page first, which writes its `[repositories.<name>]` table, then add a `[[plans]]` table after it.

This is a whole `config.toml` with one plan, which backs up a home folder every hour and leaves out caches and game libraries; on macOS, change `/home/demo` to `/Users/demo` in `iglobs` too:

```toml
schema = 1

# The repository, as the Repositories page wrote it, with its password moved
# out of this file (see the secrets guide). Yours may have more lines, such as its
# repository-id.
[repositories.nas]
repository = "rest:https://nas.example:8000/demo-laptop"
password-keyring = "ladderback/nas"

[[plans]]
id = "home"                        # the plan's name: lower-case letters, digits and hyphens
repository = "nas"                 # the name the repository was connected under
sources = ["/home/demo"]           # absolute paths; on macOS, /Users/demo
schedule = { cron = "0 * * * *" }  # on the hour, in this machine's time zone; or every = "6h"
# Skip every folder that holds one of these files, as many caches mark theirs.
exclude-if-present = ["CACHEDIR.TAG", ".nobackup"]
# rustic's globs, matched without regard to case; a leading "!" leaves out what matches.
iglobs = [
  "!/home/demo/.cache",
  "!/home/demo/.local/share/Steam",                # Steam's games
  "!/home/demo/.var/app/com.valvesoftware.Steam",  # Steam from Flathub
  "!/home/demo/Games",                             # Lutris, Heroic and the like
  "!node_modules",
  "!target",
]

# The keep policy: a snapshot an hour for a day, one a day for a week, one a
# week for a month, and one a month for a year.
[plans.retention]
keep-within-hourly = "1 day"
keep-within-daily = "7 days"
keep-within-weekly = "1 month"
keep-within-monthly = "1 year"
```

The keep policy takes effect when you press **Forget snapshots** on the Snapshots page, as [Getting started](getting-started.md#known-limits) says. Every key a plan takes, among them tags, a time zone, retries and hooks, is in [technical design §6.1](https://github.com/nledford/Ladderback/blob/main/docs/design/scheduling-and-config.md#61-source-of-truth); the filter and `keep-*` keys are rustic's own.

## Importing a rustic profile

`ladderback import-profile <profile.toml>` adds a rustic profile's repository and its snapshot entries, as plans, to the configuration. When the profile runs commands (hooks, a `password-command`, an `rclone-command`), the import lists each one in full and asks `Import them? [y/N]` before it writes anything, since Ladderback then runs them on its jobs. Read them, and answer `y` only if you know what each does. In a script there is nobody to ask, so pass `--yes` once you have read them; without it nothing is imported. A profile's global hooks are imported only into an empty configuration: once you have a plan or a repository, add them to `[hooks]` yourself ([technical design §6.2](https://github.com/nledford/Ladderback/blob/main/docs/design/scheduling-and-config.md#62-importing-rustic-profiles)).
