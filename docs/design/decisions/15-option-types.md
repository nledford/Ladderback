# Decision 15: Option types

**Ladderback owns its option types** in `domain`. The `config` adapter parses TOML with rustic's keys into them; the engine adapter maps them onto `rustic_core` types, with a parity test per struct. Replaces v0.2's plan to deserialize into `rustic_core` types directly (§6.1).
