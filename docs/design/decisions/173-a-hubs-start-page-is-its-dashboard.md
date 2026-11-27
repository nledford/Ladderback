# Decision 173: A hub's start page is its dashboard

**On a running hub, the start page (`/`), headed and titled "Dashboard", is the dashboard of [decision 149](149-the-hubs-dashboard.md): the start page's notices first, as on any daemon, then the dashboard's introduction, "Pause live updates" and a section for each approved machine. The product name in the banner, the start page's link, therefore leads to it, and the Machines tab's link "See the approved machines' latest operations" goes to `/`. The hub's own plans and operations stay on its Plans and Operations pages, which the header links on every daemon. `/dashboard` sends a hub's browser to `/` in place of itself; on a daemon that isn't a hub it is a page not found, titled so. A daemon that isn't a hub keeps its start page as it was** (recorded 2026-11-26, §4.4, #1484; the maintainer's decision).

## Why

The maintainer, having paired the first real machine with the first real hub, looked for the dashboard in the header and couldn't find it: the start page's link to it, after the notices, was easy to miss, and the header has no link of its own (decision 149, "Where it is reached"). Decision 149's reasons against a header link stand: the header's links are the same on every daemon, and its line has no room for another in a pseudolocale. The start page is the one page every daemon has a header link to, so on a hub it becomes the dashboard, the hub's main page, without a link that only a hub shows.

## Decided

1. **What a hub's start page shows.** Its heading and title are the dashboard's ("Dashboard", "Dashboard – Ladderback"). The start page's notices come first, in the same order as on any daemon: sign-in off on the network, the Full Disk Access steps, a copied data folder or a machine its hub refused or removed. Then the dashboard as decision 149 has it: its introduction, "Pause live updates", and each approved machine's section (online now or when it last connected, the reports let go of, progress, latest operations, plans), or the sentence that none is approved yet.
2. **The hub's own plans and operations.** The table of the hub's plans, the ways to the Repositories and Plans pages while there are none, the way to the Operations page and the link to the plan format leave a hub's start page. The hub's Plans page lists its plans with their next runs and its Operations page its runs, as on any machine, and the header reaches both from every page. Repeating them under the machines would make the page two summaries of different things; the dashboard is of the machines the hub serves.
3. **Until the page knows.** The page shows nothing under the header until it knows whether the daemon is a hub (`settings::agents::hub_known`, the read Settings' Machines tab already makes), so a hub's start page never shows the welcome first and then changes its heading. The title is the start page's until then, and the dashboard's once the read says hub.
4. **`/dashboard`.** On a hub it navigates to `/`, replacing its own entry in the browser's history, so a bookmark from 0.6.0-rc.1 still arrives and Back doesn't return to it. Keeping it as a second address of the same page would leave two titles and an address where the banner's link isn't current; a page not found would break those bookmarks for no gain. On a daemon that isn't a hub it stays a page not found, and its title says so (before, it was titled "Dashboard" there).
5. **The links.** The start page's link to the dashboard is gone: the page is the dashboard. The Machines tab keeps its link, under its words, to `/`.

## Considered and rejected

- **A header link shown only on a hub.** Decision 149's reasons: the first header link that depends on the daemon's mode, on a line already full in a pseudolocale.
- **The hub's own plans table below the machines.** A second, different summary on the calm page; the Plans page already shows it.
- **A redirect done by the daemon.** The web UI's router owns its paths; the daemon serves the same page for each, and an HTTP redirect would be a second place that knows the UI's addresses.

## Known limits

- On a page load, the start page waits for the hub's read before it shows anything, as `/dashboard` did; on a daemon that isn't a hub, the read is answered at once with `route-unknown`.
- A hub whose read of its key goes unanswered within 5 seconds, or fails, counts as no hub for that standing, as its Machines tab already did: its start page is then the welcome and its `/dashboard` a page not found until the page is loaded again.
- Until the hub's read answers, `/dashboard` is titled "Page not found" on a hub too, and `/` "Ladderback"; each takes its title once the read answers.
- The redirect from `/dashboard` keeps neither its query nor its fragment: `/dashboard?locale=en-XA` arrives at `/`, still shown in en-XA until the page is loaded again.
- A header link followed before the hub's read answers on a page load is taken as the page load, so focus stays where it was rather than moving to the new page's heading; on loopback the window is milliseconds.

**Amended by [decision 175](175-a-card-per-machine-on-the-hubs-dashboard.md)** (2026-11-27, #1675): item 1's section per approved machine is a card per machine, and the hub's own machine is among them, its card first. Item 2, and its rejected alternative "The hub's own plans table below the machines", hold no longer as they stood: the hub's own card is the same kind of summary as every other card, so the page stays one summary, and its plans and operations in full are on its machine page, as each machine's are on its own. Its Plans and Operations pages are as they were. The Machines tab's link reads "See the machines on the dashboard".
