---
version: 1
slug: "crates-web-src-main-rs"
primary_target: "crates/web/src/main.rs"
related_targets: ["crates/web/src","crates/web/style.css"]
---

## Scope and mode

The Ladderback web UI: every page the daemon serves (dashboard, plans, repositories, snapshots and restore, operations, log, settings, the hub's Machines tab) and the component gallery at `/_gallery`. Mode: **Operate**. The visitor completes a task; scanability, consistency and the real usage scene outrank expression.

## Audience, job, task

- Casual users check that every machine is backed up, notice a failure fast, and get files back without fear, mostly in the browser.
- Advanced users arrive from the rustic CLI; they expect the page to say exactly what the command line would, and a command for everything.
- Primary tasks: read the state of every plan and machine at a glance; act on a failure; run, cancel or preview an operation; restore files; approve a machine on the hub.

## Proof and content

Real-looking demo history: `just try --demo` (months of runs, failures and warnings). Components alone: `/_gallery` (needs the `gallery` build). Never a person's real data.

## Constraints

- PRODUCT.md's voice ("calm and exact") and accessibility floor; WCAG 2.2 AA; keyboard-only journeys; 320 px at 200 % text.
- DESIGN.md's North Star "The Ledger" and its rules hold; shared components only, each in `crates/web/src/components/`.
- All text comes from Fluent catalogs; pseudolocales must not break a layout.

## Unresolved decisions

- What an indeterminate run shows to say it is still alive (the 2026-11-20 motion critique).
- Whether a status that changes live gets a one-shot cue for sighted readers.
