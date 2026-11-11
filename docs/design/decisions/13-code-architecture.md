# Decision 13: Code architecture

**Hexagonal (ports and adapters) around a DDD core.** `domain` and `app` are the inner rings; one crate per adapter; ports are `Arc<dyn Port>` with `#[async_trait]`; the dependency rule is enforced by banned-crate and banned-call checks in CI (§3.2).
