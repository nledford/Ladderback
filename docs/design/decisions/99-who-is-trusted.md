# Decision 99: Who is trusted

**The person and every process running as their operating-system user are trusted with Ladderback's local data; the security design defends the network, other users of the machine, the repository's storage and outside text** (recorded 2026-11-15, the maintainer; §10).

Ladderback runs on a person's own computer, whether as a hub or as an agent. Anything running as that person, an AI agent or a development tool included, can already read and write the data folder: `config.toml`, the SQLite store, the admin password's hash and the logs. A defense against such a process would be one it could step around.

**So:**

- **Reviews.** A weakness that needs a same-user process first is not a security finding unless it reaches further than that user could already reach: across the network, to another user of the machine, or into a repository's storage.
- **What stays defended.** The daemon's network surface, other operating-system users of a shared machine, the storage a repository lives on and the services in between, and text from outside that a program acts on (MCP calls, rustic's output). Every route still needs a session or a token while sign-in is on, because a route is reachable beyond the person's own user.
- **I4 is unchanged.** argv and environments are visible to other users, and logs, templates, sync messages and the UI leave the person's control.
- **Development tools.** The Justfile's recipes are for contributors who know the risks. They may read the daemon's local state directly (`just install-restart`, #699), and nothing in the product depends on them.

**Why:** a threat model that counted the person's own processes as attackers would add rules nobody can enforce, and those rules would cost reviews and design time that the real boundaries need.
