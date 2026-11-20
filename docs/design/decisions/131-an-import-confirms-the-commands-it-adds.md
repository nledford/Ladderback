# Decision 131: An import lists the commands it makes runnable, and asks first

**`ladderback import-profile` lists, in full, every command the import would make runnable, and writes `config.toml` only once the person has agreed to that list at a terminal or passed `--yes`. A profile's `[global.hooks]` join the agent's hooks only while the configuration has no plan and no repository; otherwise they are left out, with a notice. Each imported command is a typed `profile-command-imported` notice, which the preview returns too** (the maintainer's decision on #1135, 2026-11-19; §6.2).

## Why

A profile is a file someone else may have written, and importing it used to make its hooks, its `password-command` and its command options runnable without a word: the daemon would then run them on the next job. Its `[global.hooks]` became agent hooks, so they also ran around plans the profile never had. rustic would run the same commands, but a person running rustic chose that profile for that run, and an import makes them part of every job from then on.

## Decided

1. **What is listed.** Every command the saved configuration would run that the import adds: the four hook lists (`run-before`, `run-after`, `run-failed`, `run-finally`) at the agent, the repository and each plan; the repository's `password-command`; the options rustic runs as commands (`rclone-command`, `post-create-command`, `post-delete-command`), in `options`, `options-hot` and `options-cold`; and each program an `rclone:` location's connection string names (an sftp remote's `ssh`, a webdav remote's `bearer_token_command`, `override.password_command`), in a location written in one of its settings too, as deep as §6.1's relative-path check reads one. A command shows as its program and arguments, quoted as a shell would need them (as decision 89, point 6, shows a proposal's); an option shows its whole value, so a program named in one of rclone's flags shows too, and a location's program shows its setting's value.
2. **Where it is.** Each command is named by the configuration key that keeps it, not the profile's: `hooks.run-before`, `repositories.<alias>.repository`, `repositories.<alias>.password-command`, `repositories.<alias>.options.rclone-command`, `plans.<id>.hooks.run-after`. That is where the person finds it later. A value shows as the configuration keeps it: a `${` the profile wrote in an option shows as `$${`, the text the configuration reads as `${` (§6.2).
3. **The question.** At a terminal, the list goes to standard error, then `Import them? [y/N]`; only `y` or `Y` goes ahead. It is `pair`'s question (`src/confirm.rs`): `--yes` skips it, and away from a terminal nothing is asked and nothing is imported. A profile that runs no command is imported without a question, as before.
4. **Refused.** A declined answer, or no terminal without `--yes`, writes nothing and exits with 1, saying so. Through the use case it is the error `profile-commands-unconfirmed`, with no arguments.
5. **The list is checked again.** Asking takes time, and the profile, a profile it uses, or the configuration can change before the answer. So the import is run again after the answer, with the list the person saw, and saves only if it would make exactly those commands runnable; any other list is refused with `profile-commands-unconfirmed`, and nothing is written.
6. **`--json` never asks.** A script reads no question, so without `--yes` a profile that runs a command is refused with `profile-commands-unconfirmed` and exit 1, where it used to be imported. With `--yes`, the document's `notices` carry one `profile-command-imported` per command, in the listed order.
7. **The notice.** `profile-command-imported` carries `key` and `program`, the program alone, never its arguments: the preview and the JSON document may be read by an AI agent or another program, and a command a person wrote may carry a token of its own (#856, as `profile-command-left-out`). An option rustic can't split as a command line names its first word. Text output prints no such notice, since the list already shows each command.
8. **The list in words.** The full lines go only to the standard error of the command that imports, before its question or, away from a terminal, before its refusal, so a script that can't answer sees what it would agree to with `--yes`. They are the person's own profile, on their own machine (decision 99). A profile may still be someone else's text, so each control character and each bidirectional format character in a key or a line is written as its escape (`\u{1b}`, `\u{202e}`; #809): the list can't show other words than what runs.
9. **Global hooks.** "Plans already exist" is read as: the configuration being imported into has any plan or any repository. Agent hooks wrap every job on this agent, a repository's check, forget and prune included, so a configuration with only a repository already has jobs they would run around. Then the profile's `[global.hooks]` are left out with `profile-global-hooks-left-out`, and are not listed. In an empty configuration they become agent hooks, as decision 10 had it, and are listed.
10. **A repository entry kept.** An import under an alias that already holds the profile's repository keeps the entry as it is (§6.2), so the profile's repository commands are not saved and not listed; `profile-setting-kept` names the settings that differ.
11. **The preview.** `POST /api/v1/profiles/preview` and the MCP tool `preview_import` return the same notices, computed against the daemon's configuration. They echo only the text the caller sent (decision 99), and they save and run nothing, so they ask nothing.

## Cost

- A scripted import of a profile with hooks or a `password-command` now fails until it passes `--yes`.
- A profile's global hooks no longer reach a configuration that has anything in it: the person adds them to the agent's `[hooks]` by hand, after reading them.
- The y/N answer at a real terminal has no automated test: the e2e runner has no terminal. The same code asks before `pair`.

## Rejected

- **Folding the global hooks into the imported repository's hooks:** they would run around that repository's jobs only, which is not what rustic does with them, and the person would not find them where the profile put them.
- **Refusing a profile with global hooks** when the configuration has plans: it makes the person edit the profile to import the rest.
- **The program alone in the listing:** `/bin/sh` tells the person nothing about what runs.
- **A second flag of the import's own** (`--trust-commands`): the CLI already has one way to say yes.
