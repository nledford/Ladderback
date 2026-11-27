// Journey steps over the gallery as a whole (technical design §4.5): each
// demo's axe-core scan and its pinned ARIA snapshot.

import assert from "node:assert/strict";

import AxeBuilder from "@axe-core/playwright";
import { Then } from "@cucumber/cucumber";

import { patienceMs } from "../support/patience.ts";
import type { BrowserWorld } from "../support/world.ts";
import { demoSection } from "../support/gallery.ts";

// WCAG 2.2 A and AA, as in steps/start_page.ts.
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

Then("the {string} demo has no accessibility violations", async function (this: BrowserWorld, component: string) {
  const section = demoSection(this.page, component);
  await section.waitFor({ timeout: patienceMs() });
  // `analyze` takes no argument: this scans the whole gallery page, the demo
  // among it, as it always has.
  const { violations } = await new AxeBuilder({ page: this.page }).withTags(WCAG_TAGS).analyze();
  const summary = violations.map((v) => `${v.id}: ${v.help} (${v.nodes.length} nodes)`);
  assert.deepEqual(summary, [], `${component} demo violations: ${summary.join("; ")}`);
});

Then("the {string} demo matches its ARIA snapshot", async function (this: BrowserWorld, component: string) {
  const pinned = SNAPSHOTS[component];
  assert.ok(pinned !== undefined, `no ARIA snapshot is pinned for the ${component} demo`);
  const snapshot = await demoSection(this.page, component).ariaSnapshot();
  assert.equal(snapshot.trim(), pinned.trim(), `the ${component} demo's ARIA snapshot`);
});

// The pinned structure of each demo (roles and names, not styling), compared
// with locator.ariaSnapshot(): toMatchAriaSnapshot() only works inside a
// Playwright test, and these run under Cucumber. Regenerate a snapshot by
// logging demoSection(page, name).ariaSnapshot() and pasting it here.
const SNAPSHOTS: Record<string, string> = {
  Dialog: `- heading "Dialog" [level=2]
- button "Open the demo dialog"
- button "Open the demo warning"
- button "Turn the demo setting on"`,
  Table: `- heading "Table" [level=2]
- radiogroup "Demo table shows":
  - text: Demo table shows
  - radio "Demo rows" [checked]
  - text: Demo rows
  - radio "Demo loading"
  - text: Demo loading
  - radio "Demo empty"
  - text: Demo empty
  - radio "Demo unreachable"
  - text: Demo unreachable
- button "Columns"
- region "Demo folders":
  - table "Demo folders":
    - caption: Demo folders
    - rowgroup:
      - row "Name Sort by name Size Sort by size Changed Sort by changed Files Sort by files Actions":
        - columnheader "Name Sort by name":
          - text: Name
          - button "Sort by name"
        - columnheader "Size Sort by size":
          - text: Size
          - button "Sort by size"
        - columnheader "Changed Sort by changed":
          - text: Changed
          - button "Sort by changed"
        - columnheader "Files Sort by files":
          - text: Files
          - button "Sort by files"
        - columnheader "Actions"
    - rowgroup:
      - row "photos 4.2 GiB Oct 4, 2026 1,204 Add a file to ⁨photos⁩":
        - rowheader "photos"
        - cell "4.2 GiB"
        - cell "Oct 4, 2026"
        - cell "1,204"
        - cell "Add a file to ⁨photos⁩":
          - button "Add a file to ⁨photos⁩"
      - row "music 12.6 GiB Aug 21, 2026 12 Add a file to ⁨music⁩":
        - rowheader "music"
        - cell "12.6 GiB"
        - cell "Aug 21, 2026"
        - cell "12"
        - cell "Add a file to ⁨music⁩":
          - button "Add a file to ⁨music⁩"
      - row "documents 820 MiB Nov 2, 2026 312 Add a file to ⁨documents⁩":
        - rowheader "documents"
        - cell "820 MiB"
        - cell "Nov 2, 2026"
        - cell "312"
        - cell "Add a file to ⁨documents⁩":
          - button "Add a file to ⁨documents⁩"
      - row "notes 1.5 MiB Nov 12, 2026 12 Add a file to ⁨notes⁩":
        - rowheader "notes"
        - cell "1.5 MiB"
        - cell "Nov 12, 2026"
        - cell "12"
        - cell "Add a file to ⁨notes⁩":
          - button "Add a file to ⁨notes⁩"
    - rowgroup:
      - row "Total 17.6 GiB 1,540":
        - rowheader "Total"
        - cell "17.6 GiB"
        - cell
        - cell "1,540"
        - cell
- region "Demo folders in a narrow frame":
  - table "Demo folders in a narrow frame":
    - caption: Demo folders in a narrow frame
    - rowgroup:
      - row "Name Size Changed Files":
        - columnheader "Name"
        - columnheader "Size"
        - columnheader "Changed"
        - columnheader "Files"
    - rowgroup:
      - row "photos 4.2 GiB Oct 4, 2026 1,204":
        - rowheader "photos"
        - cell "4.2 GiB"
        - cell "Oct 4, 2026"
        - cell "1,204"
      - row "music 12.6 GiB Aug 21, 2026 12":
        - rowheader "music"
        - cell "12.6 GiB"
        - cell "Aug 21, 2026"
        - cell "12"
      - row "documents 820 MiB Nov 2, 2026 312":
        - rowheader "documents"
        - cell "820 MiB"
        - cell "Nov 2, 2026"
        - cell "312"
      - row "notes 1.5 MiB Nov 12, 2026 12":
        - rowheader "notes"
        - cell "1.5 MiB"
        - cell "Nov 12, 2026"
        - cell "12"
- region "Demo folders, right to left":
  - table "Demo folders, right to left":
    - caption: Demo folders, right to left
    - rowgroup:
      - row "Name Size Changed Files":
        - columnheader "Name"
        - columnheader "Size"
        - columnheader "Changed"
        - columnheader "Files"
    - rowgroup:
      - row "photos 4.2 GiB Oct 4, 2026 1,204":
        - rowheader "photos"
        - cell "4.2 GiB"
        - cell "Oct 4, 2026"
        - cell "1,204"
      - row "music 12.6 GiB Aug 21, 2026 12":
        - rowheader "music"
        - cell "12.6 GiB"
        - cell "Aug 21, 2026"
        - cell "12"
      - row "documents 820 MiB Nov 2, 2026 312":
        - rowheader "documents"
        - cell "820 MiB"
        - cell "Nov 2, 2026"
        - cell "312"
      - row "notes 1.5 MiB Nov 12, 2026 12":
        - rowheader "notes"
        - cell "1.5 MiB"
        - cell "Nov 12, 2026"
        - cell "12"`,
  "Text field": `- heading "Text field" [level=2]
- text: Repository name
- paragraph: The name of the repository to check.
- textbox "Repository name"
- paragraph:
  - button "Check the repository name"
- text: Demo folders
- paragraph: One folder or file a line.
- textbox "Demo folders"
- paragraph:
  - button "Check the demo folders"
- text: Demo password
- paragraph: "A demo field: what is typed here goes nowhere."
- textbox "Demo password"
- button "Show password"
- text: Demo copies to keep
- paragraph: A whole number, such as 7.
- textbox "Demo copies to keep"
- text: Demo backup time
- paragraph: When the demo backup starts, such as 02:00.
- textbox "Demo backup time"
- text: Demo plan name
- paragraph: A demo field that must be filled in.
- textbox "Demo plan name"
- text: Demo host
- paragraph: The demo machine's host name.
- paragraph: Enter a host name, such as backup.example.
- textbox "Demo host" [invalid]
- text: Demo snapshot ID
- paragraph: The demo snapshot's short ID.
- paragraph: "Read only: you can select and copy it, but not change it."
- textbox "Demo snapshot ID": 1a2b3c4d
- text: Demo repository location
- paragraph: Set when the demo repository was made.
- paragraph: "Unavailable: it can't be changed now."
- textbox "Demo repository location" [disabled]: /srv/demo
- text: Demo keyring password
- paragraph: Kept in the demo keyring, and changed there.
- paragraph: "Unavailable: it can't be changed now."
- textbox "Demo keyring password" [disabled]
- text: Demo tag at the Small size
- textbox "Demo tag at the Small size"
- button "Add the demo tag at the Small size"
- text: Demo tag at the Medium size
- textbox "Demo tag at the Medium size"
- button "Add the demo tag at the Medium size"
- text: Demo tag at the Large size
- textbox "Demo tag at the Large size"
- button "Add the demo tag at the Large size"
- text: Demo tag at the Extra large size
- textbox "Demo tag at the Extra large size"
- button "Add the demo tag at the Extra large size"`,
  Checkbox: `- heading "Checkbox" [level=2]
- checkbox "Send a demo notification"
- text: Send a demo notification
- paragraph: "A demo choice: turning it on changes nothing."
- checkbox "Keep the demo log" [checked]
- text: Keep the demo log
- checkbox "Accept the demo terms" [invalid]
- text: Accept the demo terms
- paragraph: The demo terms change nothing.
- paragraph: Accept the demo terms to go on.`,
  "Radio group": `- heading "Radio group" [level=2]
- radiogroup "Demo check frequency":
  - text: Demo check frequency
  - paragraph: "A demo choice: choosing one changes nothing."
  - radio "Every day"
  - text: Every day
  - radio "Every week" [checked]
  - text: Every week
  - radio "Every month"
  - text: Every month
- radiogroup "Demo language":
  - text: Demo language
  - radio "English" [checked]
  - text: English
  - radio "日本語"
  - text: 日本語
  - radio "العربية"
  - text: العربية
- radiogroup "Demo backup window" [invalid]:
  - text: Demo backup window
  - paragraph: Choose a demo backup window.
  - radio "Mornings"
  - text: Mornings
  - radio "Evenings"
  - text: Evenings`,
  "Status badge": `- heading "Status badge" [level=2]
- paragraph: Queued Running Succeeded Succeeded with warnings Failed Canceled Empty`,
  Badge: `- heading "Badge" [level=2]
- paragraph: Pending changes ⁨3⁩ Demo tag Kept by the demo plan's retention rules
- paragraph: Primary Secondary Success Warning Error`,
  Progress: `- heading "Progress" [level=2]
- progressbar "Demo backup progress": Backing up… 40%
- paragraph:
  - button "Advance the demo progress"
- progressbar "Demo progress of unknown size":
  - text: Working…
  - time: Last report at ⁨11/9/26, 10:01:00 AM⁩
- progressbar "Demo progress of a step": Reading the index…
- progressbar "Demo progress at the Small size": Backing up… 60%
- progressbar "Demo progress at the Medium size": Backing up… 60%
- progressbar "Demo progress at the Large size": Backing up… 60%
- progressbar "Demo progress at the Extra large size": Backing up… 60%
- progressbar "Demo progress in the Primary tone": Backing up… 60%
- progressbar "Demo progress in the Secondary tone": Backing up… 60%
- progressbar "Demo progress in the Success tone": Backing up… 60%
- progressbar "Demo progress in the Warning tone": Backing up… 60%
- progressbar "Demo progress in the Error tone": Backing up… 60%
- progressbar "Demo progress in the Inherit tone": Backing up… 60%`,
  Toast: `- heading "Toast" [level=2]
- paragraph:
  - button "Save the demo settings"
- paragraph:
  - button "Queue a demo check"
- paragraph:
  - button "Back up the demo plan"
- paragraph:
  - button "Restore the demo snapshot"`,
  "Review dialog": `- heading "Review dialog" [level=2]
- paragraph:
  - button "Open a demo review"
- paragraph:
  - button "Open an empty demo review"
- paragraph:
  - button "Open a demo secret"
- paragraph:
  - button "Open a held demo review"`,
  "Pause control": `- heading "Pause control" [level=2]
- button "Pause the demo updates"
- paragraph:
  - button "Finish a demo run"`,
  "Live table": `- heading "Live table" [level=2]
- paragraph:
  - button "Change a demo status"
- region "Demo jobs":
  - table "Demo jobs":
    - caption: Demo jobs
    - rowgroup:
      - row "Job Status Actions Guide":
        - columnheader "Job"
        - columnheader "Status"
        - columnheader "Actions"
        - columnheader "Guide"
    - rowgroup:
      - row "photos Waiting Star ⁨photos⁩ rustic's guide for ⁨photos⁩ (goes to another site)":
        - rowheader "photos"
        - cell "Waiting"
        - cell "Star ⁨photos⁩":
          - button "Star ⁨photos⁩"
        - cell "rustic's guide for ⁨photos⁩ (goes to another site)":
          - link "rustic's guide for ⁨photos⁩ (goes to another site)":
            - /url: https://rustic.cli.rs/
      - row "music Waiting Star ⁨music⁩ rustic's guide for ⁨music⁩ (goes to another site)":
        - rowheader "music"
        - cell "Waiting"
        - cell "Star ⁨music⁩":
          - button "Star ⁨music⁩"
        - cell "rustic's guide for ⁨music⁩ (goes to another site)":
          - link "rustic's guide for ⁨music⁩ (goes to another site)":
            - /url: https://rustic.cli.rs/
      - row "documents Waiting Star ⁨documents⁩ rustic's guide for ⁨documents⁩ (goes to another site)":
        - rowheader "documents"
        - cell "Waiting"
        - cell "Star ⁨documents⁩":
          - button "Star ⁨documents⁩"
        - cell "rustic's guide for ⁨documents⁩ (goes to another site)":
          - link "rustic's guide for ⁨documents⁩ (goes to another site)":
            - /url: https://rustic.cli.rs/`,
  Tree: `- heading "Tree" [level=2]
- tree "Demo snapshot":
  - treeitem "documents" [level=1] [selected]
  - treeitem "empty" [level=1]
  - treeitem "many" [level=1]
  - treeitem "slow" [level=1]
  - treeitem "unreadable" [level=1]
  - treeitem "latest" [level=1]
  - treeitem "pipe" [level=1]
  - treeitem "abc[U+202E]txt.exe" [level=1]
  - treeitem "הערות.txt" [level=1]
  - treeitem "readme.txt" [level=1]`,
  Tabs: `- heading "Tabs" [level=2]
- heading "Demo repository" [level=3]
- tablist "Demo repository":
  - tab "Demo overview" [selected]
  - tab "Demo note"
  - tab "Demo history"
- tabpanel "Demo overview":
  - paragraph: The demo repository holds 12 snapshots of two demo plans.
- paragraph: These demo tabs are chosen with Enter or Space. The arrow keys only move between them.
- tablist "Demo machine":
  - tab "Demo log" [selected]
  - tab "Demo stats"
  - tab "Demo plans"
- tabpanel "Demo log":
  - paragraph: A demo panel that a page would read from the daemon once its tab is chosen.
- paragraph: "Chosen demo tab: \u2068Demo log\u2069"
- tablist "Demo tabs, right to left":
  - tab "First demo tab" [selected]
  - tab "Second demo tab"
  - tab "Third demo tab"
- tabpanel "First demo tab":
  - paragraph: The first demo panel.`,
  "Vertical tabs": `- heading "Vertical tabs" [level=2]
- tablist "Demo plan":
  - tab "Demo sources" [selected]
  - tab "Demo times"
  - tab "Demo hooks"
- tabpanel "Demo sources":
  - paragraph: The demo plan backs up two demo folders.
- tablist "Demo plan, right to left":
  - tab "First vertical tab" [selected]
  - tab "Second vertical tab"
  - tab "Third vertical tab"
- tabpanel "First vertical tab":
  - paragraph: The first vertical panel.`,
  "Responsive tabs": `- heading "Responsive tabs" [level=2]
- tablist "Demo settings":
  - tab "Demo general" [selected]
  - tab "Demo address"
  - tab "Demo about"
- tabpanel "Demo general":
  - paragraph: The demo settings shown here change nothing.
- tablist "Demo settings in a narrow column, right to left":
  - tab "First column tab" [selected]
  - tab "Second column tab"
  - tab "Third column tab"
- tabpanel "First column tab":
  - paragraph: The first column panel.`,
  "Tabs starting on a marked tab": `- heading "Tabs starting on a marked tab" [level=2]
- tablist "Demo snapshot":
  - tab "Demo summary"
  - tab "Demo files" [selected]
  - tab "Demo tags"
- tabpanel "Demo files":
  - paragraph: These demo tabs start on this one, which the gallery marks.`,
  "Shared vocabulary": `- heading "Shared vocabulary" [level=2]
- heading "Tones" [level=3]
- list "Tones":
  - listitem: Primary
  - listitem: Secondary
  - listitem: Success
  - listitem: Warning
  - listitem: Error
  - listitem: Inherit
- heading "Status pairs" [level=3]
- list "Status pairs":
  - listitem: Success
  - listitem: Warning
  - listitem: Error
- heading "Sizes" [level=3]
- list "Sizes":
  - listitem: Small
  - listitem: Medium
  - listitem: Large
  - listitem: Extra large
- heading "Placements" [level=3]
- list "Placements":
  - listitem: Inline
  - listitem: Start
  - listitem: Center
  - listitem: End`,
  Spinner: `- heading "Spinner" [level=2]
- paragraph: "Step 2 of 3: Loading the demo snapshots…"
- paragraph:
  - img "Demo spinner at the Small size"
  - img "Demo spinner at the Medium size"
  - img "Demo spinner at the Large size"
  - img "Demo spinner at the Extra large size"
- paragraph:
  - img "Demo spinner in the Primary tone"
  - img "Demo spinner in the Secondary tone"
  - img "Demo spinner in the Success tone"
  - img "Demo spinner in the Warning tone"
  - img "Demo spinner in the Error tone"
  - img "Demo spinner in the Inherit tone"
- text: Loading at the start of the line… Loading in the middle of the line… Loading at the end of the line…
- paragraph:
  - button "Save the demo notes" [disabled]`,
  Icon: `- heading "Icon" [level=2]
- paragraph: The next demo backup starts at 02:00 every night.
- paragraph:
  - img "Demo icon at the Small size"
  - img "Demo icon at the Medium size"
  - img "Demo icon at the Large size"
  - img "Demo icon at the Extra large size"
- paragraph: Primary Secondary Success Warning Error Inherit
- paragraph:
  - button "Demo notifications"`,
  Button: `- heading "Button" [level=2]
- paragraph:
  - button "Filled Primary"
  - button "Filled Secondary"
  - button "Filled Success"
  - button "Filled Warning"
  - button "Filled Error"
- paragraph:
  - button "Outlined Primary"
  - button "Outlined Secondary"
  - button "Outlined Success"
  - button "Outlined Warning"
  - button "Outlined Error"
  - button "Outlined Inherit"
- paragraph:
  - button "Ghost Primary"
  - button "Ghost Secondary"
  - button "Ghost Success"
  - button "Ghost Warning"
  - button "Ghost Error"
  - button "Ghost Inherit"
- paragraph:
  - button "Demo button at the Small size"
  - button "Demo button at the Medium size"
  - button "Demo button at the Large size"
  - button "Demo button at the Extra large size"
- paragraph:
  - button "Schedule a demo backup"
  - button "Demo details at the Small size"
  - button "Demo details at the Medium size"
  - button "Demo details at the Large size"
  - button "Demo details at the Extra large size"
- paragraph:
  - button "Demo work in progress"
  - button "Save the demo plan"
  - button "Check the demo repository"
  - button "Refresh the demo"
- paragraph:
  - button "Delete the demo snapshot" [disabled]
  - button "Show the demo's hidden files"
- paragraph:
  - button "Send the demo form"
- paragraph: The demo buttons haven't been pressed yet.`,
  Tooltip: `- heading "Tooltip" [level=2]
- paragraph:
  - button "Prune the demo repository"
- paragraph:
  - button "Demo schedule"
- paragraph:
  - button "Demo note above"
  - button "Demo note below"
  - button "Demo note at the start"
  - button "Demo note at the end"
- paragraph:
  - button "Demo note at the start, right to left"
  - button "Demo note at the end, right to left"
- paragraph:
  - button "Demo long note"
- paragraph:
  - button "Demo note at the foot of the page"`,
  // A link inside a sentence stands between the isolation marks (U+2068,
  // U+2069) the catalog puts around its words, which stay in the text
  // beside it.
  Link: `- heading "Link" [level=2]
- paragraph:
  - link "Demo link to the start page":
    - /url: /
- paragraph:
  - link "Demo link to this page":
    - /url: /_gallery
- paragraph:
  - link "Demo rustic documentation (goes to another site)":
    - /url: https://rustic.cli.rs/
- paragraph:
  - text: Read \u2068
  - link "the demo plan format":
    - /url: /plans
  - text: \u2069 before you write a demo plan.
- paragraph:
  - text: "The demo check failed: \u2068"
  - link "the demo log":
    - /url: /log
  - text: \u2069 says why.
- paragraph:
  - link "Demo rustic documentation, right to left (goes to another site)":
    - /url: https://rustic.cli.rs/
- list:
  - listitem:
    - link "Demo standing link to this page":
      - /url: /_gallery
  - listitem:
    - link "Demo standing link to the start page":
      - /url: /`,
  // Each banner is its icon, an image named for its tone, then its words,
  // with no role of its own: no live region and no landmark. The notice ends
  // with its Dismiss button, whose tooltip is hidden from a screen reader.
  Banner: `- heading "Banner" [level=2]
- img "Information"
- text: The demo daemon looks for a newer release once a day.
- img "Warning"
- text: The demo data folder at /srv/demo-data/ladderback-demo-data-folder is almost full, so the demo backups may soon stop.
- img "Information"
- text: A demo update is ready. This browser remembers the last edition dismissed, and shows any other.
- button "Dismiss"
- paragraph:
  - button "Show a newer demo notice"`,
  "Step-up field": `- heading "Step-up field" [level=2]
- paragraph:
  - button "Open the demo step-up"
- paragraph:
  - button "Open the demo short-ID step-up"`,
  // The skeletons are hidden from a screen reader, so only the busy
  // region's words and the control that brings its content are here.
  Skeleton: `- heading "Skeleton" [level=2]
- paragraph: Loading the demo plan…
- paragraph:
  - button "Show the demo plan"`,
  // Each alert is its icon, an image named for its tone, then its words,
  // with no role of its own: no live region. The notice ends with its
  // Dismiss button, whose tooltip is hidden from a screen reader; the
  // button that brings it back is unavailable while it shows.
  Alert: `- heading "Alert" [level=2]
- img "Information"
- text: The demo plan backs up the demo folder every night.
- img "Success"
- text: The demo backup finished and added 12 files.
- img "Warning"
- text: "The demo repository hasn't been checked in 30 days: \u2068"
- link "the demo operations":
  - /url: /operations
- text: \u2069 lists its last runs.
- img "Error"
- text: The demo repository at /srv/demo-backups/ladderback-demo-repository couldn't be read, so nothing was backed up.
- img "Information"
- text: This demo notice goes when you dismiss it.
- button "Dismiss"
- paragraph:
  - button "Show the demo notice again" [disabled]`,
  Steps: `- heading "Steps" [level=2]
- list "Demo hub move":
  - listitem: Check Done
  - listitem: Copy Done
  - listitem: Commit the demo move once every demo snapshot has reached the new hub Current
  - listitem: Switch the machines To do
  - listitem: Clean up To do
- paragraph:
  - button "Advance the demo hub move"
- list "Demo pairing":
  - listitem: Token Done
  - listitem: Connect Failed
  - listitem: Approval To do`,
  Fingerprint: `- heading "Fingerprint" [level=2]
- paragraph:
  - text: Demo hub key fingerprint
  - group "Demo hub key fingerprint":
    - img "7"
    - img "K"
    - img "3"
    - img "Q"
    - img "M"
    - img "9"
    - img "X"
    - img "D"
    - img "2"
    - img "R"
    - img "T"
    - img "V"
    - img "8"
    - img "H"
    - img "F"
    - img "A"
    - img "Q"
    - img "W"
    - img "5"
    - img "N"`,
  Footer: `- heading "Footer" [level=2]
- paragraph: Version ⁨0.4.0⁩
- paragraph: Version ⁨0.4.0+4444444.dirty⁩`,
  // Three cards in a list: the hub's own, its Back up menu closed; one that
  // needs attention, which says why first; and one that runs an operation.
  "Machine card": `- heading "Machine card" [level=2]
- list:
  - listitem:
    - article "demo-hub This hub":
      - heading "demo-hub This hub" [level=3]
      - paragraph: Online now.
      - paragraph: Succeeded
      - term: Last backup
      - definition:
        - time: ⁨11/9/26, 9:00:00 AM⁩
      - term: Next run
      - definition:
        - time: ⁨11/10/26, 9:00:00 AM⁩
      - term: Plans
      - definition: ⁨2⁩
      - button "Back up"
      - link "Open ⁨demo-hub⁩"
  - listitem:
    - article "demo-laptop":
      - heading "demo-laptop" [level=3]
      - paragraph:
        - strong: "Needs attention:"
        - text: ⁨The last run of ⁨photos⁩ failed.⁩
      - paragraph:
        - text: Last connected
        - time: ⁨11/8/26, 6:30:00 PM⁩
        - text: .
      - paragraph: Failed
      - term: Last backup
      - definition:
        - time: ⁨11/8/26, 6:00:00 PM⁩
      - term: Next run
      - definition: None planned
      - term: Plans
      - definition: ⁨3⁩
      - link "Open ⁨demo-laptop⁩"
  - listitem:
    - article "demo-desktop":
      - heading "demo-desktop" [level=3]
      - paragraph: Online now.
      - paragraph: Running
      - progressbar "Progress of the operation running on ⁨demo-desktop⁩": Backing up… 25%
      - term: Last backup
      - definition: None yet
      - term: Next run
      - definition: None planned
      - term: Plans
      - definition: Not reported yet
      - link "Open ⁨demo-desktop⁩"`,
  // Closed, a disclosure menu is its button alone: its panel is hidden, from
  // a screen reader too, until the button shows it.
  "Disclosure menu": `- heading "Disclosure menu" [level=2]
- button "Demo other pages"
- button "Demo pages"
- button "Demo after the menu"`,
  // From 54rem, as the gallery opens, the bar is one line: the brand, then
  // its navigation, the group's button closed at its end. The hamburger
  // isn't drawn there, so a screen reader doesn't find it either.
  "Navigation bar": `- heading "Navigation bar" [level=2]
- paragraph:
  - link "Demo brand":
    - /url: /
- navigation "Demo navigation":
  - list:
    - listitem:
      - link "Demo bar link to this page":
        - /url: /_gallery
    - listitem:
      - link "Demo bar link to the plans":
        - /url: /plans
    - listitem:
      - button "Demo more pages"
- button "Demo after the bar"`,
};
