# Decision 10: Hooks model

**rustic's hook levels exactly** (agent, repository, plan), so hooks import and export faithfully. Notifications are a separate system (§6.1, §11).

**Amended 2026-11-19 (the maintainer, #1135; [decision 131](131-an-import-confirms-the-commands-it-adds.md)):** an imported profile's `[global.hooks]` become agent hooks only when the configuration has no plan and no repository. Otherwise they would also run around jobs the profile never had, so the import leaves them out with a notice, and the person adds them by hand.
