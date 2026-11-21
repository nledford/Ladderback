# API tokens and AI agents

An API token lets a program other than a browser use Ladderback while sign-in is on: an AI agent through `ladderback mcp`, a scheduler, a monitoring script. Each token has its own permissions, and it can do only what they allow. Give a program only what its job needs, and a token of its own, so you can revoke one without stopping the others.

## Making a token

Tokens need sign-in on (see [Sign-in and remote access](remote-access.md)). In Settings, under "API tokens", give the token a label that says which program uses it, choose its permissions, and select **Create the token**. Ladderback shows the token once: copy it then, because it can't show it again.

A program sends the token as `Authorization: Bearer` and the token's text. For `ladderback mcp --connect`, save it in a file only you can read (`chmod 600`) and name it with `--token-file`, or save it as `mcp-token` in Ladderback's configuration folder, where the server looks when no file is named.

## The permissions

Each permission is No access or the one thing it allows:

| Permission | Allows |
|---|---|
| Backups | See plans, every repository, the list of snapshots, operations and their progress, and status. Not the files inside a snapshot, and not the log |
| Backup runs | Start a plan's backup, cancel a backup, and test a saved repository |
| Previews | Ask what forgetting or pruning snapshots, checking a repository, adopting snapshots into another plan or importing a profile would do, without doing it |
| Configuration | Propose changes to plans, which you approve on the Pending changes page, and try a plan in a dry run |
| Snapshot contents | See the names of the files and folders inside snapshots, and those a warning or a failure names |
| Daemon log | Read Ladderback's log, which can name your files and folders |
| Repository check | Nothing yet: it keeps a token you make today from starting the repository checks a later version adds |

A new token starts with Backups, Backup runs, Previews and Configuration. Snapshot contents and Daemon log stay off until you choose them: they show the names of your files, which a program, or the model behind an AI agent, then reads. No permission restores, downloads, deletes snapshots or approves a change: those stay yours, in the web UI.

## Which permissions for which job

- **An AI agent that tells you how your backups are doing:** Backups. Through `ladderback mcp` it can read status, plans, snapshots and operations, and nothing else.
- **An AI agent that finds a file in your snapshots:** Backups and Snapshot contents. Snapshot contents opens the tools that list a snapshot's folder and search snapshots for a name; Backups lets it find the repositories and snapshots to look in.
- **An AI agent that helps you set up plans:** the defaults. It proposes plans and tries them in a dry run, and you approve each change in the web UI.
- **A scheduler that only starts backups:** Backup runs alone. It starts a plan's backup by the plan's ID and reads nothing, so a leaked token tells nobody what you back up.
- **A monitoring script:** Backups, to read status and follow operations as they run. Add Daemon log only if it collects the log.
- **Troubleshooting with an AI agent:** Backups and Daemon log. It reads status, plans and operations to find what failed, and the log for what Ladderback recorded around it. Without Snapshot contents, a failure shows its code but not the file names in it. The log can name your files and folders, so turn Daemon log off again when you're done.
- **A read-only dashboard:** Backups alone, the same as a monitoring script: a page that shows your plans, snapshots and status can't start, change or propose anything, so a leaked token shows what you back up but changes none of it.

`ladderback mcp` offers an AI agent only the tools its permissions open, so an AI agent doesn't try what it can't do, and Ladderback refuses anything else it asks for all the same.

## Changing what a token may do

Select **Change permissions of token** and the token's label on its row, choose again, and save with the admin password. A program using the token gets the new permissions from its next request. A token can never change its own permissions, and setting a new admin password revokes every token.

## Revoking a token

On the "API tokens" tab of Settings, select **Revoke token** and the token's label on its row, then **Revoke the token**. A program using the token is refused from its next request with `401 api-token-invalid`, and a live-updates stream it holds ends within a few seconds. The token can't be brought back: to let the program in again, create a new token and give it to the program. Setting a new admin password revokes every token at once.

Stop the program or give it a new token: each refused request counts as a failed sign-in from its address, so it soon gets `429 sign-in-wait`, and signing in from that address, in a browser too, has to wait as well.

## When a program is refused

A program asking for something its permissions don't open is refused with one of two codes. Over HTTP the answer is `403 Forbidden` with the code and the permission it needs, such as `{"code": "api-token-permission", "args": {"permission": {"type": "text", "value": "backups"}}}`. Through `ladderback mcp` the tool call fails with the same code, then a sentence that says what to do.

- **`api-token-permission`:** the program's token lacks the permission named. On the "API tokens" tab of Settings, select **Change permissions of token** and the label on that token's row, and add the permission. When it names `session`, no token can do it: only a person signed in through a browser can. An AI agent without a token that tries to cancel anything but a backup gets this code too, naming `session`.
- **`without-token-permission`:** an AI agent without a token asked for something the "AI agents" tab doesn't allow. Turn the permission named on there, in Settings (it saves to the `[without-token]` table of the configuration file), or, after turning sign-in on, give the AI agent a token that has it. When it names `session`, only a person using Ladderback in a browser can do it.

## AI agents without a token

Settings has a tab of its own, "AI agents", for what an AI agent may do through `ladderback mcp` without a token: in a session of its own (`--headless`), and when it connects while sign-in is off. It holds the same permissions, with the same defaults, so an AI agent without a token can't look inside your snapshots until you allow Snapshot contents there. With sign-in on, saving a change takes the admin password. A `--scratch` session has a configuration of its own: it always starts from the defaults plus Snapshot contents and Daemon log, since its files and its log are made up.

With sign-in off, these permissions hold only `ladderback mcp`: any other program on the machine, or on your network if Ladderback listens there, can use Ladderback without asking. To limit every program, turn sign-in on and give each one a token.

[Technical design §10.1](https://github.com/nledford/Ladderback/blob/main/docs/design/security.md#101-web-ui) and [§4.6](https://github.com/nledford/Ladderback/blob/main/docs/design/components/mcp.md#46-mcp-server) have the details.
