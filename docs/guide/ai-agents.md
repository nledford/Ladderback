# AI agents and the MCP server

`ladderback mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server. An AI agent's client, such as Claude Code, starts it and speaks to it on standard input and output. Through its tools, the AI agent reads Ladderback's status, plans, repositories, snapshots, operations and log, starts a backup and waits for it, previews a forget, and proposes changes to plans for you to approve. `ladderback mcp call` calls the same tools from a shell, without a client.

This page sets it up in common clients, lists its tools, and says what an AI agent can't do with them. [API tokens and AI agents](api-tokens.md) says which permissions to give it.

## What an AI agent can't do

No tool restores or downloads files, forgets or prunes snapshots, adopts snapshots into another plan, edits the configuration, approves a proposal, or pairs a machine with a hub. Each of those needs a person's confirmation, or could change which commands Ladderback runs, so each stays yours, in the web UI or at the terminal. An AI agent cancels only backups, never maintenance.

The most an AI agent does to your configuration is propose a change, which does nothing until you approve it on the Pending changes page (see [Proposals](#proposals)). A backup it starts runs the plan's hooks and secret sources, as any other run of the plan does.

## Three ways to run it

`ladderback mcp` runs in one of three modes, and needs one named:

- **`--connect`** drives Ladderback that is already running, through its API, as the web UI does: the one at `http://127.0.0.1:9898`, or at the address that follows. This is the mode for everyday use. A backup an AI agent starts runs on in Ladderback after the client closes, and only this mode offers the proposal tools, since you approve a proposal in Ladderback's web UI.
- **`--headless`** runs a session of its own on Ladderback's folders, without Ladderback running. It is refused while Ladderback runs, since one process at a time holds the data folder. Operations run only while the session lasts: when the client closes it, a backup still running stops, and one queued waits for Ladderback's next start.
- **`--scratch`** runs a session of its own in a new private folder, seeded with a plan over a few generated files, and deletes the folder when the session ends: for trying the tools (see [Trying it on made-up files](#trying-it-on-made-up-files)).

## Tokens and permissions

With sign-in on, `--connect` needs an API token with the permissions the AI agent's job needs: [Making a token](api-tokens.md#making-a-token) says how to make one and where `ladderback mcp` reads it from. It refuses to start with a token file other users can read, and sends a token only to an `https` address or to this computer.

Without a token, an AI agent that connects while sign-in is off, or runs in a headless session, may do what the "AI agents" tab of Settings allows; a scratch session has permissions of its own. [AI agents without a token](api-tokens.md#ai-agents-without-a-token) says what each holds.

The server lists to an AI agent only the tools its permissions open, as the [list of tools](#the-tools) says. A call beyond them is refused with `api-token-permission` or `without-token-permission`, as [When a program is refused](api-tokens.md#when-a-program-is-refused) explains.

## Setting up a client

A client starts `ladderback mcp` itself, from the command and arguments you give it. The examples connect to Ladderback running on this computer with sign-in off. With sign-in on, add `--token-file` and the token file's path after `--connect`, or save the token as `mcp-token` as above. For a session of its own, write `--headless` or `--scratch` in place of `--connect`.

A client started from the Dock or a desktop menu may not search the folders your shell adds to its path. If it says it can't find `ladderback`, give the full path that `command -v ladderback` prints in place of `ladderback`.

### Claude Code

```sh
claude mcp add --scope user ladderback -- ladderback mcp --connect
```

`--scope user` offers Ladderback in every project; leave it out to add it to the current project alone. Everything after `--` is the command Claude Code runs, so it doesn't read `--connect` as an option of its own. `claude mcp list` then says `✔ Connected` beside `ladderback`.

### Claude Desktop

In Claude Desktop's Settings, on the Developer tab, select **Edit Config**. It opens `claude_desktop_config.json`, which is in `~/Library/Application Support/Claude` on macOS. Add Ladderback under `mcpServers`, with the full path `command -v ladderback` prints:

```json
{
  "mcpServers": {
    "ladderback": {
      "command": "/Users/demo/.cargo/bin/ladderback",
      "args": ["mcp", "--connect"]
    }
  }
}
```

Quit Claude Desktop completely and open it again: it reads the file when it starts.

### Gemini CLI

```sh
gemini mcp add --scope user ladderback ladderback -- mcp --connect
```

The first `ladderback` names the server, the second is the command, and the arguments follow `--`. The command writes the same `mcpServers` entry as Claude Desktop's into `~/.gemini/settings.json`, which you can also edit by hand. Gemini CLI starts servers only in folders you trust, so `gemini mcp list` says `Disabled` elsewhere; in a trusted folder it says `Connected` beside `ladderback`.

### Other clients

A client that starts a server on standard input and output takes the same: the command `ladderback`, then the arguments `mcp` and `--connect`. Many take the `mcpServers` entry Claude Desktop's file holds. The server speaks the protocol's revisions 2025-06-18, 2025-11-25 and 2026-07-28; a client that speaks only an earlier one can't use it.

## The tools

Each tool below is described in the words the server gives an AI agent. In parentheses is the permission that opens it, for an API token and for AI agents without one; a tool that names two needs both. A tool marked "running Ladderback only" isn't offered in a session of its own: the proposal tools need the web UI to approve in, and the plan dry run needs a running Ladderback.

<!-- The tools, as the server lists them: crates/adapters/mcp/tests/guide.rs checks this list. -->

- **`status`** (Backups). Ladderback's state: each plan with its last operation, last successful backup and how long ago, whether it is overdue and its next run; the operations queued or running; the repositories that couldn't be read at their last attempt; the Full Disk Access state; this program's build and, in connect mode, the daemon's build, sign-in state and exposure.
- **`list_plans`** (Backups). The configured plans, a page at a time: each with its repository, whether it has a schedule, its next run and its configuration warnings.
- **`list_repositories`** (Backups). The configured repositories, a page at a time: each with its location, credentials masked, where its password comes from, who its entry asks to maintain it, whether it is maintained here and its configuration warnings. Never a secret.
- **`list_snapshots`** (Backups). One repository's snapshots, newest first, a page at a time, read from the repository; only those of a plan, or taken on a host, when either is named.
- **`list_snapshot_folder`** (Snapshot contents). One folder of a snapshot, a page at a time, read from the repository: each entry's name, whether it is a folder, a file, a link or something else, a file's size and when it was last modified, with how many entries the folder holds. Restoring and downloading are done by the person, in the web UI.
- **`find_in_snapshots`** (Snapshot contents). Finds files and folders by name across a repository's newest snapshots, or one plan's: each match's path, and the snapshots that hold it with its size and when it was last modified there. Searches at most 100 snapshots for at most 15 seconds, returns at most 500 matches, and says which cap cut it short. Restoring and downloading are done by the person, in the web UI.
- **`list_operations`** (Backups). The history of operations, newest first, a page at a time, running ones included: each with its status, times, warning codes and, when it failed, why.
- **`get_operation`** (Backups). One operation: its summary, why it failed when it did, and the first 100 warnings it recorded.
- **`read_log`** (Daemon log). The daemon's recent log, newest first, as the web UI's Log page shows it: of a level or worse, and of an operation, a plan or a time range when asked. At most 200 lines and 32 KiB of text a call; stopped says which cap cut the page short, and next reads on from it.
- **`pairing_state`** (Backups). Whether this machine is paired with a hub, as its pin says: the hub's address, the fingerprint of the hub's key and how its certificate is checked. A plain http:// address names no certificate: it isn't encrypted and shows none, and the hub key still proves the hub. Never a token or a secret. It doesn't connect to the hub, so it can't say whether the hub has approved the machine: `ladderback hub-status` on the machine says that. The resource ladderback://pairing holds the steps of pairing.
- **`run_plan`** (Backup runs). Queues a backup of a plan now, under the plan's overlap policy, and returns its operation's ID. Its hooks and secret sources run as for any other run.
- **`wait_operation`** (Backups). Waits until an operation finishes or the timeout passes, then returns whether it finished and the operation as get_operation does; when its warnings are too many to read, none are listed, unlisted-warnings is absent and its warning-codes still name them. Sends progress notifications when the call asks for them. Canceling the call stops the wait, not the operation.
- **`cancel_operation`** (Backups and Backup runs). Asks a queued or running backup to stop, however it started; it then ends as canceled. Maintenance can't be canceled from here.
- **`test_repository`** (Backup runs). Tries each location of a configured repository, as a backup would choose among them, and tells each one's outcome. Records nothing.
- **`preview_forget`** (Previews). What a forget of a repository maintained here would let go, by each plan's retention, and what it leaves alone. Reads the repository; deletes nothing.
- **`preview_import`** (Previews). What importing a rustic profile, given as its TOML text, would add: the repository's name, the plans and what the import couldn't carry over. Saves nothing and reads no file; profiles it uses are named in a notice.
- **`export_profile`** (Backups). A plan as a profile the stock rustic reads, and what the export left out. A password stored in the configuration is refused, never written out; the password command, every option rustic runs as a command and every hook are left out, each named in a notice.
- **`propose_setup`** (Configuration; running Ladderback only). Proposes configuration changes for a person to approve in Ladderback's web UI: plans to add, change or remove, with the reason why. Changes nothing until a person approves; no tool approves. Returns the proposal kept, its ID and what the submission checks warn of. A plan names a repository by its alias, never a secret.
- **`get_proposal`** (Configuration; running Ladderback only). One proposal, by its ID: its status, its changes, its rationale and the reasons a person gave for rejecting any of them. With an API token, only a proposal that token made.
- **`withdraw_proposal`** (Configuration; running Ladderback only). Withdraws a pending proposal, so no person approves it, and returns it. With an API token, only a proposal that token made.
- **`dry_run_plan`** (Configuration; running Ladderback only). Counts what a draft plan would back up, without backing anything up: the files and bytes its sources hold through its source filter, and what each exclude removes on its own. Counts and sizes only, never a path. Reads only within the folders a person allowed on Ladderback's Settings page; runs no hook and opens no repository. One runs at a time, none while a backup runs or waits, each stops after a short time or a number of entries and says so, and each client may start a few an hour.

<!-- The end of the tools. -->

The server also offers two resources, which a client may read before it acts. `ladderback://status` holds what the `status` tool answers. `ladderback://pairing` holds the steps of pairing a machine with its hub, for an AI agent asked how to add a machine; it needs no permission. [Technical design §4.6](https://github.com/nledford/Ladderback/blob/main/docs/design/components/mcp.md#46-mcp-server) has each tool's details and limits.

## Proposals

An AI agent connected to a running Ladderback, with the Configuration permission, may propose adding, changing or removing plans with `propose_setup`, and gives its reason. A proposal can't give a plan hooks or change its hooks, and can't touch repositories or maintenance yet.

Before it proposes, an AI agent can try a draft plan with `dry_run_plan`, which counts the files and bytes the plan would back up, never their names. It reads only within the folders you add on the "Dry-run allowlist" tab of Settings, and refuses every source until you add one.

A proposal changes nothing until you approve it. Each pending proposal is listed on the Pending changes page. Its review shows what each change does, in Ladderback's words rather than the AI agent's, then the AI agent's reason and the change to the configuration file. A proposal that could delete snapshots is approved only after **Show what it would forget** has shown what each plan's keep policy would forget, before and after.

To approve, type the admin password, or, with sign-in off, the proposal's short ID, then select **Approve the proposal**. With sign-in off, approving is a convenience, not a safety boundary: any program that can reach Ladderback could approve its own proposal. Turn sign-in on to make approval yours alone.

To reject a proposal, give a reason for each change you want done another way, and select **Reject the proposal**. The AI agent reads your reasons with `get_proposal`, and can propose again a few minutes later. It can also withdraw a pending proposal with `withdraw_proposal`. A proposal no one decides within 48 hours expires, and one whose changes no longer fit the configuration, after another change, goes stale; neither can be approved then. [Decision 89](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/89-agent-proposed-setup.md) has the details and the limits.

## Calling a tool from a shell

`ladderback mcp call` calls a tool without a client, through the same checks, and prints its answer as one line of JSON:

```sh
ladderback mcp call status
ladderback mcp call list_snapshots repository=nas limit=5
```

Each argument is `NAME=VALUE`: a value is a number, or `true` or `false`, where the tool takes one, and text otherwise. `--args '{"repository": "nas", "limit": 5}'` gives them as one JSON object instead. With no mode named, it connects to Ladderback at `http://127.0.0.1:9898`; `--connect` with an address, `--token-file`, `--headless` and `--scratch` work as they do for `ladderback mcp`.

A call that worked prints `{"tool": …, "ok": true, "result": …}`, the result being what an AI agent gets. A refused call prints `{"tool": …, "ok": false, "error": …}`, with the refusal's code and its arguments, and says what to do next on standard error. The exit status is 0 when every call worked, 1 when a call failed or the session couldn't start, and 2 when the command line can't be read, such as a tool that doesn't exist; then nothing is called.

A backup started this way runs on in Ladderback, and `wait_operation` waits for it, 600 seconds at most a call. With [jq](https://jqlang.org):

```sh
operation=$(ladderback mcp call run_plan plan=home | jq -r .result.operation)
ladderback mcp call wait_operation operation="$operation" timeout=600
```

`--script` and a file makes the calls the file holds, one JSON object to a line, in one session, and stops at the first refused call unless `--keep-going` is given. `--script -` reads them from standard input, each answered before the next is read, so a program writing them can wait for the operation a run returned:

```json
{"tool": "status"}
{"tool": "list_plans", "arguments": {"limit": 10}}
```

A session of its own ends with its last call, and a backup still running ends with it. So `run_plan` given alone with `--headless` or `--scratch` is refused, with exit status 2, before anything runs: give it in a script followed by `wait_operation`, or call a running Ladderback.

## Trying it on made-up files

`--scratch` gives an AI agent, or you, a Ladderback to try that holds none of your files. Each session starts in a new private folder in the temporary folder, holding a configuration, a repository, and one plan, `scratch`, over a few generated files, with no snapshots until a run. The folder and everything in it are deleted when the session ends. A client that stops the server without closing it first can leave the folder behind; the next scratch session deletes it.

```sh
ladderback mcp call --scratch list_plans
claude mcp add ladderback-scratch -- ladderback mcp --scratch
```

It offers no proposal tools, which need the web UI to approve in, and no plan dry run, which needs a running Ladderback.
