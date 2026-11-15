# Decision 99: Who is trusted with Ladderback's local data

**Ladderback runs on a person's own computer, as a hub or an agent, so the person and every program running as their operating-system user are trusted with its local data; the security design defends the network, other users of the machine, a repository's storage and outside text** (recorded 2026-11-15; the rule is "Who is trusted" at the top of §10).

**The maintainer's guidance:** users run Ladderback on their personal computer, whether a hub or a client, so both they and AI tools have access to its files and its SQLite store. Only advanced users and developers use the Justfile's recipes, for development, and they know the risks of contributing and what they're doing.

**Why** (the design's reasoning from that guidance): a defense against a program running as the person would be one it could step around, and rules nobody can enforce cost reviews and design time that the real boundaries need. A review therefore weighs a same-user weakness only by what it reaches beyond that user.
