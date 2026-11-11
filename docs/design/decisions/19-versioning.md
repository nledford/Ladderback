# Decision 19: Versioning

**One SemVer version for the whole product**, judged against a declared contract (CLI, config schema, `/api/v1`, `/sync/v1`, snapshot tags, webhook JSON, exported profiles, data migrations). `0.1.0` at the end of M1b, `1.0.0` after M5; `vX.Y.Z` tags and `CHANGELOG.md` produced by release-plz from Conventional Commits through a reviewed release PR (§12.1).
