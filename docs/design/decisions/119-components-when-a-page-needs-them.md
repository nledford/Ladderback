# Decision 119: Build each new shared component just before the first page that needs it

**A shared component that no page uses yet is built in the milestone of the first page that needs it, in its own pull request just before that page, rather than all in M3b ahead of every page; the hub starts now, beside the AI agents milestone and what remains of M3b** (recorded 2026-11-18, §14; decided by the maintainer).

## The rule

- **M3b keeps the moves.** It finishes the components the pages already use, moves the pages onto them (#929, #930, #886, #840, #942, #989), the narrow or full-width page (#933), Settings as tabs (#935), the gallery and docs split (#975, #977) and the move to Tailwind (#931). Then it is released, as `0.5.0` (operations.md §12.1).
- **A new component waits for its page.** The eleven components no page used on 2026-11-18 move to the milestone of the first page planned to need them (the table below). Each issue's milestone says where it went.
- **The component comes first, to the same bar.** When a page needs a component that doesn't exist, the component gets its own pull request first, to the full checklist of development.md §13.7.5 (gallery entry, axe-core and ARIA snapshot rows, keyboard-only contract scenarios, a contract in §4.5), and the page's pull request stacks on it or follows it. A page never builds a widget of its own (AGENTS.md, Conventions); that rule doesn't change.
- **Three tracks at once.** The hub (M8, then M9), AI agents (M4) and the rest of M3b run in parallel, one agent editing code on each. Scheduled maintenance, notification channels and Japanese (M5 to M7) follow when a track frees up. Releases still follow whichever milestone finishes, in the order it finishes (operations.md §12.1).

| Component | First page planned to need it | Milestone |
|---|---|---|
| Toggle switch (#841), select (#842) | the AI agents tab in Settings, its limits and their defaults (#962) | M4 |
| Indicator (#791), responsive navigation bar (#796), pagination (#795), search field (#843) | the hub's dashboard of every paired machine (#569) and the offline alert (#570) | M8 |
| Popover (#784), dropdown menu (#786), button group (#783) | run and cancel from the hub (#576), restore from the hub (#575) | M9 |
| Date and time picker (#797), file input (#844) | the charts' time range (#579); no page planned for a file input yet | M10 |

The table is a forecast: when a page is designed and needs a different component, that component moves to the page's milestone in the same way.

## Why

- **The hub is the product's key feature** (the maintainer), and most of its first milestone, pairing and the sync protocol, has no UI at all. Holding it behind eleven components no page uses would delay it by weeks for nothing.
- **A component is better designed beside its page.** Built ahead, a component's states and its contract are guesses; built for a page, they answer that page's real data and keyboard journey, and the next page reuses them.
- **The reason M3b was added still holds.** M3b was put before the hub so its pages are built on finished components (2026-11-17). Each hub page still is: on the components it needs, finished to the same bar before it, rather than on all of them.

## Cost

- **A page's pull request can wait on its component's.** The pair is at most two pull requests stacked (development.md §13.8), or the component lands first on its own.
- **Schedule pressure on a component.** One built just before its page is built under pressure. The checklist of §13.7.5 and the adversarial review are the protection, and neither bends for it.
- **M5 to M7 move later.** The hub jumps ahead of scheduled maintenance, notification channels and Japanese, which wait for a free track.
