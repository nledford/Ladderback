# API tokens and AI agents

An API token lets a program other than a browser use Ladderback while sign-in is on: an AI agent through `ladderback mcp`, a scheduler, a monitoring script. Each token has its own permissions, and it can do only what they allow. Give a program only what its job needs, and a token of its own, so you can revoke one without stopping the others.

## Making a token

Tokens need sign-in on (see [Sign-in and remote access](remote-access.md)). In Settings, under "API tokens", give the token a label that says which program uses it, choose its permissions, and select **Create the token**. Ladderback shows the token once: copy it then, because it can't show it again.

A program sends the token as `Authorization: Bearer` and the token's text. For `ladderback mcp --connect`, save it in a file only you can read (`chmod 600`) and name it with `--token-file`, or save it as `mcp-token` in Ladderback's configuration folder, where the server looks when no file is named.

## The permissions

Each permission is No access or the one thing it allows:

| Permission | Allows |
|---|---|
| Backups | See plans, repositories, the list of snapshots, operations and their progress, and status. Not the files inside a snapshot, and not the log |
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

`ladderback mcp` offers an AI agent only the tools its permissions open, so an agent doesn't try what it can't do, and Ladderback refuses anything else it asks for all the same.

## Changing what a token may do

Select **Change permissions of token** and the token's label on its row, choose again, and save with the admin password. A program using the token gets the new permissions from its next request. A token can never change its own permissions, and setting a new admin password revokes every token.

## AI agents without a token

Settings has a tab of its own, "AI agents", for what an AI agent may do through `ladderback mcp` without a token: in a session of its own (`--headless`), and when it connects while sign-in is off. It holds the same permissions, with the same defaults, so an AI agent without a token can't look inside your snapshots until you allow Snapshot contents there. With sign-in on, saving a change takes the admin password. A `--scratch` session has a configuration of its own: it always starts from the defaults plus Snapshot contents, since its files are made up.

With sign-in off, these permissions hold only `ladderback mcp`: any other program on the machine, or on your network if Ladderback listens there, can use Ladderback without asking. To limit every program, turn sign-in on and give each one a token.

[Technical design §10.1](https://github.com/nledford/Ladderback/blob/main/docs/design/security.md#101-web-ui) and [§4.6](https://github.com/nledford/Ladderback/blob/main/docs/design/components/mcp.md#46-mcp-server) have the details.
