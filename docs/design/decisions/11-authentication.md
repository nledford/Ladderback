# Decision 11: Authentication

**Optional, as in Backrest**, with request checks always on and a startup rule that refuses non-loopback binds without auth or a trusted proxy (§10.1).

**Amended by decision 87** (2026-11-13, #503): with auth off, an address beyond loopback is listened on too, with an `--allowed-host` and a warning on stderr and on every page that anyone who can reach it can use it, as Backrest does.
