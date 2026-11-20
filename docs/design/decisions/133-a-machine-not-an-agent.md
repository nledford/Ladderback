# Decision 133: A machine running Ladderback is a machine, not an agent

**A computer, virtual machine or container that runs `ladderback serve` is a machine, in the UI, the CLI, the user guide and the glossary; one paired with a hub is a paired machine, and the hub is the machine that runs with `--hub`. "Agent" alone is no longer a Ladderback term: a language-model program acting through MCP or the API stays an AI agent. The `[agent]` table of `config.toml` keeps its name. Code identifiers, the technical design's prose and the scenarios move to the new word in one mechanical change once the open hub work has merged** (recorded 2026-11-20, §3.1; the maintainer's decision).

## Why

Ladderback had two kinds of agent: the machine that runs backups, which pairs with a hub, and the AI agent that acts through MCP or the API. The glossary kept them apart by never writing "agent" alone for the AI, but a reader never sees the glossary. A person with one machine and no hub read "this agent" in a refusal, beside an AI agents tab and a Pending changes page for what AI agents propose, and the likelier reading was the AI. macOS added a third: the CLI's help installed "this agent" as "a launchd agent".

The machine's word is the one to change. "AI agent" is the industry's word, the one people search for and MCP clients use of themselves; the machine's word is Ladderback's own, and the hub, which most of its uses describe, is not released yet.

## Decided

1. **The word.** "Machine" for one running Ladderback, whatever it runs on; "paired machine" where the pairing matters, as on a hub's list; "the hub" for the machine that runs with `--hub`, which is a machine too and keeps its own plans and repositories. The machine's ID and key are the machine ID and the machine key. A Fluent term, `-machine`, holds the word with its plural, replacing `-agent`, and each message that used it is reread, since the article and the sentence can change with the word.
2. **Not "client".** The paired machine dials out to the hub, but "client" already names what talks to Ladderback: the browser, an MCP client, an API client, and so the AI agent itself. A machine with no hub serves the web UI, the API and MCP, so it is a server to them; and the hub, which is a machine too, would be a client of itself. "Instance", "node" and "device" were weighed and read as jargon or as a phone.
3. **What keeps its name.** What a person types or a program reads keeps its name where users already have it: the `[agent]` table of `config.toml` (`schema = 1`), which the guide describes as this machine's settings. "Launchd agent" stays, as Apple's name for what it is. Names the hub alone uses, in the sync protocol, the API and the data folder, are renamed or kept in the mechanical change, before the hub's first release.
4. **The order.** The glossary, the catalogs and the user guide change first, in one docs-and-catalog change, so every message from then on says machine. Code identifiers (`AgentId`, the agent registry, `pairing/agent.rs`), the technical design's prose and the scenarios' step text follow in one mechanical change after the open hub pull requests merge, so it conflicts with none (#1220). Until then the glossary's row says that older text and code call a machine an agent.

## What it costs

Two words for one thing for a while: "machine" in what a person reads, "agent" in code, older design text and scenarios, until the mechanical change lands. A reader of the design meets both, and the glossary row is what joins them.

"Machine" is loose for a container, which isn't one. A person who runs Ladderback in Docker is used to that use of the word, and the glossary says "whatever it runs on".
