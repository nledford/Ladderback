# Decision 7: `rustic_core` dependency

**Depend on the crates.io releases** of `rustic_core` and `rustic_backend` (0.14.0 and 0.8.0, the first with `Progress::error`), each move to a new release in a commit of its own (§2.1). Until those releases the workspace pinned one git revision of `main` (`01fe284`), because `Progress::error` was unreleased.
