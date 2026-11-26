# Decision 97: Red-first in every engine

**The red-first check runs each new `@ui` scenario in Chromium, Firefox and WebKit. Chromium stays the primary engine; Firefox and WebKit are for what is specific to them. A scenario that fails in Chromium is red, as before. One that fails only in Firefox or WebKit is red only if it passes in that engine at the pull request's head, run with the head's step code. The verdict's rules have `node --test` unit tests that `just lint` runs** (recorded 2026-11-15, §13.1, §13.2; decided by the maintainer on #631, option A).

## Why

- **A bug one engine shows can be proven red.** #631's Firefox forced-colors focus ring and WebKit's sideways scroll at 320 px fail in no other engine, so a Chromium-only check would call their scenarios `GREEN` and no `behaviour` change could fix them.
- **Chromium stays primary.** CI's required `UI journeys` job runs Chromium alone, so a Chromium red keeps its meaning: the required job proves the scenario passes at the head. No required job runs Firefox or WebKit; a step adapts to them by branching on `engine`, never by skipping (§13.2).
- **The head check keeps the check strict.** No required job runs Firefox or WebKit at the head. Without the head run, a scenario Chromium passes, failing in Firefox for a reason the branch never fixes, would pass for red, and the required check would loosen.
- **The head's step code.** The head run takes `features/` and `ui-tests/` from the head. Freezing them at the `test:` commit would fail heads that legitimately change a step; a step changed later still has to pass the required Chromium job.
- **Tooling has unit tests.** The verdict lives in `scripts/red-first-ui.ts`, tested by `node --test` in `just lint`. Decision 79 rejected `node:test` only for behavior tests, which red-first and `just test` can't see; tooling logic is not behavior, and lint is where its tests run.

## Rejected

- **Freezing the step code at the `test:` commit for the head run:** fails heads that legitimately change steps.
- **Red in any engine:** a failure only Firefox or WebKit shows, never fixed, would count as red, a looser check than Chromium alone.

Known limit: a one-off Firefox or WebKit failure that passes at the head counts as red, as a one-off Chromium failure already does.
