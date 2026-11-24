### The web UI's own text (technical design §8.4): what every page shares, and
### the start page. A message is named `ui-<where>-<what>`.

# The first link of every page. It moves the keyboard to the page's main content.
ui-skip-link = Skip to content

# The product's name, in the header of every page.
ui-brand = { -brand-name }

# The running version, in the footer of every page that shows the
# navigation. $version is the release's number, such as 0.3.0.
ui-version = Version { $version }

# The available update, on a line of its own in the header wherever it
# shows the navigation, when the daemon's release check found a newer
# release. The whole sentence is the link to that release's notes. $version
# is the release's number, such as 0.4.0.
ui-update-available = Version { $version } is available

# The start page's title, as the browser's tab and history show it.
ui-start-title = { -brand-name }

# The start page's heading: the one main heading of the page.
ui-start-heading = Welcome to { -brand-name }

# The start page's text above its list of plans.
ui-start-plans-body = These are the plans, with when each one runs next.

# What the start page says when there is no plan: the order a first plan is
# made in, in the web UI's own pages. A plan backs up to a repository, so it
# can't be added before one is connected.
ui-start-no-plans = There are no plans yet. To add one, connect a { -repository } on the Repositories page first, then add a plan for it on the Plans page. Each plan is listed here with its next run.

# The link from the start page, while it has no plans, to the repositories
# page, where the repository a first plan backs up to is connected.
ui-start-repositories-link = Connect a { -repository } on the Repositories page

# The link from the start page, while it has no plans, to the Plans page,
# where a plan is added.
ui-start-plans-link = Add a plan on the Plans page

# The link from the start page to where the format of a plan in config.toml is
# described: the project's technical design, on GitHub, outside this app. It is
# for an advanced user who writes the file instead of using the Plans page.
ui-start-plan-format-link = How to write a plan in { "config.toml" } by hand, on GitHub

# The start page's table of plans: its caption.
ui-start-caption = Plans

# The start page's column of plans. Its cells are the rows' headers.
ui-column-plan = Plan

# The start page's column of the instants plans run next on their schedule.
ui-column-next-run = Next run

# The column of the start page's plans table that holds what the user should know about a plan's configuration, shown only when a plan has something.
ui-column-notes = Notes

# The next run of a plan that has a schedule, while the scheduler has planned
# none: it hasn't yet, or the schedule has no run left.
ui-start-not-planned = Not planned yet

# The next run of a plan without a schedule: it runs only when someone starts
# it on the operations page.
ui-start-on-demand = When started from Operations

# The link from the start page to the operations page, where a plan's backup
# is started by hand.
ui-start-operations-link = Start a backup from the Operations page

# The warning banner every page shows on macOS while Ladderback hasn't been given Full Disk Access: what that costs, in one sentence. The banner's link to the steps follows it.
ui-full-disk-access-banner = macOS hasn't given { -brand-name } { -full-disk-access }, so backups leave out Mail, Messages, Safari and other programs' data.

# The banner's link to the start page, where the steps to turn Full Disk Access on are.
ui-full-disk-access-banner-link = How to fix this

# The heading of the start page's steps, shown on macOS while Ladderback hasn't been given Full Disk Access, under the banner that warns of it. It names the steps' region.
ui-full-disk-access-heading = { -full-disk-access } is missing

# What the warning says under its heading, before its steps.
ui-full-disk-access-body = macOS hasn't given { -brand-name } { -full-disk-access }, so backups leave out Mail, Messages, Safari and other programs' data. Nothing can ask macOS for it: only you can turn it on, in System Settings.

# The warning's steps to turn Full Disk Access on, in order, as a numbered list. The first.
ui-full-disk-access-step-open = Open System Settings, then choose Privacy & Security.

# The second step.
ui-full-disk-access-step-pane = Choose { -full-disk-access }.

# The third step. "ladderback" is the name of the program's file, which Finder shows.
ui-full-disk-access-step-turn-on = Turn { -full-disk-access } on for { -brand-name }. If it isn't in the list, choose the add button, then the program { "ladderback" } that runs { -brand-name }.

# The fourth step: the button that asks Ladderback to look again is below the steps.
ui-full-disk-access-step-check = Come back to this page and check again.

# The warning's button, offered only to a browser on the same Mac as Ladderback: it opens System Settings at the Full Disk Access pane and shows Ladderback's program in Finder.
ui-full-disk-access-open = Open { -full-disk-access } settings

# Said once that button has opened System Settings.
ui-full-disk-access-opened = System Settings is open at { -full-disk-access }, and Finder shows the program { "ladderback" }, ready to drag into the list.

# The warning's button that asks Ladderback to look again whether it has Full Disk Access.
ui-full-disk-access-check = Check again

# Said when Ladderback looked again and still hasn't Full Disk Access: the warning stays.
ui-full-disk-access-still-missing = { -full-disk-access } is still missing.

# Said when Ladderback looked again and now has Full Disk Access: the warning is gone.
ui-full-disk-access-granted = { -brand-name } has { -full-disk-access } now.

# Said when Ladderback looked again and couldn't tell whether it has Full Disk Access, so the warning is gone.
ui-full-disk-access-unknown = { -brand-name } can't tell whether it has { -full-disk-access }, so the warning is gone.

# Said when a button of the warning got no answer from the daemon, so nothing changed.
ui-full-disk-access-unreachable = The daemon that serves this page did not answer, so nothing changed. Try again in a moment.

# The heading of the start page's notice, shown when the machine identifier the operating system gives this computer isn't the one recorded beside its machine ID: its data folder was made on another computer. It names the notice's region.
ui-copied-data-folder-heading = This data folder came from another computer

# What the notice says under its heading while the data folder is a copy. Forget, prune and adoption are the maintenance a copy doesn't run.
ui-copied-data-folder-body = { -brand-name } found that this computer isn't the one its data folder was made on: the folder was restored from a backup, or copied with a disk or a virtual machine. Backups run as usual. Forget, prune and adoption are stopped, so this computer can't remove the other computer's snapshots as if they were its own.

# The heading of the start page's notice, shown when the hub this computer paired with refused it, signed, as a second computer under its machine ID: the hub took another computer as this machine first. This computer may be the original, so the notice never says it is a copy. It names the notice's region.
ui-refused-machine-heading = Your { -hub } took another computer as this one

# What the notice says under its heading while the hub refuses this computer: the two ways back, as the hub's refusal names them. `ladderback pairing-token --forget-computer` is typed as it is: keep it a string literal.
ui-refused-machine-body = Your { -hub } refused this computer: it took another computer as this { -machine } first, as happens when a data folder is copied or the system is reinstalled. Backups run as usual, and their reports wait here until the { -hub } takes this computer. If this computer is the copy, take a new ID, restart { -brand-name } and pair it again. If it is the computer that should keep this ID, stop { -brand-name } on the other one or give that one a new ID, then stop the { -hub }'s daemon and run { "ladderback pairing-token --forget-computer" } with this { -machine }'s name on the { -hub }.

# The notice's button, which asks first, in a dialog, before taking a new machine ID.
ui-copied-data-folder-new-id = This is a new computer: take a new ID

# The title of the dialog that asks before taking a new machine ID.
ui-copied-data-folder-review-title = Take a new machine ID?

# What that dialog says: what happens to this computer's plans, its maintenance and the snapshots taken under the old machine ID.
ui-copied-data-folder-review-body = This computer keeps its plans, and its backups carry the new ID once { -brand-name } restarts. It stops maintaining every repository it maintained: turn maintenance back on for each repository this computer should maintain. The snapshots under the old ID stay the other computer's.

# The dialog's button that takes the new machine ID.
ui-copied-data-folder-review-confirm = Take a new ID

# Said in the dialog while the new machine ID is being taken.
ui-copied-data-folder-review-taking = Taking a new machine ID…

# The heading of the notice once the new machine ID is taken, for a copied data folder or a computer its hub refused. It names the notice's region.
ui-copied-data-folder-taken-heading = New machine ID taken

# What the notice says under that heading: the new machine ID is used from the next start, and until then maintenance is stopped. A machine its hub refused maintained until the new ID was taken, so this never says it stays stopped.
ui-copied-data-folder-taken = Restart { -brand-name } to start using it: until then, forget, prune and adoption are stopped.

# Said politely once the new machine ID is taken, on its own, while focus moves to the notice's heading: the heading's words and the sentence under it, whole.
ui-copied-data-folder-taken-said = New machine ID taken. Restart { -brand-name } to start using it: until then, forget, prune and adoption are stopped.

# Said under that: taking the new ID turned maintenance off on each repository this computer maintained.
ui-copied-data-folder-maintenance-off = Maintenance is off on every repository this computer maintained. Once { -brand-name } has restarted, turn it back on for each repository this computer should maintain.

# The notice's link to the Settings page's restart, once the new machine ID is taken.
ui-copied-data-folder-restart = Restart { -brand-name }

# Said in the dialog when the request for a new machine ID got no answer from the daemon, so nothing changed.
ui-copied-data-folder-unreachable = The daemon that serves this page did not answer, so no new machine ID was taken. Try again in a moment.

# The title of the page shown for an address nothing serves: the browser's tab
# and history show it.
ui-not-found-title = Page not found – { -brand-name }

# The heading of that page: its one main heading.
ui-not-found-heading = Page not found

# The text of that page.
ui-not-found-body = There is nothing at this address.

# The link on that page back to the start page.
ui-not-found-link = Go to the start page

## The component gallery, a development build's page for trying the shared
## components with the keyboard (technical design §4.5). A release build has no
## gallery, but its catalog carries these messages all the same.

# The gallery's title, as the browser's tab and history show it.
ui-gallery-title = Component gallery – { -brand-name }

# The gallery's heading: the one main heading of the page.
ui-gallery-heading = Component gallery

# The heading of the gallery's section on the announcer, the part of the page
# that tells a screen reader about changes.
ui-gallery-announcer-heading = Announcer

# The link from the gallery to the start page.
ui-gallery-start-link = Back to the start page

# The link from the gallery to an address nothing serves.
ui-gallery-missing-link = Open a page that does not exist

# The button that queues a polite message: one a screen reader says when it is
# idle.
ui-gallery-polite-button = Say politely: backup finished

# The button that queues a second, different polite message.
ui-gallery-polite-other-button = Say politely: { -machine } back online

# The button that sends an assertive message: one a screen reader says at once.
ui-gallery-assertive-button = Say urgently: restore failed

# The button that opens the modal stand-in, a dialog that makes the rest of the
# page inert.
ui-gallery-open-modal = Open the modal stand-in

# The button that closes the modal stand-in.
ui-gallery-close-modal = Close the modal stand-in

# The heading of the modal stand-in.
ui-gallery-modal-heading = Modal stand-in

# What the polite button announces.
ui-gallery-backup-finished = Backup finished.

# What the second polite button announces. The same word as the term -machine, written out because the sentence starts with a capital letter.
ui-gallery-agent-online = Machine back online.

# What the assertive button announces.
ui-gallery-restore-failed = Restore failed.

# The button inside the modal stand-in that queues a polite message while the
# modal is open.
ui-gallery-modal-polite-button = Say politely in the modal: backup finished

# The button inside the modal stand-in that queues an urgent message while the
# modal is open.
ui-gallery-modal-assertive-button = Say urgently in the modal: restore failed

## The component set's demos, below the announcer's section: one section per
## component (§4.5), each with demo data only.

# The heading of the gallery's dialog demo.
ui-gallery-dialog-heading = Dialog

# The button that opens the demo dialog.
ui-gallery-open-demo-dialog = Open the demo dialog

# The demo dialog's accessible name and heading.
ui-gallery-demo-dialog = Demo dialog

# A line of body text inside the demo dialog.
ui-gallery-demo-dialog-body = A dialog traps focus and returns it on close.

# The button inside the demo dialog that closes it.
ui-gallery-close-demo-dialog = Close the demo dialog

# The button that opens the demo warning, a dialog that hands Escape to the
# page, as the warning before a session ends unused does.
ui-gallery-open-demo-warning = Open the demo warning

# The demo warning's accessible name and heading.
ui-gallery-demo-warning = Demo warning

# What the demo warning says, which describes it.
ui-gallery-demo-warning-body = Escape keeps this warning open, as Keep the demo warning does. Answer the demo warning, and the page closes it.

# What the demo warning says when Escape or Keep the demo warning has kept it
# open.
ui-gallery-demo-warning-kept = The page keeps the demo warning open.

# The demo warning's button that has the page close it.
ui-gallery-answer-demo-warning = Answer the demo warning

# The demo warning's button that keeps it open, which it opens on.
ui-gallery-keep-demo-warning = Keep the demo warning

# The button that opens the demo setting's dialog while the setting is off.
# Changing the setting draws the button anew, and focus returns to the new one.
ui-gallery-turn-demo-setting-on = Turn the demo setting on

# The button that opens the demo setting's dialog while the setting is on.
ui-gallery-turn-demo-setting-off = Turn the demo setting off

# The demo setting dialog's accessible name and heading, at level 3.
ui-gallery-demo-setting = Demo setting

# What the demo setting dialog says, which describes it.
ui-gallery-demo-setting-body = Its button is drawn anew once the setting changes, and focus returns to the new one.

# The demo setting dialog's button that closes it and changes nothing, which it
# opens on.
ui-gallery-keep-demo-setting = Keep the demo setting

# The demo setting dialog's button that turns the setting on or off and closes
# it.
ui-gallery-change-demo-setting = Change the demo setting

# The heading of the gallery's table demo.
ui-gallery-table-heading = Table

# The legend of the table demo's choice of what its table shows.
ui-gallery-table-shows = Demo table shows

# A choice of the table demo: its rows.
ui-gallery-table-shows-rows = Demo rows

# A choice of the table demo: the rows on their way.
ui-gallery-table-shows-loading = Demo loading

# A choice of the table demo: no rows.
ui-gallery-table-shows-empty = Demo empty

# A choice of the table demo: rows that can't be read.
ui-gallery-table-shows-unreachable = Demo unreachable

# The caption of the table demo's table.
ui-gallery-table-caption = Demo folders

# The caption of the table demo's table in a narrow frame.
ui-gallery-table-narrow-caption = Demo folders in a narrow frame

# The caption of the table demo's table set right to left.
ui-gallery-table-rtl-caption = Demo folders, right to left

# Said in the table demo's place while its rows are on their way.
ui-gallery-table-loading = Loading the demo folders…

# Said in the table demo's place when it has no rows.
ui-gallery-table-empty = No demo folders yet.

# Said in the table demo's place when its rows can't be read.
ui-gallery-table-unreachable = Ladderback can't be reached, so no demo folders show.

# A demo folder's name, a row of the table demo.
ui-gallery-table-photos = photos

# A demo folder's name, a row of the table demo.
ui-gallery-table-music = music

# A demo folder's name, a row of the table demo.
ui-gallery-table-documents = documents

# A demo folder's name, a row of the table demo.
ui-gallery-table-notes = notes

# The table demo's column of folder names, each row's header.
ui-gallery-table-name = Name

# The table demo's column of folder sizes.
ui-gallery-table-size-column = Size

# The table demo's column of when each folder changed.
ui-gallery-table-changed = Changed

# The table demo's column of each folder's number of files.
ui-gallery-table-files-column = Files

# The table demo's column of each row's action.
ui-gallery-table-actions = Actions

# The header of the table demo's footer row, which adds up its columns.
ui-gallery-table-total = Total

# A demo folder's size.
# $size (size): the size, such as 4.2 GiB.
ui-gallery-table-size = { $size }

# When a demo folder changed, a day.
# $when (instant): when it changed.
ui-gallery-table-changed-on = { DATETIME($when, dateStyle: "medium") }

# A demo folder's number of files.
# $count (number): how many files.
ui-gallery-table-files = { $count }

# A row's action in the table demo, named with its row: it adds a file to the folder.
# $folder (text): the folder's name.
ui-gallery-table-add-file = Add a file to { $folder }

# The button in the table demo's Name header that sorts the rows by name.
ui-gallery-table-sort-name = Sort by name

# Announced politely when the table demo is sorted by name, A first.
ui-gallery-table-sorted-name-up = Sorted by name, A to Z.

# Announced politely when the table demo is sorted by name, Z first.
ui-gallery-table-sorted-name-down = Sorted by name, Z to A.

# The button in the table demo's Size header that sorts the rows by size.
ui-gallery-table-sort-size = Sort by size

# Announced politely when the table demo is sorted by size, the smallest first.
ui-gallery-table-sorted-size-up = Sorted by size, smallest first.

# Announced politely when the table demo is sorted by size, the largest first.
ui-gallery-table-sorted-size-down = Sorted by size, largest first.

# The button in the table demo's Changed header that sorts the rows by when they changed.
ui-gallery-table-sort-changed = Sort by changed

# Announced politely when the table demo is sorted by when its folders changed, the oldest first.
ui-gallery-table-sorted-changed-up = Sorted by changed, oldest first.

# Announced politely when the table demo is sorted by when its folders changed, the newest first.
ui-gallery-table-sorted-changed-down = Sorted by changed, newest first.

# The button in the table demo's Files header that sorts the rows by their number of files.
ui-gallery-table-sort-files = Sort by files

# Announced politely when the table demo is sorted by files, the fewest first.
ui-gallery-table-sorted-files-up = Sorted by files, fewest first.

# Announced politely when the table demo is sorted by files, the most first.
ui-gallery-table-sorted-files-down = Sorted by files, most first.

# The heading of the gallery's live table demo (§4.5): a table whose status
# column a live update changes, so a changed row is marked until it is seen.
ui-gallery-live-table-heading = Live table

# The caption of the live table demo, and the name of its frame.
ui-gallery-live-table-caption = Demo jobs

# The header of the live table demo's row header column: each row's job.
ui-gallery-live-table-job = Job

# The header of the live table demo's status column, the one a change marks.
ui-gallery-live-table-status = Status

# A demo job's status before it starts.
ui-gallery-live-table-waiting = Waiting

# A demo job's status while it runs.
ui-gallery-live-table-running = Running

# A demo job's status once it has finished.
ui-gallery-live-table-done = Done

# The button that moves the next demo job's status on, as a live update
# would: waiting, then running, then done.
ui-gallery-live-table-change = Change a demo status

# The toggle button in each row of the live table demo, a control a keyboard
# can move focus into the row with.
# $job (string): the demo job's name, "photos".
ui-gallery-live-table-star = Star { $job }

# The heading of the live table demo's last column, a link to another site
# in each row.
ui-gallery-live-table-guide = Guide

# The link to another site in each row of the live table demo, rustic's
# documentation.
# $job (string): the demo job's name, "photos".
ui-gallery-live-table-guide-link = rustic's guide for { $job }

# The heading of the gallery's link demo (§4.5): a link to a page of the
# app, to the page it is on, to another site, inside a sentence and inside a
# status sentence, and right to left.
ui-gallery-link-heading = Link

# The demo link to the start page, a page of the app.
ui-gallery-link-start = Demo link to the start page

# The demo link to the gallery itself, the page the reader is on.
ui-gallery-link-here = Demo link to this page

# The demo link to another site, rustic's documentation.
ui-gallery-link-outside = Demo rustic documentation

# The demo link to another site, in a line set right to left.
ui-gallery-link-outside-rtl = Demo rustic documentation, right to left

# A sentence with a link inside it; $link is the link's words
# (ui-gallery-link-in-sentence-link).
ui-gallery-link-in-sentence = Read { $link } before you write a demo plan.

# The words of the link inside ui-gallery-link-in-sentence.
ui-gallery-link-in-sentence-link = the demo plan format

# A status sentence in the Error tone with a link inside it, drawn in the
# sentence's color; $link is the link's words (ui-gallery-link-status-link).
ui-gallery-link-status = The demo check failed: { $link } says why.

# The words of the link inside ui-gallery-link-status.
ui-gallery-link-status-link = the demo log

# A link standing alone in the link demo's list of links, to the gallery
# itself, the page the reader is on.
ui-gallery-link-standing-here = Demo standing link to this page

# A link standing alone in the link demo's list of links, to the start page.
ui-gallery-link-standing-start = Demo standing link to the start page

# The heading of the gallery's alert demo (§4.5): an inline message in each
# tone, one with a link inside it and one the reader can dismiss.
ui-gallery-alert-heading = Alert

# The demo alert in the Primary tone, information.
ui-gallery-alert-information = The demo plan backs up the demo folder every night.

# The demo alert in the Success tone.
ui-gallery-alert-success = The demo backup finished and added 12 files.

# The demo alert in the Warning tone, with a link inside it, drawn in the
# alert's text color; $link is the link's words
# (ui-gallery-alert-warning-link).
ui-gallery-alert-warning = The demo repository hasn't been checked in 30 days: { $link } lists its last runs.

# The words of the link inside ui-gallery-alert-warning.
ui-gallery-alert-warning-link = the demo operations

# The demo alert in the Error tone, with a long path in it that wraps on a
# small screen.
ui-gallery-alert-error = The demo repository at /srv/demo-backups/ladderback-demo-repository couldn't be read, so nothing was backed up.

# The demo alert the reader can dismiss with its button.
ui-gallery-alert-dismissible = This demo notice goes when you dismiss it.

# A demo button that brings the dismissed demo notice back, unavailable
# while the notice shows; focus moves to it when the notice is dismissed.
ui-gallery-alert-show = Show the demo notice again

# The heading of the gallery's text field demo (§4.5): every kind, size and
# state of the shared text field.
ui-gallery-text-field-heading = Text field

# The label of the demo text field whose error a button shows and clears.
ui-gallery-repository-name = Repository name

# The hint under that demo text field, describing its input.
ui-gallery-repository-hint = The name of the repository to check.

# The button that checks that demo text field.
ui-gallery-check-repository = Check the repository name

# The error that demo text field shows when its input is empty.
ui-gallery-repository-required = A repository name is required.

# The label of the demo text field of many lines, which takes one entry a
# line.
ui-gallery-demo-folders = Demo folders

# The hint under the demo text field of many lines, describing its input.
ui-gallery-demo-folders-hint = One folder or file a line.

# The button that checks the demo text field of many lines.
ui-gallery-check-folders = Check the demo folders

# The error the demo text field of many lines shows.
ui-gallery-folders-required = Enter at least one folder.

# The heading of the gallery's checkbox demo.
ui-gallery-checkbox-heading = Checkbox

# The label of the demo checkbox that starts off.
ui-gallery-demo-notification = Send a demo notification

# The description under the demo checkbox that starts off.
ui-gallery-demo-notification-hint = A demo choice: turning it on changes nothing.

# The label of the demo checkbox that starts on. It has no description.
ui-gallery-demo-log = Keep the demo log

# The label of the third demo checkbox, which shows a checkbox's error: it starts off.
ui-gallery-demo-terms = Accept the demo terms

# The description under the third demo checkbox.
ui-gallery-demo-terms-hint = The demo terms change nothing.

# The error the third demo checkbox shows under its description, as a form would once checked.
ui-gallery-demo-terms-error = Accept the demo terms to go on.

# The heading of the gallery's radio group demo.
ui-gallery-radio-heading = Radio group

# The legend of the demo radio group, which names the group.
ui-gallery-demo-frequency = Demo check frequency

# The description under the demo radio group's legend.
ui-gallery-demo-frequency-hint = A demo choice: choosing one changes nothing.

# The first choice of the demo radio group.
ui-gallery-demo-daily = Every day

# The second choice of the demo radio group, the one chosen as the demo opens.
ui-gallery-demo-weekly = Every week

# The third choice of the demo radio group.
ui-gallery-demo-monthly = Every month

# The legend of the second demo radio group, whose choices are named in other languages.
ui-gallery-demo-language = Demo language

# A choice of the second demo radio group: the name of English, written in English. Not translated.
ui-gallery-demo-english = { "English" }

# A choice of the second demo radio group: the name of Japanese, written in Japanese, which the group marks as Japanese. Not translated.
ui-gallery-demo-japanese = { "日本語" }

# A choice of the second demo radio group: the name of Arabic, written in Arabic, which the group marks as Arabic, running right to left. Not translated.
ui-gallery-demo-arabic = { "العربية" }

# The legend of the third demo radio group, which shows a group's error: no choice is made in it.
ui-gallery-demo-window = Demo backup window

# The first choice of the third demo radio group.
ui-gallery-demo-mornings = Mornings

# The second choice of the third demo radio group.
ui-gallery-demo-evenings = Evenings

# The error the third demo radio group shows under its legend, as a form would once checked.
ui-gallery-demo-window-error = Choose a demo backup window.

# The heading of the gallery's status badge demo.
ui-gallery-badge-heading = Status badge

# The empty badge of the status badge demo: a badge with nothing to tell.
ui-gallery-badge-empty = Empty

# The heading of the gallery's badge demo: the shared badge, a count or a tag on a pill.
ui-gallery-badges-heading = Badge

# A badge of the badge demo that counts something: the changes a demo plan has waiting. The words say what the number counts.
# $count (number): how many changes wait, 3 in the demo.
ui-gallery-badges-pending = Pending changes { $count }

# A badge of the badge demo that names something: a tag a demo snapshot carries.
ui-gallery-badges-tag = Demo tag

# A badge of the badge demo whose words are long enough to wrap on a small screen with larger text.
ui-gallery-badges-long = Kept by the demo plan's retention rules

# The heading of the gallery's demo of the shared vocabulary (§4.5): the
# tones, sizes and placements every shared component takes.
ui-gallery-vocabulary-heading = Shared vocabulary

# The headings of the vocabulary demo's four lists, which name the lists
# too. Status pairs lists Success, Warning and Error again, each in its words
# on its fill: the two colors a part tinted in that status takes.
ui-gallery-tones-heading = Tones
ui-gallery-status-pairs-heading = Status pairs
ui-gallery-sizes-heading = Sizes
ui-gallery-placements-heading = Placements

# The tones, each written in its own color beside a swatch of it: Primary is
# the accent for action, Secondary the neutral one, Success, Warning and
# Error the three status colors, and Inherit the color of the text around a
# part, such as a spinner in a button.
ui-gallery-tone-primary = Primary
ui-gallery-tone-secondary = Secondary
ui-gallery-tone-success = Success
ui-gallery-tone-warning = Warning
ui-gallery-tone-error = Error
ui-gallery-tone-inherit = Inherit

# The sizes, each beside a swatch drawn at that size, smallest first.
ui-gallery-size-small = Small
ui-gallery-size-medium = Medium
ui-gallery-size-large = Large
ui-gallery-size-xlarge = Extra large

# The placements, each beside a swatch placed so in its line: in the line of
# text, or on a line of its own at its start, in its middle or at its end,
# in the page's direction.
ui-gallery-placement-inline = Inline
ui-gallery-placement-start = Start
ui-gallery-placement-center = Center
ui-gallery-placement-end = End

# The heading of the gallery's progress demo.
ui-gallery-progress-heading = Progress

# The demo progress bar's accessible name.
ui-gallery-demo-backup-progress = Demo backup progress

# The button that advances the demo progress.
ui-gallery-advance-progress = Advance the demo progress

# The accessible name of the demo progress bar whose size isn't known.
ui-gallery-demo-unknown-progress = Demo progress of unknown size

# The accessible name of the demo progress bar whose text names the step a
# running operation is in.
ui-gallery-demo-step-progress = Demo progress of a step

# The names of the demo's progress bars, each 60% done, one for each size of
# the shared vocabulary (Small, Medium, Large, Extra large), then one for each
# of its tones. The size and tone names match the vocabulary demo's.
ui-gallery-progress-small = Demo progress at the Small size
ui-gallery-progress-medium = Demo progress at the Medium size
ui-gallery-progress-large = Demo progress at the Large size
ui-gallery-progress-xlarge = Demo progress at the Extra large size
ui-gallery-progress-primary = Demo progress in the Primary tone
ui-gallery-progress-secondary = Demo progress in the Secondary tone
ui-gallery-progress-success = Demo progress in the Success tone
ui-gallery-progress-warning = Demo progress in the Warning tone
ui-gallery-progress-error = Demo progress in the Error tone
ui-gallery-progress-inherit = Demo progress in the Inherit tone

# The heading of the gallery's toast demo.
ui-gallery-toast-heading = Toast

# The button that saves the demo settings and shows a toast.
ui-gallery-save-settings = Save the demo settings

# The toast shown after the demo settings are saved.
ui-gallery-settings-saved = Settings saved.

# The button that queues a demo check and shows an information toast.
ui-gallery-queue-check = Queue a demo check

# The information toast shown after the demo check is queued.
ui-gallery-check-queued = Demo check queued.

# The button that backs up the demo plan and shows a warning toast.
ui-gallery-back-up-plan = Back up the demo plan

# The warning toast shown after the demo backup finishes with warnings.
ui-gallery-backup-warned = Demo backup finished with warnings: 2 files couldn't be read.

# The button that restores the demo snapshot and shows an error toast.
ui-gallery-restore-snapshot = Restore the demo snapshot

# The error toast shown after the demo restore fails.
ui-gallery-demo-restore-failed = The demo restore failed.

# The alert that keeps the demo restore's failure on the page, under its
# toast: what failed, why, and what to do.
ui-gallery-demo-restore-failed-detail = The demo restore failed: its target folder can't be written to. Choose another folder and restore again.

# The heading of the gallery's review dialog demo.
ui-gallery-review-heading = Review dialog

# The button that opens the review demo that has something to confirm.
ui-gallery-open-review = Open a demo review

# The button that opens the review demo that has nothing to do.
ui-gallery-open-empty-review = Open an empty demo review

# The accessible name and heading of the review demo that has something to confirm.
ui-gallery-demo-review = Demo review to confirm

# What the review demo that has something to confirm says it is about.
ui-gallery-demo-review-body = Nothing changes until you confirm this demo review.

# The button inside that review demo that confirms it.
ui-gallery-confirm-demo-review = Confirm the demo review

# The second confirm button inside that review demo, after the first: a
# review with two ways to go ahead, as Settings' restart has (once they
# finish, or now).
ui-gallery-also-demo-review = Confirm the demo review now

# Announced politely when the demo review is confirmed.
ui-gallery-demo-review-confirmed = Demo review confirmed.

# The accessible name and heading of the review demo that has nothing to do.
ui-gallery-demo-empty-review = Demo review with nothing to do

# What the review demo that has nothing to do says.
ui-gallery-demo-empty-review-body = There is nothing to change.

# The button that opens the review demo that starts on a button of its own.
ui-gallery-open-secret-review = Open a demo secret

# The accessible name and heading of the review demo that starts on a button of its own, as a dialog that shows a secret once does.
ui-gallery-demo-secret-review = Demo secret shown once

# What the review demo that starts on a button of its own says.
ui-gallery-demo-secret-review-body = This demo secret is shown only once.

# The button inside that review demo that focus starts on. The demo shows only where focus starts: pressing it does nothing.
ui-gallery-copy-demo-secret = Copy the demo secret

# The button that opens the review demo that is held: it can't be confirmed yet, and says why.
ui-gallery-open-held-review = Open a held demo review

# The accessible name and heading of the held review demo.
ui-gallery-demo-held-review = Demo review on hold

# What the held review demo says first.
ui-gallery-demo-held-review-body = This demo review can't be confirmed yet. Nothing runs.

# The held review demo's confirm button, which says why it is held when pressed.
ui-gallery-confirm-held-review = Confirm the held demo review

# Why the held review demo can't be confirmed, shown beside its confirm button and said when it is pressed.
ui-gallery-demo-held-review-reason = The demo review waits for something that never comes.

# The heading of the pause control's demo.
ui-gallery-pause-heading = Pause control

# The demo's pause control: it pauses the demo's own updates.
ui-gallery-pause-demo = Pause the demo updates

# A button that ends a demo run: said at once while updates are not paused,
# counted while they are.
ui-gallery-finish-demo-run = Finish a demo run

# The heading of the gallery's demo of the tree (§4.5): a snapshot's folders
# and files, each folder read as it is opened.
ui-gallery-tree-heading = Tree

# The name of the demo tree.
ui-gallery-demo-tree = Demo snapshot

# The names of the demo tree's folders and files: made-up names, as a
# snapshot's would be.
ui-gallery-tree-documents = documents
ui-gallery-tree-empty = empty
ui-gallery-tree-many = many
ui-gallery-tree-slow = slow
ui-gallery-tree-unreadable = unreadable
ui-gallery-tree-readme = readme.txt
ui-gallery-tree-notes = notes.txt
ui-gallery-tree-report = report.txt
ui-gallery-tree-alpha = alpha.txt
ui-gallery-tree-bravo = bravo.txt
ui-gallery-tree-charlie = charlie.txt
ui-gallery-tree-delta = delta.txt
ui-gallery-tree-echo = echo.txt
ui-gallery-tree-latest = latest
ui-gallery-tree-pipe = pipe

# The shown name of a demo file whose name holds U+202E, which would show
# its "txt.exe" reversed: the daemon makes the character visible (decision
# 102), and the row is named by this, not by the file's key.
ui-gallery-tree-shown = abc[U+202E]txt.exe

# The name of a demo file written right to left: "notes.txt" in Hebrew.
ui-gallery-tree-written-rtl = הערות.txt

# What the demo tree says when its folder "unreadable" is opened.
ui-gallery-tree-unreadable-said = The demo folder could not be read.

# The heading of the gallery's spinner demo (§4.5): the drawing that turns
# while a page waits for something it can't show yet.
ui-gallery-spinner-heading = Spinner

# The text of a line in the gallery's spinner demo, a step of a longer task,
# which the first spinner and its label follow on the same line.
ui-gallery-spinner-step = Step 2 of 3:

# The label beside the demo's first spinner, as a page would show it while it
# reads a list.
ui-gallery-spinner-loading = Loading the demo snapshots…

# The names of the demo's spinners that show no label, one for each size of
# the shared vocabulary (Small, Medium, Large, Extra large): a screen reader
# reads them as images. The size names match the vocabulary demo's.
ui-gallery-spinner-small = Demo spinner at the Small size
ui-gallery-spinner-medium = Demo spinner at the Medium size
ui-gallery-spinner-large = Demo spinner at the Large size
ui-gallery-spinner-xlarge = Demo spinner at the Extra large size

# The names of the demo's spinners that show no label, one for each tone of
# the shared vocabulary. The tone names match the vocabulary demo's.
ui-gallery-spinner-primary = Demo spinner in the Primary tone
ui-gallery-spinner-secondary = Demo spinner in the Secondary tone
ui-gallery-spinner-success = Demo spinner in the Success tone
ui-gallery-spinner-warning = Demo spinner in the Warning tone
ui-gallery-spinner-error = Demo spinner in the Error tone
ui-gallery-spinner-inherit = Demo spinner in the Inherit tone

# The labels of the demo's spinners on lines of their own, each saying where
# on its line it sits.
ui-gallery-spinner-start = Loading at the start of the line…
ui-gallery-spinner-center = Loading in the middle of the line…
ui-gallery-spinner-end = Loading at the end of the line…

# A demo button busy with the save it started: its label names the action,
# as a busy button's does, and a spinner a screen reader skips stands before
# it.
ui-gallery-spinner-save = Save the demo notes

# The heading of the gallery's fingerprint demo (§4.5): a key's fingerprint
# in groups of four, spoken a character at a time.
ui-gallery-fingerprint-heading = Fingerprint

# The name of the demo's fingerprint, the shape of a hub key's.
ui-gallery-fingerprint-name = Demo { -hub } key fingerprint

# The heading of the gallery's footer demo (§4.5): the running version at
# the foot of every page.
ui-gallery-footer-heading = Footer

# The heading of the gallery's disclosure menu demo (§4.5): buttons that
# show and hide a panel of links under them, as the header's Settings menu.
ui-gallery-disclosure-menu-heading = Disclosure menu

# The words of the demo's second menu's button, whose panel holds a link to
# the gallery itself, the page the reader is on.
ui-gallery-disclosure-menu-pages = Demo pages

# The words of the demo's first menu's button, whose panel holds a link to
# another page.
ui-gallery-disclosure-menu-other = Demo other pages

# A row of the second menu: a link to the gallery itself.
ui-gallery-disclosure-menu-here = Demo menu link to this page

# A row of the second menu: a link to the start page.
ui-gallery-disclosure-menu-start = Demo menu link to the start page

# The second menu's last row: a button, which does nothing in the demo.
ui-gallery-disclosure-menu-action = Demo menu action

# The first menu's row: a link to the Log page.
ui-gallery-disclosure-menu-log = Demo menu link to the log

# A button after the demo's menus, where focus goes when it leaves them;
# it does nothing.
ui-gallery-disclosure-menu-after = Demo after the menu

# The heading of the gallery's steps demo (§4.5): a process's steps in
# order, each with its state, and where the process has reached.
ui-gallery-steps-heading = Steps

# The name of the demo's first list of steps: a demo of moving the hub, with
# its five steps, one of them a long name that wraps.
ui-gallery-steps-move = Demo { -hub } move
ui-gallery-steps-check = Check
ui-gallery-steps-copy = Copy
ui-gallery-steps-commit = Commit the demo move once every demo snapshot has reached the new { -hub }
ui-gallery-steps-switch = Switch the { -machine(form: "other") }
ui-gallery-steps-clean-up = Clean up

# The button that moves the demo hub move on to its next step, and from its
# end back to its first.
ui-gallery-steps-advance = Advance the demo { -hub } move

# The name of the demo's second list of steps: a demo of pairing a machine,
# which failed at its second step.
ui-gallery-steps-pairing = Demo pairing
ui-gallery-steps-token = Token
ui-gallery-steps-connect = Connect
ui-gallery-steps-approval = Approval

# The heading of the gallery's skeleton demo (§4.5): the still shape of
# content on its way, which a screen reader skips.
ui-gallery-skeleton-heading = Skeleton

# The words the demo's busy region shows beside its skeleton while its
# content, a demo backup plan, is on its way: the wait said in words.
ui-gallery-skeleton-loading = Loading the demo plan…

# The demo plan once it has come, in place of the words above: its name,
# then three short lines, which take the place of the skeleton's three.
ui-gallery-skeleton-plan = Demo plan: home
ui-gallery-skeleton-folder = Backs up the demo folder.
ui-gallery-skeleton-schedule = Runs every night.
ui-gallery-skeleton-keeps = Keeps a week of snapshots.

# A demo toggle button that brings the demo plan in place of its skeleton,
# pressed while the plan shows; pressing it again shows the wait again.
ui-gallery-skeleton-show = Show the demo plan

# The heading of the gallery's banner demo (§4.5): a page-wide notice in
# each tone it takes, one with a long path in it and one the reader can
# dismiss, which this browser then remembers.
ui-gallery-banner-heading = Banner

# The demo banner in the Primary tone, information.
ui-gallery-banner-information = The demo daemon looks for a newer release once a day.

# The demo banner in the Warning tone, with a long path in it that wraps on
# a small screen.
ui-gallery-banner-warning = The demo data folder at /srv/demo-data/ladderback-demo-data-folder is almost full, so the demo backups may soon stop.

# The demo banner the reader can dismiss with its button. The browser keeps
# the last edition dismissed, and the demo shows any other edition again.
ui-gallery-banner-dismissible = A demo update is ready. This browser remembers the last edition dismissed, and shows any other.

# A demo button that shows a newer edition of the dismissible demo notice,
# which a dismissal of the one before doesn't hide; focus moves to it when
# the notice is dismissed.
ui-gallery-banner-newer = Show a newer demo notice

# The heading of the gallery's icon demo (§4.5): one Lucide glyph, beside
# words or inside a control.
ui-gallery-icon-heading = Icon

# The words of a line in the gallery's icon demo, which a clock icon
# stands before as decoration.
ui-gallery-icon-line = The next demo backup starts at 02:00 every night.

# The names of the demo's icons that a label names, one for each size of
# the shared vocabulary (Small, Medium, Large, Extra large): a screen reader
# reads them as images. The size names match the vocabulary demo's.
ui-gallery-icon-small = Demo icon at the Small size
ui-gallery-icon-medium = Demo icon at the Medium size
ui-gallery-icon-large = Demo icon at the Large size
ui-gallery-icon-xlarge = Demo icon at the Extra large size

# The name of a demo toggle button that shows only an icon (a bell, crossed
# out while it is off): the button's own label, which a screen reader reads
# as its name, pressed or not. Pressing it turns the demo's notifications
# on or off, and nothing else.
ui-gallery-icon-notifications = Demo notifications

# The heading of the gallery's button demo (§4.5).
ui-gallery-button-heading = Button

# The demo's buttons in each appearance (Filled, Outlined, Ghost) and each
# tone of the shared vocabulary, each named by the two in words, so its
# color never says them alone. A Filled button has no Inherit tone. The
# appearance and tone names are the component's and the vocabulary's.
ui-gallery-button-filled-primary = Filled Primary
ui-gallery-button-filled-secondary = Filled Secondary
ui-gallery-button-filled-success = Filled Success
ui-gallery-button-filled-warning = Filled Warning
ui-gallery-button-filled-error = Filled Error
ui-gallery-button-outlined-primary = Outlined Primary
ui-gallery-button-outlined-secondary = Outlined Secondary
ui-gallery-button-outlined-success = Outlined Success
ui-gallery-button-outlined-warning = Outlined Warning
ui-gallery-button-outlined-error = Outlined Error
ui-gallery-button-outlined-inherit = Outlined Inherit
ui-gallery-button-ghost-primary = Ghost Primary
ui-gallery-button-ghost-secondary = Ghost Secondary
ui-gallery-button-ghost-success = Ghost Success
ui-gallery-button-ghost-warning = Ghost Warning
ui-gallery-button-ghost-error = Ghost Error
ui-gallery-button-ghost-inherit = Ghost Inherit

# The demo's buttons at each size of the shared vocabulary (Small, Medium,
# Large, Extra large). The size names match the vocabulary demo's.
ui-gallery-button-small = Demo button at the Small size
ui-gallery-button-medium = Demo button at the Medium size
ui-gallery-button-large = Demo button at the Large size
ui-gallery-button-xlarge = Demo button at the Extra large size

# A demo button with an icon (a calendar with a plus) before its label.
ui-gallery-button-schedule = Schedule a demo backup

# The names of the demo's icon-only buttons (an "i" in a circle), one at
# each size: a screen reader reads each as the button's name.
ui-gallery-button-details-small = Demo details at the Small size
ui-gallery-button-details-medium = Demo details at the Medium size
ui-gallery-button-details-large = Demo details at the Large size
ui-gallery-button-details-xlarge = Demo details at the Extra large size

# Three demo buttons that turn busy together: one with only its label, one
# with an icon (a circle with a check) and one with only an icon (an arrow
# turning round a clock), which this names.
ui-gallery-button-save = Save the demo plan
ui-gallery-button-check = Check the demo repository
ui-gallery-button-refresh = Refresh the demo

# A demo toggle button that makes the three buttons above busy while it is
# pressed, and lets them go when it is pressed again.
ui-gallery-button-working = Demo work in progress

# A demo button that can't be used: it stays in the Tab order and ignores
# presses.
ui-gallery-button-delete = Delete the demo snapshot

# A demo toggle button, pressed or not, which changes nothing but its own
# state.
ui-gallery-button-hidden-files = Show the demo's hidden files

# A demo form's submit button, and what the page says politely when the
# form is sent.
ui-gallery-button-send = Send the demo form
ui-gallery-button-sent = Demo form sent.

# How many times the demo's buttons were pressed (the toggles not counted),
# a sentence under them; $count is that number.
ui-gallery-button-presses =
    { $count ->
        [0] The demo buttons haven't been pressed yet.
        [one] The demo buttons were pressed once.
       *[other] The demo buttons were pressed { $count } times.
    }

# The demo password field, which a button beside it shows and hides, and
# its hint.
ui-gallery-text-field-password = Demo password
ui-gallery-text-field-password-hint = A demo field: what is typed here goes nowhere.

# The demo number field, which asks for the keyboard of digits, and its hint.
ui-gallery-text-field-copies = Demo copies to keep
ui-gallery-text-field-copies-hint = A whole number, such as { "7" }.

# The demo text field with an icon (a clock) at its start, and its hint.
ui-gallery-text-field-time = Demo backup time
ui-gallery-text-field-time-hint = When the demo backup starts, such as { "02:00" }.

# The demo text field that must be filled in, and its hint. Its label says
# "(required)" after it (ui-text-field-required).
ui-gallery-text-field-plan = Demo plan name
ui-gallery-text-field-plan-hint = A demo field that must be filled in.

# The demo text field shown with its error, its hint and the error.
ui-gallery-text-field-host = Demo host
ui-gallery-text-field-host-hint = The demo machine's host name.
ui-gallery-text-field-host-error = Enter a host name, such as { "backup.example" }.

# The demo read-only text field, holding a demo snapshot's short ID, and its
# hint.
ui-gallery-text-field-snapshot = Demo snapshot ID
ui-gallery-text-field-snapshot-hint = The demo snapshot's short ID.

# The demo disabled text field, holding a demo folder, and its hint.
ui-gallery-text-field-location = Demo repository location
ui-gallery-text-field-location-hint = Set when the demo repository was made.

# The demo disabled password field, which has no button to show it, and
# its hint.
ui-gallery-text-field-keyring = Demo keyring password
ui-gallery-text-field-keyring-hint = Kept in the demo keyring, and changed there.

# The demo text fields at each size of the shared vocabulary (Small, Medium,
# Large, Extra large), each beside a button of its size, which the next four
# name. The size names match the vocabulary demo's.
ui-gallery-text-field-small = Demo tag at the Small size
ui-gallery-text-field-medium = Demo tag at the Medium size
ui-gallery-text-field-large = Demo tag at the Large size
ui-gallery-text-field-xlarge = Demo tag at the Extra large size
ui-gallery-text-field-add-small = Add the demo tag at the Small size
ui-gallery-text-field-add-medium = Add the demo tag at the Medium size
ui-gallery-text-field-add-large = Add the demo tag at the Large size
ui-gallery-text-field-add-xlarge = Add the demo tag at the Extra large size

# The heading of the gallery's tabs demo (§4.5): a row of tabs, each showing
# its panel below the row, one stop for Tab.
ui-gallery-tabs-heading = Tabs

# A set of demo tabs that selects each tab the arrow keys arrive at, named by
# the heading above it. Its middle panel holds a field, to show that what is
# typed in it stays while another tab shows.
ui-gallery-tabs-repository = Demo repository
ui-gallery-tabs-overview = Demo overview
ui-gallery-tabs-overview-text = The demo repository holds 12 snapshots of two demo plans.
ui-gallery-tabs-note = Demo note
ui-gallery-tabs-note-field = Demo note text
ui-gallery-tabs-note-hint = What you type here stays while another tab shows.
ui-gallery-tabs-history = Demo history
ui-gallery-tabs-history-text = The last demo check found nothing wrong.

# A set of demo tabs chosen by hand, as tabs whose panels read from the daemon
# would be: the paragraph says how, and the set is named by its own label. The
# line after the set names the tab it says is chosen, as a page that reads that
# tab's panel from the daemon would follow it; $tab is one of the tabs' names.
ui-gallery-tabs-manual-hint = These demo tabs are chosen with Enter or Space. The arrow keys only move between them.
ui-gallery-tabs-agent = Demo { -machine }
ui-gallery-tabs-log = Demo log
ui-gallery-tabs-log-text = A demo panel that a page would read from the daemon once its tab is chosen.
ui-gallery-tabs-stats = Demo stats
ui-gallery-tabs-stats-text = The demo stats, read once their tab is chosen.
ui-gallery-tabs-plans = Demo plans
ui-gallery-tabs-plans-text = The demo plans, read once their tab is chosen.

# The gallery's manual tabs demo says which tab is chosen.
# $tab (text): the chosen tab's name.
ui-gallery-tabs-chosen = Chosen demo tab: { $tab }

# A set of demo tabs in a part of the page that runs right to left: the first
# tab is at the right, and the left arrow goes to the next.
ui-gallery-tabs-rtl = Demo tabs, right to left
ui-gallery-tabs-first = First demo tab
ui-gallery-tabs-first-text = The first demo panel.
ui-gallery-tabs-second = Second demo tab
ui-gallery-tabs-second-text = The second demo panel.
ui-gallery-tabs-third = Third demo tab
ui-gallery-tabs-third-text = The third demo panel.

# The heading of the gallery's vertical tabs demo (§4.5): a column of tabs
# at the start of their panel, which the up and down arrows move between.
ui-gallery-tabs-vertical-heading = Vertical tabs

# A set of demo vertical tabs, named by its own label.
ui-gallery-tabs-plan = Demo plan
ui-gallery-tabs-sources = Demo sources
ui-gallery-tabs-sources-text = The demo plan backs up two demo folders.
ui-gallery-tabs-times = Demo times
ui-gallery-tabs-times-text = The demo plan runs every night.
ui-gallery-tabs-hooks = Demo hooks
ui-gallery-tabs-hooks-text = The demo plan runs no hooks.

# The same kind of tabs in a part of the page that runs right to left: a
# column at the start of the panel, on the right.
ui-gallery-tabs-vertical-rtl = Demo plan, right to left
ui-gallery-tabs-vertical-rtl-first = First vertical tab
ui-gallery-tabs-vertical-rtl-first-text = The first vertical panel.
ui-gallery-tabs-vertical-rtl-second = Second vertical tab
ui-gallery-tabs-vertical-rtl-second-text = The second vertical panel.
ui-gallery-tabs-vertical-rtl-third = Third vertical tab
ui-gallery-tabs-vertical-rtl-third-text = The third vertical panel.

# The heading of the gallery's responsive tabs demo (§4.5): tabs in a column
# where their container has room for it beside the panel, and in a row
# above the panel where it hasn't.
ui-gallery-tabs-responsive-heading = Responsive tabs

# A set of demo responsive tabs as wide as the page. Its middle panel holds
# a field, to show that what is typed in it stays while the tabs change
# layout.
ui-gallery-tabs-settings = Demo settings
ui-gallery-tabs-general = Demo general
ui-gallery-tabs-general-text = The demo settings shown here change nothing.
ui-gallery-tabs-address = Demo address
ui-gallery-tabs-address-field = Demo address text
ui-gallery-tabs-address-hint = What you type here stays while the tabs change layout.
ui-gallery-tabs-about = Demo about
ui-gallery-tabs-about-text = The demo { -machine } runs on this computer.

# The same kind of tabs in a narrow column of the page that runs right to
# left: a row, as the column has no room for the list beside the panel.
ui-gallery-tabs-column = Demo settings in a narrow column, right to left
ui-gallery-tabs-column-first = First column tab
ui-gallery-tabs-column-first-text = The first column panel.
ui-gallery-tabs-column-second = Second column tab
ui-gallery-tabs-column-second-text = The second column panel.
ui-gallery-tabs-column-third = Third column tab
ui-gallery-tabs-column-third-text = The third column panel.

# The heading of the gallery's demo of tabs that start on a tab their caller
# marks (§4.5), as Settings marks the tab its address names.
ui-gallery-tabs-marked-heading = Tabs starting on a marked tab

# A set of demo tabs, named by its own label, that starts on its middle tab.
# The ar-XB rows of features/ui/components/tabs.feature name these tabs as
# that pseudolocale writes them: a change to a tab's words changes those rows.
ui-gallery-tabs-marked = Demo snapshot
ui-gallery-tabs-marked-summary = Demo summary
ui-gallery-tabs-marked-summary-text = A demo plan took the demo snapshot.
ui-gallery-tabs-marked-files = Demo files
ui-gallery-tabs-marked-files-text = These demo tabs start on this one, which the gallery marks.
ui-gallery-tabs-marked-tags = Demo tags
ui-gallery-tabs-marked-tags-text = The demo snapshot has no tags.

# The heading of the gallery's tooltip demo (§4.5): a short note beside a
# control, shown on keyboard focus or a resting pointer.
ui-gallery-tooltip-heading = Tooltip

# A demo button and the tooltip that describes it, which a screen reader
# reads after the button's name. Pressing it changes nothing.
ui-gallery-tooltip-prune = Prune the demo repository
ui-gallery-tooltip-prune-note = Frees the space no snapshot uses any longer. A demo: pressing it changes nothing.

# The name of a demo button that shows only an icon (a calendar with a
# clock): its own label, which its tooltip shows too, from this one string.
ui-gallery-tooltip-schedule = Demo schedule

# Demo buttons whose tooltips ask to stand on one side of them: above,
# below, at the start of their line and at its end, where the line's start
# is the left in English and the right in a right-to-left language. Their
# notes are short, so each has room on its side.
ui-gallery-tooltip-above = Demo note above
ui-gallery-tooltip-above-note = Above its button.
ui-gallery-tooltip-below = Demo note below
ui-gallery-tooltip-below-note = Below its button.
ui-gallery-tooltip-start = Demo note at the start
ui-gallery-tooltip-start-note = At its start.
ui-gallery-tooltip-end = Demo note at the end
ui-gallery-tooltip-end-note = At its end.

# The same two buttons in a line set right to left, where the start is the
# right.
ui-gallery-tooltip-start-rtl = Demo note at the start, right to left
ui-gallery-tooltip-end-rtl = Demo note at the end, right to left

# A demo button whose tooltip is long, to show it wrapping onto lines of its
# own, never cut.
ui-gallery-tooltip-long = Demo long note
ui-gallery-tooltip-long-note = A longer note, for a control that needs a little more said: it wraps onto lines of its own at about twenty of its letters' widths, or at the window's width on a small screen, and none of its words is ever cut.

# The last button of the gallery, at the foot of the page, whose tooltip asks
# to stand below it and has no room there.
ui-gallery-tooltip-foot = Demo note at the foot of the page
ui-gallery-tooltip-foot-note = Asked to stand below its button, with no room there, it stands above.

# The heading of the gallery's demo of the step-up field (§4.5): the field a
# restore is confirmed with, in a review dialog.
ui-gallery-step-up-heading = Step-up field

# The button that opens the step-up demo.
ui-gallery-open-step-up = Open the demo step-up

# The title of the step-up demo's dialog.
ui-gallery-demo-step-up = Demo step-up

# What the step-up demo's dialog says first. Nothing is checked or sent.
ui-gallery-demo-step-up-body = A demo of the field a restore is confirmed with. Nothing is checked or sent.

# The step-up demo's confirm button, which shows the field's error.
ui-gallery-check-demo-step-up = Check the demo password

# The error the step-up demo shows, whatever was typed.
ui-gallery-demo-step-up-wrong = That is not the demo password. Type it again, or paste it.

# The button that opens the step-up demo that asks for a short ID, as with sign-in off.
ui-gallery-open-step-up-id = Open the demo short-ID step-up

# The title of the dialog of the step-up demo that asks for a short ID.
ui-gallery-demo-step-up-id = Demo short-ID step-up

# The label of the short-ID step-up demo's field.
ui-gallery-demo-step-up-id-label = Demo ID

# What the short-ID step-up demo's field says under its label, before the demo short ID it shows.
ui-gallery-demo-step-up-id-hint = Type or paste the demo ID to confirm:

# The short-ID step-up demo's confirm button, which shows the field's error.
ui-gallery-check-demo-step-up-id = Check the demo ID

# The error the short-ID step-up demo shows, whatever was typed.
ui-gallery-demo-step-up-id-wrong = That is not the demo ID. Type or paste the 8 characters shown above.

## The review dialog (§4.5): what a run would do, shown in a dialog before it
## runs. The gallery's demos and the snapshots page both use its buttons.

# The button of a review dialog that has something to confirm, which closes it
# without running anything.
ui-review-cancel = Cancel

# The only button of a review dialog that has nothing to run.
ui-review-close = Close

## The operations page: the backup history, live (§4.4).

# The operations page's title, as the browser's tab and history show it.
ui-operations-title = Operations – { -brand-name }

# The operations page's heading: its one main heading.
ui-operations-heading = Operations

# The operations page's text, until its history loads.
ui-operations-body = The history of every operation, live.

# The operations table's column of what each row is: a backup's plan, or the
# maintenance run and its repository. Its cells are the rows' headers.
ui-column-operation = Operation

# The operations table's column of statuses.
ui-column-status = Status

# The operations table's column of the instants runs started.
ui-column-started = Started

# The operations table's column of what a run that succeeded produced.
ui-column-result = Result

# The operations table's column of what a row offers to do.
ui-column-actions = Actions

# The button that starts a plan's backup, named with its plan, so each of the
# page's start buttons says which plan it runs.
# $plan (text): the plan's ID, as config.toml names it. It is shown as it came.
ui-start-plan = Start backup of { $plan }

# The button that cancels a plan's running or queued backup, named with its
# row's plan. It begins with the button's visible text, ui-cancel.
ui-cancel-plan = Cancel { $plan }

# The visible text of a row's Cancel button, one short word, so the button takes
# one line in the narrow Actions column. Its name (ui-cancel-plan,
# ui-cancel-operation-of or ui-cancel-operation) begins with it.
ui-cancel = Cancel

# The row header of a maintenance run, a restore or a download: what ran and
# the repository it ran on or read, by the alias this machine gave the repository
# when the run was first logged.
# $kind is the wire text of what ran: forget, prune, check, adopt, restore or
# download; any other kind is an operation.
# $repository (text): the repository's alias. It is shown as it came.
ui-operation-of =
    { $kind ->
        [forget] Forget in { $repository }
        [prune] Prune of { $repository }
        [check] Check of { $repository }
        [adopt] Adoption in { $repository }
        [restore] Restore from { $repository }
        [download] Download from { $repository }
       *[other] Operation on { $repository }
    }

# The row header of a run the log names no plan or repository for, such as a
# maintenance run or a restore an earlier version logged: what ran.
# $kind is the wire text of what ran: backup, forget, prune, check, adopt,
# restore or download; any other kind is an operation.
ui-operation =
    { $kind ->
        [backup] Backup
        [forget] Forget
        [prune] Prune
        [check] Check
        [adopt] Adoption
        [restore] Restore
        [download] Download
       *[other] Operation
    }

# The button that cancels a running or queued maintenance run, restore or
# download, named as its row is, so each Cancel says which run it stops. It
# begins with ui-cancel.
# $kind is the wire text of what runs: forget, prune, check, adopt, restore or
# download; any other kind is an operation.
# $repository (text): the repository's alias. It is shown as it came.
ui-cancel-operation-of =
    { $kind ->
        [forget] Cancel the forget in { $repository }
        [prune] Cancel the prune of { $repository }
        [check] Cancel the check of { $repository }
        [adopt] Cancel the adoption in { $repository }
        [restore] Cancel the restore from { $repository }
        [download] Cancel the download from { $repository }
       *[other] Cancel the operation on { $repository }
    }

# The button that cancels a running or queued run the log names no plan or
# repository for. It begins with ui-cancel.
# $kind is the wire text of what runs: backup, forget, prune, check, adopt,
# restore or download; any other kind is an operation.
ui-cancel-operation =
    { $kind ->
        [backup] Cancel the backup
        [forget] Cancel the forget
        [prune] Cancel the prune
        [check] Cancel the check
        [adopt] Cancel the adoption
        [restore] Cancel the restore
        [download] Cancel the download
       *[other] Cancel the operation
    }

# Announced politely on the Operations page when a run it watched ended well,
# warnings or not. $kind is the wire text of what ran: backup, forget, prune,
# check, adopt, restore or download; any other kind is an operation.
ui-operation-finished =
    { $kind ->
        [backup] Backup finished.
        [forget] Forget finished.
        [prune] Prune finished.
        [check] Check finished.
        [adopt] Adoption finished.
        [restore] Restore finished.
        [download] Download finished.
       *[other] Operation finished.
    }

# Announced politely on the Operations page when a run it watched was
# cancelled. $kind is the wire text of what ran: backup, forget, prune, check,
# adopt, restore or download; any other kind is an operation.
ui-operation-cancelled =
    { $kind ->
        [backup] Backup cancelled.
        [forget] Forget cancelled.
        [prune] Prune cancelled.
        [check] Check cancelled.
        [adopt] Adoption cancelled.
        [restore] Restore cancelled.
        [download] Download cancelled.
       *[other] Operation cancelled.
    }

# Announced assertively on the Operations page when a run it watched ended
# badly. $kind is the wire text of what ran: backup, forget, prune, check,
# adopt, restore or download; any other kind is an operation.
ui-operation-failed =
    { $kind ->
        [backup] Backup failed.
        [forget] Forget failed.
        [prune] Prune failed.
        [check] Check failed.
        [adopt] Adoption failed.
        [restore] Restore failed.
        [download] Download failed.
       *[other] Operation failed.
    }

# The row header of a restore the log kept the entry and the target of: the
# entry it restored, the repository it read and where it went (#813).
# $entry (text): the entry's path in the snapshot, such as /home/docs. It is shown as it came.
# $repository (text): the repository's alias. It is shown as it came.
# $to (text): in-place when the restore wrote over the entry's original path, or folder when it wrote into a new folder.
# $folder (text): the new folder's path, for folder only. It is shown as it came.
ui-operation-restore-of =
    { $to ->
        [in-place] In-place restore of { $entry } from { $repository }
       *[folder] Restore of { $entry } from { $repository } to { $folder }
    }

# The button that cancels a running or queued restore the log kept the entry
# and the target of, named as its row is (ui-operation-restore-of). It begins
# with ui-cancel.
# $entry (text): the entry's path in the snapshot. It is shown as it came.
# $repository (text): the repository's alias. It is shown as it came.
# $to (text): in-place or folder, as for ui-operation-restore-of.
# $folder (text): the new folder's path, for folder only. It is shown as it came.
ui-cancel-operation-restore-of =
    { $to ->
        [in-place] Cancel the in-place restore of { $entry } from { $repository }
       *[folder] Cancel the restore of { $entry } from { $repository } to { $folder }
    }

# Announced politely on the Operations page when a restore it watched, which
# the log kept the entry and the target of, ended well, warnings or not.
# $entry (text): the entry's path in the snapshot. It is shown as it came.
# $to (text): in-place or folder, as for ui-operation-restore-of.
# $folder (text): the new folder's path, for folder only. It is shown as it came.
ui-operation-restore-finished =
    { $to ->
        [in-place] In-place restore of { $entry } finished.
       *[folder] Restore of { $entry } to { $folder } finished.
    }

# Announced politely on the Operations page when a restore it watched, which
# the log kept the entry and the target of, was cancelled.
# $entry (text): the entry's path in the snapshot. It is shown as it came.
# $to (text): in-place or folder, as for ui-operation-restore-of.
# $folder (text): the new folder's path, for folder only. It is shown as it came.
ui-operation-restore-cancelled =
    { $to ->
        [in-place] In-place restore of { $entry } cancelled.
       *[folder] Restore of { $entry } to { $folder } cancelled.
    }

# Announced assertively on the Operations page when a restore it watched,
# which the log kept the entry and the target of, ended badly.
# $entry (text): the entry's path in the snapshot. It is shown as it came.
# $to (text): in-place or folder, as for ui-operation-restore-of.
# $folder (text): the new folder's path, for folder only. It is shown as it came.
ui-operation-restore-failed =
    { $to ->
        [in-place] In-place restore of { $entry } failed.
       *[folder] Restore of { $entry } to { $folder } failed.
    }

# One fact of a finished backup's result: the snapshot it wrote, by the first
# 8 hex digits of its ID, as rustic names a snapshot.
# $snapshot (text): those digits. They are shown as they came.
ui-result-snapshot = Snapshot { $snapshot }

# One fact of a finished backup's result: the files it went through, new,
# changed or unmodified.
# $files (count): how many.
ui-result-files =
    { $files ->
        [one] { $files } file
       *[other] { $files } files
    }

# One fact of a finished backup's result: the bytes of the source it went
# through, changed or not.
# $bytes (bytes): how much, as a size such as 1 MiB.
ui-result-processed = { $bytes } processed

# One fact of a finished backup's result: the new data it added to the
# repository, before compression.
# $bytes (bytes): how much, as a size such as 1 MiB.
ui-result-added = { $bytes } added

# One fact of a finished prune's result: the packs it deleted.
# $packs (count): how many.
ui-result-packs-deleted =
    { $packs ->
        [one] { $packs } pack deleted
       *[other] { $packs } packs deleted
    }

# One fact of a finished check's result, when it found no error and nothing
# to tidy.
ui-result-check-clean = The check found nothing wrong

# One fact of a finished check's result: the errors it found.
# $errors (count): how many.
ui-result-check-errors =
    { $errors ->
        [one] { $errors } error found
       *[other] { $errors } errors found
    }

# One fact of a finished check's result: the things to tidy it found.
# $warnings (count): how many.
ui-result-check-untidy =
    { $warnings ->
        [one] { $warnings } thing to tidy
       *[other] { $warnings } things to tidy
    }

# One fact of a finished forget's result: the snapshots it removed.
# $removed (count): how many.
ui-result-removed =
    { $removed ->
        [one] { $removed } snapshot removed
       *[other] { $removed } snapshots removed
    }

# One fact of a finished restore's result: the files it wrote, new or
# overwritten.
# $files (count): how many.
ui-result-restored =
    { $files ->
        [one] { $files } file restored
       *[other] { $files } files restored
    }

# One fact of a finished restore's result: the bytes it wrote.
# $bytes (bytes): how much, as a size such as 1 MiB.
ui-result-written = { $bytes } written

# One fact of a finished adoption's result: the snapshots it rewrote into
# the plan.
# $rewritten (count): how many.
ui-result-adopted =
    { $rewritten ->
        [one] { $rewritten } snapshot adopted
       *[other] { $rewritten } snapshots adopted
    }

# One fact of a finished run's result: how long it ran.
# $duration (duration): that length of time, such as 1 min 5 s.
ui-result-took = Took { $duration }

# One fact of a run on a repository reached at several locations: the location
# it used, and its address as the run recorded it.
# $position (count): the location's place in the repository's order, from 1.
# $count (count): how many locations the repository had then, at least 2.
# $address (text): the location's address, with any credentials in it masked. It is shown as it came.
ui-result-used-location = Used location { $position } of { $count }: { $address }

# One fact of a restore or a download: the entry of the snapshot it read, as
# its path from the snapshot's top.
# $entry (text): that path, such as /home/docs, with any secret in it masked; a long one keeps its end after "…". It is shown as it came.
ui-result-entry = Entry { $entry }

# The button in the operations header that re-sorts the rows.
ui-sort-by-started = Sort by started

# Announced politely when the rows are sorted newest first.
ui-sorted-newest = Sorted by started, newest first.

# Announced politely when the rows are sorted oldest first.
ui-sorted-oldest = Sorted by started, oldest first.

# What the operations page says when the daemon holds no operations.
ui-no-operations = No operations yet.

# Shown beside a progress bar in place of its percentage while the
# operation has no total to measure against: a backup before it has
# walked its whole source, or a step that counts without a total.
ui-progress-working = Working…

# Shown below the words of a progress bar that has no total to measure
# against, once its step counts, so a reader can tell a run that is still
# reporting from one that has gone quiet. It is an exact instant, never "a
# minute ago", so it changes only when a new report arrives.
# $when (instant): when the daemon last heard the run report, in the daemon's clock.
ui-progress-reported = Last report at { DATETIME($when, dateStyle: "short", timeStyle: "medium") }

# What a screen reader hears of a progress bar that has no total to measure
# against and knows when its run last reported. It changes at most every 30
# seconds.
# $step (text): the step's own message, such as "Backing up…", or "Working…" when the operation names no step.
# $when (instant): when the daemon last heard the run report, in the daemon's clock.
ui-progress-unknown-value = { $step } Last report at { DATETIME($when, dateStyle: "short", timeStyle: "medium") }

# Shown below the words of a progress bar whose step has counted nothing yet:
# such a step reports only as it begins and ends, so this says how long the
# run has been in it, not when it last reported. It is an exact instant,
# never "for 3 minutes", so it changes only when the step does.
# $when (instant): when the daemon heard the step begin, in the daemon's clock.
ui-progress-since = Since { DATETIME($when, dateStyle: "short", timeStyle: "medium") }

# What a screen reader hears of a progress bar whose step has counted nothing
# yet: the step, then since when the run has been in it.
# $step (text): the step's own message, such as "Listing the packs…", or "Working…" when the operation names no step.
# $when (instant): when the daemon heard the step begin, in the daemon's clock.
ui-progress-since-value = { $step } Since { DATETIME($when, dateStyle: "short", timeStyle: "medium") }

# The name of a running backup's progress bar, named as its row is, by its
# plan, so two running backups' bars are told apart and a check's bar is
# never announced as a backup's.
# $plan (text): the plan's ID, as config.toml names it. It is shown as it came.
ui-plan-progress = Backup progress of { $plan }

# The name of a running maintenance run's, restore's or download's progress
# bar: what it measures and the repository it runs on or reads, by the alias
# this machine gave the repository when the run was first logged, as its row is
# named.
# $kind is the wire text of what runs: forget, prune, check, adopt, restore or
# download; any other kind is an operation.
# $repository (text): the repository's alias. It is shown as it came.
ui-operation-progress-of =
    { $kind ->
        [forget] Forget progress in { $repository }
        [prune] Prune progress of { $repository }
        [check] Check progress of { $repository }
        [adopt] Adoption progress in { $repository }
        [restore] Restore progress from { $repository }
        [download] Download progress from { $repository }
       *[other] Operation progress on { $repository }
    }

# The name of the progress bar of a run the log names no plan or repository
# for, such as one an earlier version logged: what it measures.
# $kind is the wire text of what runs: backup, forget, prune, check, adopt,
# restore or download; any other kind is an operation.
ui-operation-progress =
    { $kind ->
        [backup] Backup progress
        [forget] Forget progress
        [prune] Prune progress
        [check] Check progress
        [adopt] Adoption progress
        [restore] Restore progress
        [download] Download progress
       *[other] Operation progress
    }

# The name of a running restore's progress bar when the log kept its entry
# and its target, as its row is named (ui-operation-restore-of).
# $entry (text): the entry's path in the snapshot. It is shown as it came.
# $repository (text): the repository's alias. It is shown as it came.
# $to (text): in-place or folder, as for ui-operation-restore-of.
# $folder (text): the new folder's path, for folder only. It is shown as it came.
ui-operation-progress-restore-of =
    { $to ->
        [in-place] In-place restore progress of { $entry } from { $repository }
       *[folder] Restore progress of { $entry } from { $repository } to { $folder }
    }

# What a screen reader reads as the value of a progress bar that shows a
# percentage (§4.5): the step, how far, how much of how much, and the time
# left once it can be told. It changes at most every 30 seconds.
# $step (text): the step's own message, such as "Backing up…", or none when the operation names no step.
# $percent (number): how far, as a whole percent.
# $done (number or size): how much is done; a size such as 3.1 GiB when the bar counts bytes, a plain number when it counts things.
# $total (number or size): the whole, in the same unit as $done.
# $left (text): how the time left is told: none (not known yet), soon (under a minute), minutes or hours.
# $minutes (number): the whole minutes left, for minutes.
# $hours (number): the whole hours left, for hours.
ui-progress-value =
    { $step ->
        [none] { NUMBER($percent) }%, { $done } of { $total }
       *[other] { $step } { NUMBER($percent) }%, { $done } of { $total }
    }{ $left ->
        [soon] , less than a minute left
        [minutes] , about { $minutes ->
            [one] { $minutes } minute
           *[other] { $minutes } minutes
        } left
        [hours] , about { $hours ->
            [one] { $hours } hour
           *[other] { $hours } hours
        } left
       *[none] {""}
    }

## The step a running operation is in (§4.4; decision 82), beside its
## progress bar in place of the percentage, or above a bar that shows one.
## Each says what the engine is doing now.

# Reading the index, which lists what each pack holds.
ui-step-reading-index = Reading the { -index }…

# Reading the snapshots of the repository.
ui-step-getting-snapshots = Reading the { -snapshot(form: "other") }…

# Reading the backup's source and writing what is new.
ui-step-backing-up = Backing up…

# Listing the packs the repository holds.
ui-step-listing-packs = Listing the { -pack(form: "other") }…

# Comparing the machine's cache of the repository with the repository.
ui-step-checking-cache = Checking the cache of the { -repository }…

# Comparing a repository's hot copy with the repository itself.
ui-step-checking-hot-repository = Checking the hot copy of the { -repository }…

# A check reading what each snapshot holds.
ui-step-checking-trees = Checking what each { -snapshot } holds…

# A check reading all the data to check it can be read.
ui-step-reading-pack-data = Reading the data in each { -pack }…

# A prune finding which data the snapshots still use.
ui-step-finding-used-data = Finding the data the { -snapshot(form: "other") } use…

# A prune marking packs no index lists, to delete them later.
ui-step-marking-unused-packs = Marking unused { -pack(form: "other") } for deletion…

# A prune moving the data still in use out of packs it will delete.
ui-step-repacking = Moving the data still in use into new { -pack(form: "other") }…

# A prune writing the index anew.
ui-step-rebuilding-index = Rebuilding the { -index }…

# A prune deleting packs and index files no longer needed.
ui-step-deleting-unused-data = Deleting data no longer needed…

# A forget or an adoption deleting snapshots.
ui-step-deleting-snapshots = Deleting { -snapshot(form: "other") }…

# An adoption writing the snapshots it made.
ui-step-saving-snapshots = Saving the new { -snapshot(form: "other") }…

# A status label: the operation waits for its turn.
ui-status-queued = Queued

# A status label: the operation runs.
ui-status-running = Running

# A status label: the operation ended well.
ui-status-succeeded = Succeeded

# A status label: the operation ended well, with warnings.
ui-status-succeeded-with-warnings = Succeeded with warnings

# A status label: the operation ended badly.
ui-status-failed = Failed

# A status label: the operation was cancelled.
ui-status-cancelled = Cancelled

# Announced politely when a backup has been asked for.
ui-backup-started = Backup started.

# An instant, as the page's locale writes it.
ui-instant = { DATETIME($when, dateStyle: "short", timeStyle: "medium") }

# When a listed snapshot was taken, as the clock that took it read it, with
# that clock's offset from UTC after it: shown where the viewer's clock was
# at another offset then (§16, decision 56). `when` is the reading, already
# written as the page's locale writes `ui-instant`; `offset` is the offset as
# RFC 3339 writes one ("-06:00"). "UTC" is a string literal: a locale may
# name the reference otherwise ("GMT"), and a pseudolocale leaves it as it is.
ui-snapshot-time-offset = { $when } { "UTC" }{ $offset }

# The heading a live page shows when the daemon cannot be reached at all.
ui-unreachable-heading = Cannot reach { -brand-name }

# What that page says about its state.
ui-unreachable-body = The daemon that serves this page is not answering. It comes back on its own.

# What a live page says while its first answer is still on its way.
ui-loading = Loading…

## The snapshots page: a repository's history grouped by owner, newest first,
## and the maintenance that changes it, each run reviewed before it starts (§4.4).

# The snapshots page's title, as the browser's tab and history show it.
ui-snapshots-title = Snapshots – { -brand-name }

# The snapshots page's heading: its one main heading.
ui-snapshots-heading = Snapshots

# The snapshots page's text, until a repository is chosen.
ui-snapshots-body = The snapshots of a repository, grouped by the plan that owns them, newest first.

# The snapshots table's column of snapshot IDs.
ui-column-snapshot = Snapshot

# The snapshots table's column of the instants snapshots were taken.
ui-column-time = Time

# The button that shows one repository's snapshots, named with the repository.
ui-snapshots-of = Snapshots of { $alias }

# What the snapshots page says when the repository holds no snapshots.
ui-no-snapshots = No snapshots yet.

# The heading of the section of a repository's maintenance, named with the repository.
ui-maintenance-heading = Maintenance of { $alias }

# What the maintenance section says before its buttons.
ui-maintenance-body = Each of these shows what it would do first. Nothing changes until you confirm.

# What the maintenance section says instead, with no buttons but the one that turns maintenance from this machine on, of a repository this machine does not maintain: its entry asks for maintenance by the hub or by nothing.
ui-maintenance-elsewhere = This { -machine } does not maintain this repository, so it cannot forget, prune or check it, or adopt history into its plans.

# A warning above the maintenance section's buttons when this paired machine holds no lease on the repository from its hub, so the hub has it maintain nothing (technical design §7.3). It goes once the hub grants a lease.
# $since (instant): since when maintenance is paused, read in the viewer's zone.
ui-maintenance-paused = Maintenance from this { -machine } has been paused since { DATETIME($since, dateStyle: "medium") }, because it holds no lease on this repository from its { -hub }. It starts again once the { -hub } grants one.

# The button that reviews a forget: the snapshots the plans' retention no longer keeps.
ui-maintenance-forget = Forget snapshots

# The button that reviews a prune: the data no snapshot uses.
ui-maintenance-prune = Prune unused data

# The button that reviews a check of the repository.
ui-maintenance-check = Check the repository

# The button after a repository's maintenance that asks this machine to maintain it, which first opens a dialog that says what that means.
ui-maintainer-on = Maintain from this { -machine }

# The button after a repository's maintenance that asks this machine to stop maintaining it, which first opens a dialog that says what stops.
ui-maintainer-off = Stop maintaining from this { -machine }

# The title of the dialog that asks before this machine maintains a repository.
# $alias (text): the repository's alias, such as demo.
ui-maintainer-on-title = Maintain { $alias } from this { -machine }

# What that dialog says: what maintaining means, and why to leave it off. Nothing can tell whether another machine maintains the repository, so the person is told that only one place may.
# $alias (text): the repository's alias, such as demo.
ui-maintainer-on-body = This { -machine } will forget, prune and check { $alias }, and adopt history into its plans. Only one place may maintain a repository, so leave this off when another { -machine } or the { -hub } already does.

# The button in that dialog that turns maintenance from this machine on.
ui-maintainer-on-confirm = Turn on maintenance

# The title of the dialog that asks before this machine stops maintaining a repository.
# $alias (text): the repository's alias, such as demo.
ui-maintainer-off-title = Stop maintaining { $alias } from this { -machine }

# What that dialog says: what stops, and that backups go on while nothing removes old snapshots.
# $alias (text): the repository's alias, such as demo.
ui-maintainer-off-body = This { -machine } will stop forgetting, pruning and checking { $alias }, and adopting history into its plans. Backups go on, but nothing removes old snapshots until another place maintains it.

# The button in that dialog that turns maintenance from this machine off.
ui-maintainer-off-confirm = Turn off maintenance

# Said in either dialog while the daemon saves the change.
ui-maintainer-saving = Saving the change…

# Announced once this machine maintains the repository.
# $alias (text): the repository's alias, such as demo.
ui-maintainer-on-done = This { -machine } now maintains { $alias }.

# Announced once this machine no longer maintains the repository.
# $alias (text): the repository's alias, such as demo.
ui-maintainer-off-done = This { -machine } no longer maintains { $alias }.

# What the maintenance section shows while a review is being prepared.
ui-maintenance-preparing = Preparing the review…

# What the maintenance section shows, and the review that confirmed it says, while the start of a run is on its way. $kind is the wire text of what starts: forget, prune, check or adopt.
ui-maintenance-starting =
    { $kind ->
        [forget] Starting the forget…
        [prune] Starting the prune…
        [check] Starting the check…
       *[adopt] Starting the adoption…
    }

# What the maintenance section says while a run goes on. $kind is the wire text of what runs: forget, prune, check or adopt.
ui-maintenance-running =
    { $kind ->
        [forget] A forget is running.
        [prune] A prune is running.
        [check] A check is running.
       *[adopt] An adoption is running.
    }

# The button that asks the run that goes on to stop.
ui-maintenance-cancel = Cancel the run

# Said politely, and shown, when a run finished. $kind is the wire text of what ran: forget, prune, check or adopt. A check that says this found nothing.
ui-maintenance-finished =
    { $kind ->
        [forget] Forget finished.
        [prune] Prune finished.
        [check] Check finished. It found nothing wrong.
       *[adopt] Adoption finished.
    }

# Said politely, and shown, when a check ran to its end and found only things to tidy, which it lists below this line; with no error found, no snapshot's data depends on them. $warnings (count): how many it found.
ui-maintenance-check-untidy =
    { $warnings ->
        [one] Check finished. It found one thing to tidy, listed below, and no errors.
       *[other] Check finished. It found { $warnings } things to tidy, listed below, and no errors.
    }

# Said assertively, and shown, when a check ran to its end and found errors, which it lists below this line with what else it found. An error is damage in the repository, or a part of the repository, or of this machine's copy of it, that could not be read or did not match. $errors (count): how many errors it found.
ui-maintenance-check-errors =
    { $errors ->
        [one] Check finished. It found an error, listed below.
       *[other] Check finished. It found { $errors } errors, listed below.
    }

# What follows the list of what a check found when the list leaves some out, a sentence of its own; $count is how many it leaves out.
ui-maintenance-check-more =
    { $count ->
        [one] The check found 1 more that is not listed.
       *[other] The check found { $count } more that are not listed.
    }

# Said politely, and shown, when a run was cancelled. $kind is the wire text of what ran: forget, prune, check or adopt.
ui-maintenance-cancelled =
    { $kind ->
        [forget] Forget cancelled.
        [prune] Prune cancelled.
        [check] Check cancelled.
       *[adopt] Adoption cancelled.
    }

# Said assertively, and shown, when a run failed. $kind is the wire text of what ran: forget, prune, check or adopt.
ui-maintenance-failed =
    { $kind ->
        [forget] Forget failed.
        [prune] Prune failed.
        [check] Check failed.
       *[adopt] Adoption failed.
    }

# Said politely, and shown, when this page stopped watching a run that goes on. $kind is the wire text of what runs: forget, prune, check or adopt.
ui-maintenance-unwatched =
    { $kind ->
        [forget] This page stopped watching the forget, which goes on. The Operations page shows how it ends.
        [prune] This page stopped watching the prune, which goes on. The Operations page shows how it ends.
        [check] This page stopped watching the check, which goes on. The Operations page shows how it ends.
       *[adopt] This page stopped watching the adoption, which goes on. The Operations page shows how it ends.
    }

# The heading of the section of one plan's snapshots.
ui-snapshots-plan = Plan { $plan }

# The caption of the table of one plan's snapshots.
ui-snapshots-of-plan = Snapshots of plan { $plan }

# What follows a list that shows only the newest of its snapshots, a sentence of its own; $count is how many it leaves out.
ui-snapshots-more =
    { $count ->
        [one] 1 more snapshot is not listed.
       *[other] { $count } more snapshots are not listed.
    }

## A snapshot's page: its tree of folders and files, each folder read as it
## is opened (§4.4, §4.5).

# A snapshot's page's title, as the browser's tab and history show it.
ui-snapshot-title = Snapshot – { -brand-name }

# A snapshot's page's heading: its one main heading.
# $snapshot (text): the snapshot's short ID, its first 8 hexadecimal digits.
# $alias (text): the repository's alias, such as demo.
ui-snapshot-heading = Snapshot { $snapshot } of { $alias }

# A snapshot's page's text above its tree, saying how to use it.
ui-snapshot-body = The folders and files the { -snapshot } holds, in the order it records them. Tab moves into the tree, where the arrow keys move from row to row and open and close folders, Home and End go to the first and last rows, and typing the first letters of a name goes to it.

# The name of a snapshot's tree.
ui-snapshot-tree = Folders and files

# What a snapshot's page says in place of a tree when the snapshot holds nothing.
ui-snapshot-empty = The { -snapshot } holds no folders or files.

# The link from a snapshot's page back to the snapshots page.
ui-snapshot-back = Back to the snapshots

# What a row of a snapshot's tree says after its name, on screen and to a
# screen reader, when the snapshot recorded no modification time.
# $kind (text): what the entry is, and how far a folder is read: folder,
# loading (a folder whose entries are on their way), empty (a folder read,
# holding nothing), failed (a folder that could not be read), file, link or
# other (a device, a pipe or a socket).
# $size (bytes): a file's size, such as 2 KiB.
ui-tree-entry =
    { $kind ->
        [folder] Folder
        [loading] Folder, loading
        [empty] Folder, empty
        [failed] Folder that could not be read
        [file] File, { $size }
        [link] Link
       *[other] Device, pipe or socket
    }

# What a row of a snapshot's tree says after its name, with when the entry
# was last modified. $kind and $size are as in ui-tree-entry.
# $modified (text): when, as the page writes an instant.
ui-tree-entry-modified =
    { $kind ->
        [folder] Folder, modified { $modified }
        [loading] Folder, loading, modified { $modified }
        [empty] Folder, empty, modified { $modified }
        [failed] Folder that could not be read, modified { $modified }
        [file] File, { $size }, modified { $modified }
        [link] Link, modified { $modified }
       *[other] Device, pipe or socket, modified { $modified }
    }

# The last row of a folder in a snapshot's tree while more of it is to read:
# activating it reads the next entries.
# $shown (count): how many of the folder's entries are shown.
# $total (count): how many it holds.
ui-tree-more = Show more: { $shown } of { $total } shown

# The heading of the section of history that no plan of this machine owns.
ui-orphaned-heading = Orphaned history

# What the orphaned section says about its groups.
ui-orphaned-body = No plan of this { -machine } owns these snapshots now. A forget leaves them alone, and adopting a group rewrites it into a plan.

# The heading of an orphaned group. $owner is both when the group carries the tags of a machine and a plan, plan or agent when it carries one of them, and neither when it carries none; $plan and $agent are the tags, $agent the machine's.
ui-orphaned-group =
    { $owner ->
        [both] Plan { $plan } of { -machine } { $agent }
        [plan] Plan { $plan } of an unknown { -machine }
        [agent] An unknown plan of { -machine } { $agent }
       *[neither] History of an unknown { -machine } and plan
    }

# The caption of the table of an orphaned group's snapshots; $owner, $plan and $agent are as in the group's heading.
ui-orphaned-caption =
    { $owner ->
        [both] Snapshots of plan { $plan } of { -machine } { $agent }
        [plan] Snapshots of plan { $plan } of an unknown { -machine }
        [agent] Snapshots of an unknown plan of { -machine } { $agent }
       *[neither] Snapshots of an unknown { -machine } and plan
    }

# The button that adopts an orphaned group, named by its machine and plan, into a plan of this machine.
ui-adopt-orphaned = Adopt history of plan { $plan } from { -machine } { $agent } into plan { $into }

# What an orphaned group says when it lacks the tag of a machine or of a plan, which an adoption names.
ui-orphaned-untagged = This history lacks the tag of a { -machine } or a plan, so it cannot be adopted.

# What an orphaned group says when no plan of this machine could take it.
ui-orphaned-no-plan = No plan of this { -machine } could take this history, so it cannot be adopted.

# The heading of the section of history that no machine tagged.
ui-unmanaged-heading = Unmanaged history

# What the unmanaged section says about its groups.
ui-unmanaged-body = No { -machine } tagged these snapshots. A forget leaves them alone, and a plan that backs up the same host, label and paths can adopt them.

# The heading of an unmanaged group. $labeled is yes when the group has a label and no when it has none; $hostname is its host and $label its label.
ui-unmanaged-group =
    { $labeled ->
        [yes] Host { $hostname }, label { $label }
       *[no] Host { $hostname }, no label
    }

# What introduces the list of the paths an unmanaged group backed up.
ui-unmanaged-paths = Paths:

# The caption of the table of an unmanaged group's snapshots; $labeled, $hostname and $label are as in the group's heading.
ui-unmanaged-caption =
    { $labeled ->
        [yes] Snapshots of host { $hostname }, label { $label }
       *[no] Snapshots of host { $hostname }, no label
    }

# The button that adopts an unmanaged group into the plan that backs up the same things; $labeled, $hostname and $label are as in the group's heading, $into is the plan.
ui-adopt-unmanaged =
    { $labeled ->
        [yes] Adopt history of host { $hostname }, label { $label } into plan { $into }
       *[no] Adopt history of host { $hostname }, no label into plan { $into }
    }

# What an unmanaged group says when no plan of this machine could adopt it.
ui-unmanaged-no-plan = No plan of this { -machine } backs up this host, label and paths, so this history cannot be adopted.

# The title of the review of a forget, named with the repository.
ui-review-title-forget = Forget snapshots of { $alias }

# The title of the review of a prune, named with the repository.
ui-review-title-prune = Prune { $alias }

# The title of the review of a check, named with the repository.
ui-review-title-check = Check { $alias }

# The title of the review of an adoption, named with the plan that takes the history.
ui-review-title-adopt = Adopt history into { $plan }

# The button that confirms a forget; $count is how many snapshots it removes.
ui-review-confirm-forget =
    { $count ->
        [one] Forget 1 snapshot
       *[other] Forget { $count } snapshots
    }

# The button that confirms an adoption; $count is how many snapshots it rewrites.
ui-review-confirm-adopt =
    { $count ->
        [one] Adopt 1 snapshot
       *[other] Adopt { $count } snapshots
    }

# The button that confirms a prune.
ui-review-confirm-prune = Start prune

# The button that confirms a check.
ui-review-confirm-check = Start check

# What a forget does to the snapshots of one plan; $remove of its $total go.
ui-review-removes =
    { $remove ->
        [one]
            { $total ->
                [one] Plan { $plan }: its 1 snapshot is forgotten.
               *[other] Plan { $plan }: 1 of its { $total } snapshots is forgotten.
            }
       *[other] Plan { $plan }: { $remove } of its { $total } snapshots are forgotten.
    }

# What a forget does to a plan whose retention lets none of its $total snapshots go.
ui-review-removes-none =
    { $total ->
        [one] Plan { $plan }: nothing is forgotten among its 1 snapshot.
       *[other] Plan { $plan }: nothing is forgotten among its { $total } snapshots.
    }

# What a forget says when no plan of this machine backs up to the repository.
ui-review-no-plans = No plan of this { -machine } backs up to this repository, so a forget has nothing to keep or remove.

# What a forget leaves alone when it finds only orphaned history; $snapshots counts the snapshots.
ui-review-leaves-orphaned = Left alone: orphaned snapshots ({ $snapshots }), which no plan of this { -machine } owns.

# What a forget leaves alone when it finds only unmanaged history; $snapshots counts the snapshots.
ui-review-leaves-unmanaged = Left alone: unmanaged snapshots ({ $snapshots }), which no { -machine } tagged.

# What a forget leaves alone when it finds both kinds of history, counted by $orphaned and $unmanaged.
ui-review-leaves-both = Left alone: orphaned snapshots ({ $orphaned }), which no plan of this { -machine } owns, and unmanaged snapshots ({ $unmanaged }), which no { -machine } tagged.

# What an adoption does; $snapshots is how many snapshots become part of the plan $into.
ui-review-adopts =
    { $snapshots ->
        [one] 1 snapshot becomes part of plan { $into }. It gets a new ID and keeps its tree.
       *[other] { $snapshots } snapshots become part of plan { $into }. They get new IDs and keep their trees.
    }

# What an adoption says when no snapshot is left to adopt into the plan $into.
ui-review-adopts-none = No snapshots are left to adopt into plan { $into }.

# What a prune's dry run found. $marked counts the packs marked as unused now, $deleted the packs deleted now (marked earlier), $recovered the packs a snapshot uses again, which the prune marks for recovery or brings back once they have been marked for recovery long enough; $bytes is the size the deleted packs free.
ui-review-prunes =
    Marked as unused now: { $marked ->
        [one] 1 { -pack }
       *[other] { $marked } { -pack(form: "other") }
    }. Deleted now, marked earlier: { $deleted ->
        [one] 1 { -pack }
       *[other] { $deleted } { -pack(form: "other") }
    }, freeing { $bytes }. Used by a { -snapshot } again, marked for recovery now or brought back now: { $recovered ->
        [one] 1 { -pack }
       *[other] { $recovered } { -pack(form: "other") }
    }.

# How long the packs that are marked now wait for a later prune to delete them, in whole days; $days counts them.
ui-review-prune-wait-days =
    { $days ->
        [one] A later prune deletes the { -pack(form: "other") } marked now, after 1 day.
       *[other] A later prune deletes the { -pack(form: "other") } marked now, after { $days } days.
    }

# How long the packs that are marked now wait for a later prune to delete them, in hours; $hours counts them.
ui-review-prune-wait-hours =
    { $hours ->
        [one] A later prune deletes the { -pack(form: "other") } marked now, after 1 hour.
       *[other] A later prune deletes the { -pack(form: "other") } marked now, after { $hours } hours.
    }

# What a check reads when it looks at the repository's structure only.
ui-review-checks-structure = The check reads the structure of the repository and changes nothing. It holds the repository while it runs.

# What a check reads when it also reads all the data.
ui-review-checks-data = The check reads the structure and all the data of the repository, which takes longer, and changes nothing. It holds the repository while it runs.

# The caption of the table of the snapshots a forget removes from the plan $plan.
ui-review-forget-caption = Snapshots forgotten from plan { $plan }

# The caption of the table of the snapshots an adoption rewrites into the plan $plan.
ui-review-adopt-caption = Snapshots adopted into plan { $plan }

# Announced politely when the settle-poll after a start or a cancel ran out
# of its budget without the store ever saying how the run ended.
ui-still-waiting = Still waiting for Ladderback to say how the run ended.

## The link (§4.5): what it says of itself, the same on every page.

# What a link to another site says after its own words, heard by a screen
# reader and not shown: an icon shows it. It begins with the space between
# it and the link's words, { " " }, which a language that writes none there
# leaves out. A whole phrase of its own, never part of the link's words.
ui-link-outside = { " " }(goes to another site)

## The alert (§4.5): what it says of itself, the same on every page.

# The name of an alert's icon, heard by a screen reader before the alert's
# words: the alert's tone in one word, which its icon's shape shows.
ui-alert-information = Information
ui-alert-success = Success
ui-alert-warning = Warning
ui-alert-error = Error

# The name of the icon-only button that takes an alert away, which its
# tooltip shows.
ui-alert-dismiss = Dismiss

## The text field (§4.5): what it says of itself, the same on every page.

# The marker after a required text field's label, which says it must be
# filled in. A screen reader skips it: the field says it is required itself.
# It begins with the space between it and the label, { " " }, which a
# language that writes none there leaves out.
ui-text-field-required = { " " }(required)

# The name of the toggle button beside a password field, pressed while the
# password is shown. It keeps this one name pressed or not.
ui-text-field-show-password = Show password

# Under a read-only text field's label: it can be read and copied, not
# changed.
ui-text-field-read-only = Read only: you can select and copy it, but not change it.

# Under a disabled text field's label: it can't be used now. The field's
# own hint says why, where there is a reason.
ui-text-field-disabled = Unavailable: it can't be changed now.

## The banner (§4.5): what it says of itself, the same on every page.

# The name of a banner's icon, heard by a screen reader before the banner's
# words: the banner's tone in one word, which its icon's shape shows.
ui-banner-information = Information
ui-banner-warning = Warning

# The name of the icon-only button that takes a banner away, which its
# tooltip shows. The browser remembers the choice.
ui-banner-dismiss = Dismiss

## The steps (§4.5): what a list of a process's steps says of each step, and
## what is said when the process reaches another, the same on every page.

# A step's state, in words under its name, which its icon's shape shows too:
# the process has gone past it, is at it now, hasn't reached it yet, or
# stopped at it, failed.
ui-steps-done = Done
ui-steps-current = Current
ui-steps-to-do = To do
ui-steps-failed = Failed

# Said politely when a process reaches another of its steps, such as the
# copy of a hub move.
# $number (count): the step's place in the process, from 1.
# $total (count): how many steps the process has, at least 1.
# $name (text): the step's name, as its page shows it.
ui-steps-reached = Step { $number } of { $total }: { $name }

# Said politely when a step of a process fails, which stops the process
# there.
# $number (count): the failed step's place in the process, from 1.
# $total (count): how many steps the process has, at least 1.
# $name (text): the failed step's name, as its page shows it.
ui-steps-failed-at = Step { $number } of { $total } failed: { $name }

# Said politely when a process has done every one of its steps.
# $total (count): how many steps the process has, at least 1.
ui-steps-finished =
    { $total ->
        [one] The step is done.
       *[other] All { $total } steps are done.
    }

## The step-up field (§4.5, §10.4): what a restore is confirmed with, the
## admin password with sign-in on, the short ID of what it confirms with
## sign-in off. Paste and password managers work in it.

# The label of the step-up field with sign-in on.
ui-step-up-password-label = Admin password

# What the step-up field says under its label with sign-in on.
ui-step-up-password-hint = Type the admin password again, or fill it in from your password manager. Paste works here.

# The button beside a short ID that copies it to the clipboard.
ui-step-up-copy = Copy the ID

# Said beside that button once the ID is on the clipboard.
ui-step-up-copied = Copied.

# Said beside that button when the browser would not let the page write to the clipboard, as on a page not served over HTTPS or from this computer.
ui-step-up-copy-failed = The browser would not let the page copy it. Select the ID and copy it yourself.

# The button beside a key's fingerprint in the step-up field that copies it to the clipboard.
ui-step-up-copy-fingerprint = Copy the fingerprint

# Said beside that button when the browser would not let the page write to the clipboard, as on a page not served over HTTPS or from this computer.
ui-step-up-copy-fingerprint-failed = The browser would not let the page copy it. Select the fingerprint and copy it yourself.

## Restoring an entry of a snapshot from its page (§10.4): the Restore
## button beside the tree, its dialog and what is said as it runs.

# The heading of the pane beside a snapshot's tree that acts on the entry the tree selects.
ui-restore-pane = Selected entry

# What that pane says while the tree selects nothing.
ui-restore-pane-empty = Select a folder or file in the tree to restore or download it.

# The button that opens the restore dialog for the entry the tree selects, and that dialog's title.
# $name (text): the entry's name, as the tree shows it.
ui-restore-button = Restore { $name }

# What the restore dialog says first, which a screen reader reads as the dialog opens.
# $name (text): the entry's name, as the tree shows it.
# $snapshot (text): the snapshot's short ID.
# $alias (text): the repository's alias.
ui-restore-summary = This restores { $name } as { -snapshot } { $snapshot } of { $alias } holds it. A restore never deletes a file.

# The name of the choice of where to restore.
ui-restore-where = Where to restore

# The choice to restore into a folder made for this restore.
ui-restore-new-folder = Into a new folder

# The choice to restore over the original files, which adds and overwrites and never deletes.
ui-restore-in-place = Over the original files

# What the choice of where to restore says under its name.
ui-restore-where-hint = A new folder is made for it in the { -machine }'s restore folder. Over the original files, it first reads what it would change.

# Said in the restore dialog while the dry run of a restore over the original files reads what it would change.
ui-restore-previewing = Reading what a restore over the original files would change…

# What the dry run of a restore over the original files found, said in the restore dialog before it can be confirmed.
# $added (count): files it would add, where nothing is.
# $overwritten (count): files it would overwrite, which differ from the snapshot's.
# $unchanged (count): files already as the snapshot holds them.
ui-restore-preview =
    { $added ->
        [one] { $added } file added
       *[other] { $added } files added
    }, { $overwritten } overwritten, { $unchanged ->
        [one] { $unchanged } left as it is
       *[other] { $unchanged } left as they are
    }. Files the { -snapshot } doesn't hold stay.

# Beside the restore dialog's confirm button, while restoring over the original files waits for its dry run; also said when that button is pressed then.
ui-restore-held-reading = Restoring over the original files waits until the dry run says what it would change.

# Beside the restore dialog's confirm button, after the dry run of a restore over the original files couldn't be read; also said when that button is pressed then.
ui-restore-held-unread = The dry run couldn't be read, so nothing is restored over the original files. Read it again, or restore into a new folder.

# The button in the restore dialog that asks for the dry run again, after it couldn't be read.
ui-restore-read-again = Read the dry run again

# The label of the step-up field with sign-in off: the snapshot's short ID.
ui-restore-snapshot-id = Snapshot ID

# What the step-up field says under its label with sign-in off, before the short ID it shows.
ui-restore-snapshot-id-hint = Type or paste this { -snapshot }'s short ID to confirm:

# The restore dialog's confirm button.
ui-restore-confirm = Restore

# Said in the restore dialog while the restore is being started.
ui-restore-starting = Starting the restore…

# The step-up field's error with sign-in off, when what was typed is not the snapshot's short ID. Nothing was restored.
# $length (count): how many characters the short ID has.
ui-restore-wrong-id = That is not this { -snapshot }'s ID. Type or paste { $length } characters shown above.

# The step-up field's error with sign-in on, when what was typed is not the admin password. Nothing was restored.
ui-restore-wrong-password = That is not the admin password, so nothing was restored. Type it again, or fill it in from your password manager.

# Said politely once a restore started, while it runs.
# $name (text): the entry restored, as the tree shows it.
ui-restore-running = Restore of { $name } started.

# Said politely when a restore finished.
# $name (text): the entry restored, as the tree shows it.
ui-restore-finished = Restore of { $name } finished.

# Said politely when a restore into a new folder finished, naming that folder.
# $name (text): the entry restored, as the tree shows it.
# $folder (text): the folder made for the restore, its full path on this machine.
ui-restore-finished-into = Restore of { $name } finished. Its files are in { $folder }.

# Said politely when a restore was cancelled. What it wrote before stays.
# $name (text): the entry restored, as the tree shows it.
ui-restore-cancelled = Restore of { $name } was cancelled. What it wrote before stays.

# Said politely when a restore into a new folder was cancelled, naming that
# folder. What it wrote before stays there.
# $name (text): the entry restored, as the tree shows it.
# $folder (text): the folder made for the restore, its full path on this machine.
ui-restore-cancelled-into = Restore of { $name } was cancelled. What it wrote before stays in { $folder }.

# Said urgently when a restore failed. The Operations page says why.
# $name (text): the entry restored, as the tree shows it.
ui-restore-failed = Restore of { $name } failed. The Operations page says why.

# Said when the page stopped waiting for a restore, which goes on.
# $name (text): the entry restored, as the tree shows it.
ui-restore-unwatched = The daemon stopped answering, so this page can't say how the restore of { $name } ends. The Operations page will show it.

## Downloading an entry of a snapshot from its page (§10.4): the Download
## button beside the tree, its dialog and what is said once the browser has it.

# The button that opens the download dialog for the entry the tree selects, and that dialog's title.
# $name (text): the entry's name, as the tree shows it.
ui-download-button = Download { $name }

# What the download dialog says first, which a screen reader reads as the dialog opens.
# $name (text): the entry's name, as the tree shows it.
# $snapshot (text): the snapshot's short ID.
# $alias (text): the repository's alias.
ui-download-summary = This downloads { $name } as { -snapshot } { $snapshot } of { $alias } holds it, to this browser's downloads.

# The name of the choice of archive a folder is downloaded as.
ui-download-format = Archive format

# What the choice of archive format says under its name: neither archive compresses what it holds.
ui-download-format-hint = Either archive holds the files as they are, not compressed.

# The choice to download a folder as a tar archive. The extension is a literal.
ui-download-tar = Tar ({ ".tar" })

# The choice to download a folder as a zip archive. The extension is a literal.
ui-download-zip = Zip ({ ".zip" })

# The download dialog's confirm button.
ui-download-confirm = Download

# Said in the download dialog while the download is being confirmed.
ui-download-starting = Starting the download…

# The step-up field's error with sign-in off, when what was typed is not the snapshot's short ID. Nothing was downloaded.
# $length (count): how many characters the short ID has.
ui-download-wrong-id = That is not this { -snapshot }'s ID. Type or paste { $length } characters shown above.

# The step-up field's error with sign-in on, when what was typed is not the admin password. Nothing was downloaded.
ui-download-wrong-password = That is not the admin password, so nothing was downloaded. Type it again, or fill it in from your password manager.

# Said politely once the download was handed to the browser, which saves it as it does any download.
# $name (text): the entry downloaded, as the tree shows it.
ui-download-handed = Download of { $name } started. The browser saves it with its other downloads.

## A repository's stats at the end of the snapshots page (§4.4, §16 decision
## 104): four figures as last read, and a button that reads them now.

# The heading of the section of a repository's stats, named with the repository; it also names the table of the figures.
ui-stats-heading = Stats of { $alias }

# What the stats section shows of a repository whose stats have never been read.
ui-stats-none = No stats have been read yet.

# The stats table's header of the column of row headers, each naming a figure.
ui-stats-column-figure = Figure

# The stats table's header of the column of the figures as read.
ui-stats-column-value = Value

# The stats table's row header of how many snapshots the repository holds.
ui-stats-snapshots = Snapshots

# The stats table's row header of the size of the sources the snapshots hold, added up: a file two snapshots hold counts twice.
ui-stats-total-size = Total size

# The stats table's row header of what the repository stores for the snapshots, after deduplication and compression.
ui-stats-stored-size = Stored size

# The stats table's row header of what compression saved.
ui-stats-compression = Compression

# The stats table's count of snapshots.
# $count (number): how many snapshots the repository holds.
ui-stats-count = { $count }

# A size in the stats table.
# $bytes (bytes): the size, as a size such as 1 MiB.
ui-stats-size = { $bytes }

# What compression saved, in the stats table.
# $percent (number): the share of the data's raw size compression saved, a whole percent.
ui-stats-saved = { NUMBER($percent) }% saved

# What the stats table shows for compression when the repository stores no data, so compression saved nothing it could tell.
ui-stats-saved-none = None: the repository stores no data yet.

# When the stats shown were read, below their table, in the viewer's own zone.
# $when (instant): when they were read.
ui-stats-taken = Read { DATETIME($when, dateStyle: "short", timeStyle: "medium") }.

# The button that reads the repository's stats now.
ui-stats-read = Read the stats now

# Shown, and announced politely, while the stats are being read.
ui-stats-reading = Reading the stats…

# Announced politely when the stats were read; the table then shows them.
ui-stats-done = Stats read.

## Pausing live updates (§4.5, WCAG 2.2.2): a page that changes by itself
## holds still until the reader resumes it.

# The toggle button that pauses a page's live updates. Its name stays the same
# while it is pressed; a screen reader says that it is.
ui-live-pause = Pause live updates

# Said politely, once, when live updates are paused.
ui-live-paused = Live updates are paused.

# The banner beside the paused button, which describes it. It is not said by
# itself: a screen reader reads it with the button.
# $count (number): how many things on the page have changed since updates were
# paused, which the page shows when they resume.
ui-live-paused-banner =
    { $count ->
        [0] Live updates are paused. Nothing has changed since.
        [one] Live updates are paused. One change is waiting.
       *[other] Live updates are paused. { $count } changes are waiting.
    }

# The button in a page's row of actions that marks every row of its live
# table that a live update changed as seen, shown while any row is marked.
ui-mark-all-seen = Mark all as seen

# Read by a screen reader after the status of a row a live update changed
# and the reader hasn't seen yet, as a mark it carries; never shown. It
# reads on its own, after the cell's own words ("Failed").
ui-table-cue-unseen = Changed, not yet seen

# The button before a table that opens and closes its Columns panel, where
# the reader hides, shows and reorders the table's columns. The panel's
# list of columns is named by it too.
ui-table-columns = Columns

# The Columns panel's button that moves a column one place toward the
# start of the table, shown as an arrow; its name.
# $column (text): the column's header, as the table shows it.
ui-table-columns-move-up = Move { $column } up

# The Columns panel's button that moves a column one place toward the end
# of the table, shown as an arrow; its name.
# $column (text): the column's header, as the table shows it.
ui-table-columns-move-down = Move { $column } down

# The Columns panel's button that shows every column again, in the order
# the page gives them.
ui-table-columns-reset = Reset columns

# Said politely, once, when the reader hides a column in the Columns panel.
# $column (text): the column's header, as the table shows it.
ui-table-columns-hidden = { $column } hidden.

# Said politely, once, when the reader shows a hidden column again in the
# Columns panel.
# $column (text): the column's header, as the table shows it.
ui-table-columns-shown = { $column } shown.

# Said politely, once, when the reader moves a column in the Columns panel,
# by a Move button or by dragging it.
# $column (text): the column's header, as the table shows it.
# $position (number): its place in the panel's list now, from 1.
# $count (number): how many columns the list holds.
ui-table-columns-moved = { $column } moved to position { $position } of { $count }.

# Said politely, once, when "Reset columns" is pressed.
ui-table-columns-was-reset = Columns reset: every column shown, in the order given.

# Said politely, once, when "Mark all as seen" is pressed.
# $count (number): how many rows it marked as seen.
ui-marked-seen =
    { $count ->
        [one] One changed row marked as seen.
       *[other] { $count } changed rows marked as seen.
    }

# Said politely, once, when live updates resume, in place of one announcement
# for each run that ended while they were paused.
# $ended (number): how many runs ended while updates were paused.
# $failed (number): how many of those runs failed.
ui-live-resumed =
    { $ended ->
        [0] Live updates are on again. No run ended while they were paused.
        [one] { $failed ->
            [0] Live updates are on again. One run ended while they were paused, and it did not fail.
           *[other] Live updates are on again. One run ended while they were paused, and it failed.
        }
       *[other] { $failed ->
            [0] Live updates are on again. { $ended } runs ended while they were paused, and none of them failed.
            [one] Live updates are on again. { $ended } runs ended while they were paused, and one of them failed.
           *[other] Live updates are on again. { $ended } runs ended while they were paused, and { $failed } of them failed.
        }
    }

## The navigation every page shares.

# The accessible name of the navigation in the header, the list of links to the pages.
ui-nav-label = Pages

# The accessible name of the header's icon-only hamburger button, on a screen narrower than 54rem, that shows and hides the menu: every page's link and Sign out. The same name whether the menu shows or not.
ui-menu-button = Menu

# The navigation's link to the operations page.
ui-nav-operations = Operations

# The navigation's link to the Plans page.
ui-nav-plans = Plans

# The navigation's link to the "Pending changes" page, where a person approves or rejects the configuration changes an AI agent proposed.
ui-nav-proposals = Pending changes

# The navigation's link to the snapshots page.
ui-nav-snapshots = Snapshots

# The navigation's link to the repositories page.
ui-nav-repositories = Repositories

# The navigation's link to the Log page.
ui-nav-log = Log

# The navigation's link to the Settings page.
ui-nav-settings = Settings

# The words of the header's button that shows and hides the Settings menu, after the pages' links: a panel holding the links to the Settings and Log pages and, while signed in, Sign out. The name of that group of pages; the same whether the menu shows or not.
ui-settings-menu-button = Settings

## The repositories page: the repositories Ladderback backs up to, and the
## form that tests, connects or creates one (§4.4).

# The repositories page's title, as the browser's tab and history show it.
ui-repositories-title = Repositories – { -brand-name }

# The repositories page's heading: its one main heading.
ui-repositories-heading = Repositories

# The repositories page's text, above the list.
ui-repositories-body = The repositories { -brand-name } backs up to.

# The repositories table's column of names.
ui-column-name = Name

# The repositories table's column of locations.
ui-column-location = Location

# The repositories table's column of where each password comes from.
ui-column-password = Password

# What the repositories page says when none is configured.
ui-no-repositories = No repositories yet.

# The second line of a location cell: where the hot half of a repository kept in
# two places is.
# $location (text): that location, with any password in it masked. It is shown as it came.
ui-repository-hot = Hot storage: { $location }

# A password source: the configuration file holds the password itself. The
# password is never shown.
ui-password-inline = Kept in the configuration file

# A password source: a file holds the password.
# $path (text): the file's path. It is shown as it came.
ui-password-file = Read from the file { $path }

# A password source: an entry of the operating system's keyring holds the password.
# $entry (text): the entry, as service and name. It is shown as it came.
ui-password-keyring = Read from the keyring entry { $entry }

# A password source: an environment variable of the daemon holds the password.
# $name (text): the variable's name. It is shown as it came.
ui-password-env = Read from the environment variable { $name }

# A password source: a program prints the password.
# $program (text): the program's name. It is shown as it came.
ui-password-command = Read from the output of { $program }

# The heading of the form that tests, connects or creates a repository.
ui-repositories-form-heading = Test, connect or create a repository

# The label of the field for the name a repository is saved under.
ui-repositories-name-label = Name

# The hint under the name field.
ui-repositories-name-hint = Up to 64 lower-case letters, digits and hyphens. Connecting a repository needs one.

# The label of the field for where the repository is.
ui-repositories-location-label = Location

# The hint under the location field. The location is typed as it is: keep the example a string literal.
ui-repositories-location-hint = A folder on this computer, or a remote location such as { "sftp:user@host:/path" }.

# The label of the optional field for the hot half of a repository kept in two places.
ui-repositories-hot-label = Hot storage (optional)

# The hint under the hot storage field.
ui-repositories-hot-hint = The fast half of a repository kept in two places. Leave it empty when there is one place.

# The label of the password field.
ui-repositories-password-label = Password

# The hint under the password field. It says where the typed password goes: where the choice under the field says, and, in the configuration file, also in the history of it that { -brand-name } keeps.
ui-repositories-password-hint = { -brand-name } keeps it where you choose below. In the configuration file, it is also in the copies { -brand-name } keeps of that file's earlier versions. It never shows it again.

# The legend of the choice, under the password field, of where the typed password is kept.
ui-repositories-keep-password-legend = Where to keep the password

# The hint under that legend. The entry's name is typed text: keep "ladderback/" as it is.
ui-repositories-keep-password-hint = The keyring entry is named { "ladderback/" } followed by the repository's name. If this computer has no keyring, keep the password in the configuration file.

# The choice that keeps the typed password in the configuration file, chosen first.
ui-repositories-keep-password-configuration = In the configuration file

# The choice that keeps the typed password in the system keyring: the macOS keychain, or the Secret Service on Linux.
ui-repositories-keep-password-keyring = In the system keyring

# The label of the checkbox that asks this machine to maintain a repository it connects: run its forget, prune and check. Off unless the person turns it on.
ui-repositories-maintain-label = Maintain this repository from this { -machine }

# The hint under that checkbox. It says what maintaining means, that a repository created here is always maintained from here, and why to leave it off: only one place may maintain a repository, and another machine or the hub may already do it.
ui-repositories-maintain-hint = When this is on, this { -machine } forgets, prunes and checks the repository you connect, and adopts history into its plans. Only one place may maintain a repository, so leave this off when another { -machine } or the { -hub } already does. A repository you create here under a name is always maintained from here.

# The button that opens the repository and saves nothing.
ui-repositories-test = Test the repository

# The button that opens the repository and, if it opens, saves it under the name.
ui-repositories-connect = Connect the repository

# The button that creates a repository where there is none.
ui-repositories-create = Create the repository

# The heading of the list of what is wrong with the form.
# $count (number): how many problems the list names.
ui-repositories-problems = { $count ->
    [one] There is { $count } problem with the form
   *[other] There are { $count } problems with the form
  }

# A problem with the form: the name is missing.
ui-repositories-name-required = Enter a name for the repository.

# A problem with the form: the location is missing.
ui-repositories-location-required = Enter where the repository is.

# A problem with the form: the password is missing.
ui-repositories-password-required = Enter the repository's password.

# What follows a repository that would not open, after the reason.
ui-repositories-try-again = Check the location and the password, then try again.

# Shown and announced politely while the repository is being opened or created.
ui-repositories-working = Working with the repository…

# Announced politely when a test opened the repository.
ui-repositories-opens = The repository opens.

# Shown when a test opened the repository.
# $id (text): the repository's ID, as rustic knows it.
ui-repositories-opens-with-id = The repository opens. Its ID is { $id }.

# Announced politely when a repository was created.
ui-repositories-created = The repository was created.

# Shown when a repository was created.
# $id (text): the repository's ID, as rustic knows it.
ui-repositories-created-with-id = The repository was created. Its ID is { $id }.

# Shown and announced politely when a repository was connected and saved.
# $alias (text): the name it was saved under.
ui-repositories-connected = Connected { $alias }.

# The button in a repository's row that creates the repository where its entry says, with the password the entry names. Shown only once an operation found no repository there, while none of them ever succeeded.
# $alias (text): the name the repository is saved under.
ui-repository-create = Create { $alias }

# Shown below the list of repositories when a repository was created from its row, which then offers no Create; focus moves to it, so a screen reader reads it. The repository's ID follows in a sentence of its own, ui-repository-created-id, outside what has focus.
# $alias (text): the name the repository is saved under.
ui-repository-created = Created { $alias }.

# Shown right after ui-repository-created: the ID of the repository created from its row. Kept apart from it, so a screen reader doesn't read 64 hex digits when focus moves to the confirmation.
# $id (text): the repository's ID, as rustic knows it.
ui-repository-created-id = Its ID is { $id }.

# Shown in a repository's row when creating it was refused because a repository is already where its entry says. Nothing was written.
# $alias (text): the name the repository is saved under.
ui-repository-already-there = There is already a repository at the location of { $alias }, so nothing was created.

# The name of the list, in a repository's row, of the locations a repository reached at several is tried at, in order (decision 80).
# $alias (text): the name the repository is saved under.
ui-repository-locations = Locations of { $alias }

# What heads the first location in a repository's list of locations: a job tries it first.
# $position (count): its place in the order, 1.
ui-location-tried-first = Location { $position }, tried first

# What heads a later location in a repository's list of locations.
# $position (count): its place in the order, from 2.
ui-location-position = Location { $position }

# One option of a location in a repository's list of locations, whose value is made of references to secrets, which say where a secret is and never hold it.
# $name (text): the option's name, as the configuration file writes it. It is shown as it came.
# $set (text): the set of options it is in: options, options-hot or options-cold, as the configuration file writes it. It is shown as it came.
# $reference (text): the value, such as ${LAN_KEY}. It is shown as it came.
ui-location-option-reference = The option { $name } in { $set } comes from { $reference }.

# One option of a location in a repository's list of locations, whose value is written in the configuration file. The value may be a secret, so it is never shown.
# $name (text): the option's name, as the configuration file writes it. It is shown as it came.
# $set (text): the set of options it is in: options, options-hot or options-cold, as the configuration file writes it. It is shown as it came.
ui-location-option-written = The option { $name } in { $set } is written in the configuration file and not shown.

# The button in the row of a repository reached at several locations that tries each location in turn, without changing anything: its name, which a screen reader says. It begins with the button's visible text, ui-repository-test.
# $alias (text): the name the repository is saved under.
ui-repository-test-locations = Test the locations of { $alias }

# The visible text of that button, one short word, so the button takes one line in a narrow column. Its name, ui-repository-test-locations, begins with it.
ui-repository-test = Test

# What a row's Test found at one location, shown under the location in the repository's list of locations.
# $outcome (text): what it found, one of the codes below. Not shown.
ui-location-tested = { $outcome ->
    [used] Answers and holds the repository.
    [silent] Did not answer.
    [refused] Refused its credentials.
    [not-a-repository] Answers, but holds no repository.
    [other-repository] Answers, but holds another repository. Check its address.
    [password-not-accepted] Answers, but did not accept the repository's password.
    [secret-unavailable] Not tried: a secret it needs could not be read.
    [template-invalid] Not tried: one of its settings is not a valid template.
    [repository-path-relative] Not tried: one of its paths is relative once resolved. Write an absolute path.
    [secret-expansion-too-long] Not tried: one of its settings is too long once its references are resolved.
    [secret-in-process-arguments] Not tried: a setting passed to a program holds a reference, which would show its secret.
    [location-too-many-credentials] Not tried: its rclone connection string holds more credentials than a job can hide.
    [rule-not-met] Not tried: its rule doesn't hold on the network this machine is on.
    [rule-unknown] Not tried: its rule couldn't be checked on this machine.
   *[other] Could not be used.
}

# One alternative of a location's rule, under the location in a repository's list of locations. The location may be tried when any one of its alternatives holds.
# $network (text): the kind of network the alternative asks for, as the configuration file writes it: wired or wifi. Not shown.
ui-location-rule = { $network ->
    [wired] May be tried on a wired network.
   *[wifi] May be tried on any Wi-Fi network.
}

# One alternative of a location's rule that lists Wi-Fi networks by name, one line per name: the location may be tried on that network. The location may be tried when any one of its alternatives holds.
# $name (text): the network's name, as the configuration file lists it.
ui-location-rule-only = May be tried on the Wi-Fi network “{ $name }”.

# Right after ui-location-rule for a Wi-Fi alternative that leaves networks out by name, one line per name: the location isn't tried on that network.
# $name (text): the network's name, as the configuration file lists it.
ui-location-rule-except = Not tried on the Wi-Fi network “{ $name }”.

# Under a location's rule, after its alternatives: where the location isn't tried. A VPN that carries all traffic makes the machine's network link the VPN's.
ui-location-rule-elsewhere = Skipped on any other network, and behind a VPN that carries all of this machine's traffic.

# Shown right after ui-location-tested, when the daemon named where a secret was looked for or which setting can't be used. Never a value.
# $outcome (text): what the Test found, as in ui-location-tested. Not shown.
# $source (text): where the secret was looked for, such as env LAN_KEY, or the setting, such as options.key. It is shown as it came.
ui-location-tested-source = { $outcome ->
    [secret-unavailable] The secret was looked for in { $source }.
   *[other] The setting is { $source }.
}

# Announced politely when a row's Test of a repository's locations answered. Each location's own outcome is shown in its list.
# $alias (text): the name the repository is saved under.
# $answering (count): how many of its locations answer and hold the repository.
# $count (count): how many locations it has, at least 2.
ui-repository-locations-tested = { $answering ->
    [one] Tested { $alias }: { $answering } of its { $count } locations answers and holds the repository.
   *[other] Tested { $alias }: { $answering } of its { $count } locations answer and hold the repository.
}

## Sign-in (security §10.1): the sign-in page, the dialog shown when a session
## ends while the page is in use, and the start page's notice that sign-in is off.

# The sign-in page's title, as the browser's tab and history show it.
ui-sign-in-title = Sign in – { -brand-name }

# The sign-in page's heading: the one main heading of the page.
ui-sign-in-heading = Sign in to { -brand-name }

# The label of the one field of the sign-in form, on the page and in the dialog.
ui-sign-in-password-label = Admin password

# The hint under that field: the two ways the admin password is set, the Admin password tab of Settings first. "Admin password" is that tab's name, as `ui-admin-password-heading` words it. `ladderback set-password` is typed as it is: keep it a string literal.
ui-sign-in-password-hint = The password set in the Admin password tab of Settings, or with { "ladderback set-password" }. Paste and password managers work here.

# The button that sends the admin password.
ui-sign-in-submit = Sign in

# The checkbox under the admin password that lifts the 12-hour limit on a session left unused. The form leaves it out when the session length is Until I close the browser.
ui-sign-in-keep = Keep me signed in on this device

# The description under that checkbox.
ui-sign-in-keep-hint = Only on a device no one else uses: this browser then stays signed in however long it goes unused, until the session length chosen in Settings ends it.

# Shown when the form is sent with the field empty, in the error summary and under the field.
ui-sign-in-password-required = Enter the admin password.

# The heading of the sign-in form's error summary, which takes focus when the form is refused.
# $count (number): how many problems the list names.
ui-sign-in-problems = { $count ->
    [one] There is { $count } problem with the form
   *[other] There are { $count } problems with the form
  }

# The name and heading of the dialog shown when the session ends while the page is in use.
ui-session-ended-title = Sign in again

# What that dialog says. The page under it stays as it was, so nothing typed there is lost; the request that found the session ended is sent again once signed in.
ui-session-ended-body = Your session has ended. Sign in again to go on: what you typed on this page is kept, and what you asked for is done once you are signed in.

# The start page's notice when no admin password is set and Ladderback listens on loopback only, so anyone with an account on this machine can use it. Sign-in is decided only when Ladderback starts, so a password set takes a restart. The notice's link to Settings, `ui-sign-in-off-link`, follows it.
ui-sign-in-off = Sign-in is off: anyone on this machine can use { -brand-name }. To turn sign-in on, set an admin password, then restart { -brand-name }.

# The notice on every page when no admin password is set and Ladderback listens on the network (an address beyond loopback), so anyone who can reach that address can use it. Sign-in is decided only when Ladderback starts, so a password set takes a restart. The notice's link to Settings, `ui-sign-in-off-link`, follows it.
ui-sign-in-off-network = Sign-in is off, and { -brand-name } answers on the network: anyone who can reach it can run and change backups. To turn sign-in on, set an admin password, then restart { -brand-name }.

# The link after either notice that sign-in is off, to the Admin password tab of Settings, where the admin password is set.
ui-sign-in-off-link = Set an admin password in Settings

# The name and heading of the warning shown 2 minutes before a session would end because the page has not used it for 12 hours (WCAG 2.2.1). It is not shown when the session's 30-day lifetime ends it first.
ui-idle-warning-title = You will be signed out soon

# What the warning says. Being signed out keeps what was typed on the page: the dialog that asks to sign in again opens over it.
ui-idle-warning-body = This page has not been used for nearly 12 hours, so { -brand-name } will sign you out in 2 minutes. Choose Stay signed in to go on. If you are signed out, what you typed here is kept, and you can sign in again.

# The warning's one button, which keeps the session another 12 hours unused, up to its 30-day lifetime. Escape keeps it too.
ui-idle-warning-stay = Stay signed in

# The control at the end of the navigation that ends the session, shown while sign-in is on and the page is signed in.
ui-sign-out = Sign out

# Shown beside Sign out, or in the warning, when the daemon did not answer the request, so the session is as it was.
ui-session-unreachable = The daemon that serves this page did not answer, so nothing changed. Try again in a moment.

## The Log page (§4.4): the recent log, Ladderback's own log lines since the
## daemon started, newest first, secrets masked.

# The Log page's title, as the browser's tab and history show it.
ui-log-title = Log – { -brand-name }

# The Log page's heading: its one main heading, which also names the table of lines.
ui-log-heading = Log

# Under the heading: what the page lists.
ui-log-body = The { -recent-log }: { -brand-name }'s own log since the daemon started, newest first, with secrets masked. Only the newest lines are kept.

# The legend of the choice of which lines the table lists.
ui-log-levels = Show

# The choice that lists every line.
ui-log-levels-all = All levels

# The choice that lists the warnings and the errors.
ui-log-levels-warnings = Warnings and errors

# The choice that lists only the errors.
ui-log-levels-errors = Errors only

# The button that copies the lines the table lists, with the running version first, for a bug report.
ui-log-copy = Copy for a bug report

# Said once the lines are on the clipboard.
ui-log-copied = Copied the lines shown.

# Said when the browser would not let the page write to the clipboard, as on a page not served over HTTPS or from this computer.
ui-log-copy-failed = The browser would not let the page copy the lines. Select them in the table and copy them yourself.

# The header of the column that says when a line was written, in UTC as the log writes it.
ui-log-at = Time (UTC)

# The header of the column that says how much a line matters.
ui-log-level = Level

# The header of the column that names the part of the program that wrote a line.
ui-log-source = Source

# The header of the column that holds what a line says.
ui-log-message = Message

# A line's level: an error.
ui-log-level-error = Error

# A line's level: a warning.
ui-log-level-warn = Warning

# A line's level: information.
ui-log-level-info = Info

# A line's level: detail for debugging.
ui-log-level-debug = Debug

# A line's level: the finest detail.
ui-log-level-trace = Trace

# In the table's place when the daemon has logged nothing yet.
ui-log-none = The daemon has logged nothing yet.

# In the table's place when no line is at the levels chosen.
ui-log-none-shown = No line of the { -recent-log } is at these levels.

## The Settings page (§4.4): the API tokens, which let a program other than a
## browser use Ladderback while sign-in is on (§10.1).

# The Settings page's title, as the browser's tab and history show it.
ui-settings-title = Settings – { -brand-name }

# The Settings page's heading: its one main heading.
ui-settings-heading = Settings

# Settings, while sign-in is off: an admin password has been set since Ladderback started, by `ladderback set-password` or in the Admin password tab. Sign-in is decided only when Ladderback starts, so it stays off until a restart, which the Restart Ladderback tab of the same page does where a service manager runs Ladderback. "Restart Ladderback" is that tab's name, as `ui-restart-heading` words it.
ui-admin-password-waiting = Admin password set. Restart { -brand-name } to turn sign-in on. The Restart { -brand-name } tab does it where a service manager runs { -brand-name }.

# Settings, while sign-in is off: an admin password file has appeared that Ladderback can't read or check, so it would refuse to start with it. The password can be set again in the Admin password tab of the same page, named as `ui-admin-password-heading` words it, or at a terminal. `ladderback set-password` is typed as it is: keep it a string literal.
ui-admin-password-damaged = The admin password file is damaged, so { -brand-name } would not start with it. Set the password again in the Admin password tab, or at a terminal with { "ladderback set-password" }.

# The heading of the section of Settings that restarts Ladderback.
ui-restart-heading = Restart { -brand-name }

# What that section says when a restart is offered: what a restart is for. The Admin password tab is the tab of the same page, named as `ui-admin-password-heading` words it.
ui-restart-body = A restart applies what { -brand-name } reads only when it starts, such as an admin password set in the Admin password tab. { -brand-name } stops in order, and its service manager starts it again.

# What that section says in place of the button when no service manager runs Ladderback.
ui-restart-unavailable = No service manager runs { -brand-name }, so nothing would start it again after a restart. To restart it from here, install it as a service with { "ladderback install-service" }.

# The button that asks before Ladderback restarts.
ui-restart-button = Restart { -brand-name }

# The title of the dialog that asks before Ladderback restarts.
ui-restart-title = Restart { -brand-name }?

# What that dialog says about the operations running now, and the two choices it offers when some are: a restart once they finish, which starts no new operation meanwhile, or a restart now, which stops them as any stop of Ladderback does.
# $running (count): how many operations are running.
ui-restart-running = { $running ->
        [0] No operation is running.
        [one] One operation is running. Restart once it finishes, and { -brand-name } starts no new operation meanwhile. Or restart now: that stops it, as any stop of { -brand-name } does, and its history says the shutdown interrupted it.
       *[other] { $running } operations are running. Restart once they finish, and { -brand-name } starts no new operation meanwhile. Or restart now: that stops them, as any stop of { -brand-name } does, and their history says the shutdown interrupted them.
    }

# What that dialog says after the running operations: what becomes of queued ones.
ui-restart-queued = Queued operations wait, and run once { -brand-name } is back.

# The button of that dialog that restarts Ladderback.
ui-restart-confirm = Restart

# The button of that dialog, while operations are running, that restarts Ladderback once they finish.
# $running (count): how many operations are running.
ui-restart-when-finished = { $running ->
        [one] Restart once it finishes
       *[other] Restart once they finish
    }

# The button of that dialog, while operations are running, that restarts Ladderback at once, stopping them.
ui-restart-now = Restart now

# What the restart section of Settings says while a restart waits for the running operations to finish.
ui-restart-pending = { -brand-name } restarts once the running operations finish, and starts no new operation meanwhile.

# The button of that section that cancels a restart that waits, so the queued operations start again.
ui-restart-cancel = Cancel restart

# Said in that dialog while the request to restart is on its way.
ui-restart-asking = Asking { -brand-name } to restart…

# Shown and announced once Ladderback has begun to restart, while the page waits for it to answer again.
ui-restarting = { -brand-name } is restarting. This page reloads once it answers again.

# Shown when Ladderback hasn't answered a minute after the restart began.
ui-restart-not-back = { -brand-name } hasn't answered for a minute since it began to restart. Check its service: { "systemctl --user status ladderback" } on Linux, or its log, { "~/Library/Logs/ladderback.log" }, on macOS.

# The heading of the section of Settings that sets the admin password, or changes it while sign-in is on.
ui-admin-password-heading = Admin password

# What that section says while sign-in is off: no admin password is in use yet. Sign-in is decided only when Ladderback starts.
ui-admin-password-off-body = There is no admin password yet. Once it is set, sign-in protects { -brand-name }: the web UI asks for the password, and only a signed-in browser or an { -api-token } can use { -brand-name }. Sign-in turns on when { -brand-name } next starts.

# What that section says while sign-in is on.
ui-admin-password-on-body = Changing the admin password signs out every browser, this one included, and revokes every { -api-token }.

# The label of the field for the admin password as it is now, asked for while sign-in is on.
ui-admin-password-current-label = Current password

# The hint under that field.
ui-admin-password-current-hint = The admin password you signed in with.

# The label of the field for the new admin password.
ui-admin-password-new-label = New password

# The hint under that field.
# $min (number): the fewest characters an admin password may have.
ui-admin-password-new-hint = { $min ->
    [one] At least 1 character.
   *[other] At least { $min } characters.
  }

# The label of the field where the new admin password is typed a second time.
ui-admin-password-again-label = New password again

# The hint under that field.
ui-admin-password-again-hint = Type the new password once more, so a typing mistake can't lock you out.

# The button that sets the admin password while sign-in is off.
ui-admin-password-set = Set the admin password

# The button that changes the admin password while sign-in is on.
ui-admin-password-change = Change the admin password

# Said while the daemon sets the admin password.
ui-admin-password-saving = Setting the admin password…

# The heading of the list of what is wrong with the form, which takes focus when the form is refused.
# $count (number): how many problems the list names.
ui-admin-password-problems = { $count ->
    [one] There is { $count } problem with the form
   *[other] There are { $count } problems with the form
  }

# Shown when the form is sent without the current password, in the error summary and under the field.
ui-admin-password-current-required = Enter the current admin password.

# Shown when the form is sent without a new password.
ui-admin-password-new-required = Enter a new password.

# Shown when the two new passwords typed differ.
ui-admin-password-mismatch = The two new passwords differ. Type the same password in both fields.

# Shown on the sign-in page once the admin password has been changed in Settings, which signed every browser out.
ui-admin-password-changed = Admin password changed. Sign in with the new password.

# The heading of the section of Settings that chooses the language the web UI is shown in.
ui-language-heading = Language

# The legend of the choice of the web UI's language. Each choice after the first is a language named in itself (ui-language-name), as its readers write it.
ui-language-legend = Show { -brand-name } in

# The description under that legend. The choice is kept in this browser alone, not by the daemon. Saving loads the page again, which drops anything else Settings is showing, such as a new API token's secret, so the hint says so.
ui-language-hint = Kept in this browser. Saving reloads the page, which clears anything else Settings is showing. With the browser's languages, { -brand-name } uses the first of them it offers, and American English if it offers none.

# The first choice of the web UI's language: no language of its own, but the languages the browser's settings list, best first. The one chosen until another is.
ui-language-browser = The browser's languages

# The name of this catalog's language, written in that language as its readers write it, as the choice of language in Settings shows it: "English", "日本語", "Deutsch".
ui-language-name = English

# The button that keeps the language chosen in this browser, then shows the page again in it.
ui-language-save = Save the language

# Shown when the browser refused to keep the language chosen: its settings, or a private window, may let a site keep nothing.
ui-language-not-kept = This browser didn't keep the language. Its settings may stop sites from keeping anything.

# The heading of the section of Settings that chooses how wide each page's content is.
ui-page-width-heading = Page width

# The legend of the choice of how wide each page's content is: Narrow (ui-page-width-narrow) or Full width (ui-page-width-full).
ui-page-width-legend = Width of each page

# The description under that legend. The choice is kept in this browser alone, not by the daemon, and takes effect as soon as it is chosen, with no button. In Full width the tables use the window's width, while paragraphs and forms stay about 70 characters wide.
ui-page-width-hint = Kept in this browser, and applied as soon as it's chosen. Full width lets tables use the whole window, while paragraphs and forms keep a readable line length.

# The first choice of the page's width, the one chosen until another is: the content stays at a fixed width, centered in a wide window.
ui-page-width-narrow = Narrow

# The second choice of the page's width: the content uses the window's whole width.
ui-page-width-full = Full width

# Shown when the browser refused to keep the page width chosen: its settings, or a private window, may let a site keep nothing. The page keeps the width it had.
ui-page-width-not-kept = This browser didn't keep the page width, so the page stays as it was. Its settings may stop sites from keeping anything.

# The heading of the section of Settings about signing in: how long a session lasts, and signing out every browser at once.
ui-signing-in-heading = Signing in

# What that section says while sign-in is off, in place of its choice and its button.
ui-signing-in-off-body = Stay signed in for applies once sign-in is on, and so does Sign out everywhere.

# The legend of the choice of how long every session lasts after its sign-in (the session length).
ui-session-length-legend = Stay signed in for

# The description under that legend.
ui-session-length-hint = How long a browser stays signed in after it signs in, however much it is used. A shorter choice also signs out the browsers that signed in longer ago, this one included. Until I close the browser ends a session when the browser closes, or after 12 hours unused.

# A choice of the session length: the session ends when the browser closes, or after 12 hours unused.
ui-session-length-until-browser-closes = Until I close the browser

# A choice of the session length.
ui-session-length-7-days = 7 days

# A choice of the session length, the one chosen until the admin chooses another.
ui-session-length-30-days = 30 days

# A choice of the session length.
ui-session-length-90-days = 90 days

# A choice of the session length.
ui-session-length-1-year = 1 year

# A choice of the session length: the session ends only at a sign-out, or after 12 hours unused without Keep me signed in.
ui-session-length-until-sign-out = Until I sign out

# The button that saves the session length chosen.
ui-session-length-save = Save the session length

# Said while the daemon saves the session length.
ui-session-length-saving = Saving the session length…

# Shown and said once the daemon has saved the session length.
ui-session-length-saved = Saved. Stay signed in for now applies to every session, the ones already open included.

# What the section says about the button below it.
ui-sign-out-everywhere-body = Sign out everywhere ends the session of every browser signed in to { -brand-name }, this one included. { -api-token(form: "other") } keep working.

# The button that asks before every browser's session is ended.
ui-sign-out-everywhere = Sign out everywhere

# The title of the dialog that asks before every browser's session is ended.
ui-sign-out-everywhere-title = Sign out everywhere?

# What that dialog says.
ui-sign-out-everywhere-confirm-body = Every browser signed in to { -brand-name }, this one included, is signed out and has to sign in again. { -api-token(form: "other") } keep working.

# The dialog's button that ends every browser's session. Cancel comes first.
ui-sign-out-everywhere-confirm = Sign out every browser

# Said in the dialog while the daemon ends every session.
ui-sign-out-everywhere-working = Signing out every browser…

# The heading of the section of Settings that lists the API tokens and creates them. It names the table of tokens too.
ui-api-tokens-heading = { -api-token(form: "other") }

# What that section says above the list.
ui-api-tokens-body = An { -api-token } lets a program other than a browser, such as MCP connect mode, use { -brand-name } while sign-in is on. It can do only what its permissions allow, and it works until you revoke it or set a new admin password.

# Shown in place of the list when no token is kept.
ui-api-tokens-none = No { -api-token(form: "other") } yet.

# The header of the column of each token's label, which names its row.
ui-api-tokens-column-label = Label

# The header of the column that lists what each API token may do.
ui-api-tokens-column-permissions = Permissions

# The header of the column of when each token was created.
ui-api-tokens-column-created = Created

# The header of the column of when a program last used each token.
ui-api-tokens-column-last-used = Last used

# A permission's name: the legend of its group of choices, in the API token form and in the AI agents tab.
# $permission (text): the permission, as a code: backups, backup-runs, previews, configuration, snapshot-contents, daemon-log or repository-check.
ui-permission = { $permission ->
    [backups] Backups
    [backup-runs] Backup runs
    [previews] Previews
    [configuration] Configuration
    [snapshot-contents] Snapshot contents
    [daemon-log] Daemon log
   *[repository-check] Repository check
  }

# The hint under a permission's name: what it opens, in plain words.
# $permission (text): the permission, as a code: backups, backup-runs, previews, configuration, snapshot-contents, daemon-log or repository-check.
ui-permission-hint = { $permission ->
    [backups] See plans, every { -repository }, the list of { -snapshot(form: "other") }, operations and their progress, and status. Not the files inside a { -snapshot }, and not the daemon's log: each is a permission of its own.
    [backup-runs] Start a plan's backup, cancel a backup, and test a saved { -repository }. On its own it shows nothing, so a scheduler can start backups without reading anything.
    [previews] Ask what forgetting or pruning { -snapshot(form: "other") }, checking a { -repository }, adopting { -snapshot(form: "other") } into another plan or importing a profile would do, without doing it.
    [configuration] Propose changes to plans for you to approve on the Pending changes page, and try a plan in a dry run.
    [snapshot-contents] See the names of the files and folders inside { -snapshot(form: "other") }, and those a warning or a failure names.
    [daemon-log] Read { -brand-name }'s log, which can name your files and folders.
   *[repository-check] Start a check of a { -repository }. Nothing offers this yet: it is here so that what you allow today allows no check a later version adds.
  }

# A choice in a permission's group: no access, or the one level the permission has.
# $level (text): none, read, start-and-cancel, preview, propose or run.
ui-permission-level = { $level ->
    [none] No access
    [read] Read
    [start-and-cancel] Start and cancel
    [preview] Preview
    [propose] Propose
   *[run] Run
  }

# How an API token's row lists a permission it holds: the permission's name and its level.
# $permission (text): the permission, as a code: backups, backup-runs, previews, configuration, snapshot-contents, daemon-log or repository-check.
ui-permission-granted = { $permission ->
    [backups] Backups: Read
    [backup-runs] Backup runs: Start and cancel
    [previews] Previews: Preview
    [configuration] Configuration: Propose
    [snapshot-contents] Snapshot contents: Read
    [daemon-log] Daemon log: Read
   *[repository-check] Repository check: Run
  }

# The legend of the permissions of the API token form, around one group per permission.
ui-api-token-permissions-legend = Permissions

# The hint under the legend of the token form's permissions.
ui-api-token-permissions-hint = What a program with this token may do. Give it only what its job needs, and at least one permission.

# The button on an API token's row that opens the dialog that changes its permissions, named for the token.
# $label (text): the token's label.
ui-api-token-change = Change permissions of token { $label }

# The title of the dialog that changes an API token's permissions.
# $label (text): the token's label.
ui-api-token-change-title = Permissions of token { $label }

# What the dialog that changes an API token's permissions says, above its groups.
ui-api-token-change-body = A program that uses this token may do only what you choose here, from its next request on. Saving takes the admin password.

# The button that saves an API token's new permissions.
ui-api-token-change-confirm = Save the permissions

# What the save button says while the change is sent.
ui-api-token-changing = Saving the permissions…

# What is announced once an API token's permissions are changed.
# $label (text): the token's label.
ui-api-token-changed = Token { $label } has its new permissions.

# The name of the Settings tab of what AI agents may do without an API token.
ui-ai-agents-tab = AI agents

# The heading of the AI agents tab.
ui-ai-agents-heading = What AI agents may do without a token

# What the AI agents tab says first: what its permissions limit.
ui-ai-agents-body = These permissions limit what an AI agent may do through { "ladderback mcp" } when it has no { -api-token }: in a session of its own, and when it connects to { -brand-name } while sign-in is off. An AI agent with a token may do what its token allows.

# What the AI agents tab says with sign-in off.
ui-ai-agents-sign-in-off = Sign-in is off, so these permissions limit only { "ladderback mcp" }: any other program on this machine or your network may use { -brand-name } without asking. Set an admin password to turn sign-in on, and give each program a token of its own.

# The button that saves what AI agents may do without a token.
ui-ai-agents-save = Save what AI agents may do

# What the save button says while the change is sent.
ui-ai-agents-saving = Saving…

# What is announced once what AI agents may do without a token is saved.
ui-ai-agents-saved = AI agents without a token now have the permissions you chose.

# The Last used column of a token no program has used yet.
ui-api-token-never-used = Never

# The Last used column of a token: how long ago a program last used it, counted back from now on this computer's clock.
# $unit (text): which count follows: now (under a minute), minutes, hours or days.
# $count (number): how many minutes, hours or days, rounded down.
ui-api-token-last-used = { $unit ->
    [now] Less than a minute ago
    [minutes] { $count ->
        [one] { $count } minute ago
       *[other] { $count } minutes ago
      }
    [hours] { $count ->
        [one] { $count } hour ago
       *[other] { $count } hours ago
      }
   *[days] { $count ->
        [one] { $count } day ago
       *[other] { $count } days ago
      }
  }

# The button on a token's row that asks to revoke it, named for the token so a screen reader tells the rows' buttons apart.
# $label (text): the token's label.
ui-api-token-revoke = Revoke token { $label }

# The title of the dialog that asks before a token is revoked.
# $label (text): the token's label.
ui-api-token-revoke-title = Revoke token { $label }

# What that dialog says: what revoking does, and that it can't be undone.
ui-api-token-revoke-body = A program that uses this token is refused from then on, and the token can't be brought back. To let the program in again, create a new token and give it to the program.

# The button of that dialog that revokes the token.
ui-api-token-revoke-confirm = Revoke the token

# Said in that dialog while the daemon revokes the token.
ui-api-token-revoking = Revoking the token…

# Announced once a token is revoked.
# $label (text): the token's label.
ui-api-token-revoked = Token { $label } revoked.

# The heading of the form that creates a token.
ui-api-token-form-heading = Create an { -api-token }

# The label of the field of the token's label.
ui-api-token-label-label = Label

# The hint under that field.
# $max (number): the most characters a label may have.
ui-api-token-label-hint = Up to { $max } characters, so you can tell this token from the others: the program that uses it, for example.

# The button that creates the token.
ui-api-token-create = Create the token

# Said while the daemon creates the token.
ui-api-token-creating = Creating the token…

# The heading of the list of what is wrong with the form, which takes focus when the form is refused.
# $count (number): how many problems the list names.
ui-api-token-problems = { $count ->
    [one] There is { $count } problem with the form
   *[other] There are { $count } problems with the form
  }

# The title of the dialog that shows a new token's whole text, the only time it is shown.
ui-api-token-created-title = Copy the new { -api-token }

# What that dialog says: the token is shown this once, so it must be copied now.
# $label (text): the token's label.
ui-api-token-created-body = This is token { $label }. Copy it now and give it to the program that uses it: { -brand-name } doesn't keep the token itself, so it won't show it again. If it is lost, revoke it and create another.

# The label of the field that holds the new token's whole text, which can be selected and copied by hand.
ui-api-token-created-field = The new token

# The button that copies the new token to the clipboard.
ui-api-token-copy = Copy the token

# Said in the dialog once the token is on the clipboard.
ui-api-token-copied = Copied.

# Said in the dialog when the browser would not let the page write to the clipboard, as on a page not served over HTTPS or from this computer.
ui-api-token-copy-failed = The browser would not let the page copy it. Select the token in the field and copy it yourself.

## Settings' "Machines" tab, only on a hub (decision 121): the pairing token a
## person makes for another machine, confirmed by a step-up, shown once.

# The tab and the heading of the section of Settings that makes pairing tokens. The same word as the term -machine in its plural, written out because a heading starts with a capital letter.
ui-agents-heading = Machines

# What that section says above its form.
ui-agents-intro = To pair another { -machine } with this { -hub }, make a pairing token here and give it to { "ladderback pair" } on that { -machine }. A token works once, for a few minutes, and only for the address it names.

# A warning under that, shown only with sign-in off: a reverse proxy on the hub's own computer can make a request from anywhere look local, and with sign-in off the step-up is the hub key's ID, which anyone can read (decision 120).
ui-agents-proxy-sign-in = A { -hub } reached through a reverse proxy must turn sign-in on, by setting an admin password: a proxy on this computer can make a request from anywhere look as if it came from this computer, and with sign-in off anyone who can reach the proxy can then use { -brand-name } and make a pairing token.

# The link after that warning, to the guide's section on a hub behind a reverse proxy, on GitHub.
ui-agents-proxy-link = A { -hub } behind a reverse proxy, in the guide on GitHub

# The heading of the form that makes a pairing token.
ui-pairing-new-heading = Make a pairing token

# The label of the field of the address the other machine will reach this hub at.
ui-pairing-address-label = Address { -machine(form: "other") } will use

# The hint under that field.
ui-pairing-address-hint = The address another { -machine } reaches this { -hub } at, as { "https://hub.example:9443" }. It goes into the token.

# Said under the address when it names this computer alone, as localhost or 127.0.0.1.
ui-pairing-warning-loopback = This address is loopback: only this computer can reach it, so another { -machine } can't use the token. Type the address other { -machine(form: "other") } reach this { -hub } at.

# Said under the address when it starts with http:// and isn't loopback.
ui-pairing-warning-plain = This address is plain { "http://" }, which a { -machine } refuses: its key would cross the network unprotected. Use the { "https://" } address.

# Says which certificate a token tells the other machine to trust, on a hub that serves no certificate of its own, as when its TLS key couldn't be served.
ui-pairing-certificate = The token trusts a certificate authority's certificate for this address, as the system of the other { -machine } does. This { -hub } serves no certificate of its own, so no token can pin one.

# Said in place of the certificate choice when the address starts with http://, which shows no certificate, so the token names none, on a hub that serves a certificate of its own. `http://` and `https://` are typed as they are: keep them string literals.
ui-pairing-certificate-none = This address starts with { "http://" }, so the connection isn't encrypted and the token names no certificate. The { -hub } key in the token still proves that what answers there is the { -hub }. To choose a certificate, type the { -hub }'s { "https://" } address.

# The same on a hub that serves no certificate of its own, as when its TLS key couldn't be served, so an https:// address offers no choice either. `http://` is typed as it is: keep it a string literal.
ui-pairing-certificate-none-unserved = This address starts with { "http://" }, so the connection isn't encrypted and the token names no certificate. The { -hub } key in the token still proves that what answers there is the { -hub }.

# The legend of the choice of the certificate a token tells the other machine to trust.
ui-pairing-certificate-legend = Certificate the token trusts

# Said under the certificate choice. `https://` is typed as it is: keep it a string literal.
ui-pairing-certificate-hint = The { -hub }'s own certificate needs no certificate authority: the token pins it, and the address starts with { "https://" }. Choose a certificate authority's when a proxy in front of the { -hub } serves the address.

# The choice of the hub's own certificate, which the token pins.
ui-pairing-certificate-own = The { -hub }'s own certificate

# What the dialog lists as the certificate a token trusts when it pins the hub's own.
ui-pairing-certificate-pinned = The token is pinned to the { -hub }'s own certificate

# The label of the field of how long the token works.
ui-pairing-life-label = Life in minutes

# The hint under that field.
ui-pairing-life-hint = From 5 to 1440 minutes. A token no { -machine } has used by then lapses.

# Said under the life field when what is typed isn't a whole number of minutes.
ui-pairing-life-invalid = Type the life as a whole number of minutes, from 5 to 1440.

# The label of the field a step-up with sign-in off asks for: the hub key's fingerprint, which the section shows, as the hub's terminal and the pairing token's dialog show it.
ui-pairing-key-fingerprint-label = Hub key fingerprint

# The hint under that field, which shows the fingerprint to type or paste.
ui-pairing-key-fingerprint-hint = To confirm, type or paste this { -hub } key's fingerprint:

# Said when the step-up is wrong: no token was made.
ui-pairing-step-up-failed = That isn't right, so no token was made. Try again.

# The button that makes the token.
ui-pairing-make = Make a pairing token

# The title of the dialog that shows a new pairing token, the only time it is shown.
ui-pairing-dialog-title = Copy the pairing token

# What that dialog says above the token.
ui-pairing-dialog-body = Give this token to { "ladderback pair" } on the other { -machine } before it lapses. It is shown only this once. Check there that the { -hub } key's fingerprint is the one below.

# The name of the fingerprint of the hub's key, as the dialog lists it.
ui-pairing-fingerprint = Hub key fingerprint

# The name of the address the token names, as the dialog lists it.
ui-pairing-address = Address

# The name of the certificate the token trusts, as the dialog lists it.
ui-pairing-certificate-label = Certificate

# What the dialog lists as the certificate the token trusts.
ui-pairing-certificate-ca = A certificate authority's certificate

# The name of when the token lapses, as the dialog lists it.
ui-pairing-lapses-label = Lapses

# When the token lapses, counted down while the dialog is open.
# $when (text): the time it lapses, in this computer's zone, or in UTC where the browser names no zone.
# $offset (text): the offset from UTC of the zone $when is written in, as -05:00.
# $minutes (number): the whole minutes left, rounded up.
ui-pairing-lapses = { $when } ({ $offset }), in { $minutes ->
    [one] { $minutes } minute
   *[other] { $minutes } minutes
  }

# The name of the token's text in the dialog.
ui-pairing-token = The pairing token

# The button that copies the token to the clipboard.
ui-pairing-copy = Copy the token

# Said in the dialog once the token is on the clipboard.
ui-pairing-copied = Copied.

# Said in the dialog when the browser would not let the page write to the clipboard.
ui-pairing-copy-failed = The browser would not let the page copy it. Select the token and copy it yourself.

# The button that cancels the token before any machine uses it.
ui-pairing-cancel = Cancel this token

# Said beside the dialog's Close button.
ui-pairing-close-note = Closing this dialog cancels the token if no { -machine } has used it.

# Said in the dialog in place of the token once it is canceled.
ui-pairing-canceled = The token is canceled. No { -machine } can use it.

# Said in the dialog in place of the token once it lapses.
# $when (text): the time it lapsed, in this computer's zone, or in UTC where the browser names no zone.
ui-pairing-lapsed = Lapsed at { $when }. Make a new token to pair a { -machine }.

# The button that closes the dialog once its token is canceled or lapsed, back to the form.
ui-pairing-make-new = Make a new token

# Said on the page once the dialog is closed and the hub canceled the token no machine had used.
ui-pairing-closed-canceled = The unused pairing token was canceled.

# Said on the page once the dialog is closed and the hub refused or didn't answer the cancel of the token no machine had used.
ui-pairing-closed-not-canceled = The { -hub } couldn't cancel the unused pairing token, so it still works until it lapses.

# Said on the page when the hub refused or didn't answer the cancel of a closed dialog's token after the dialog showed a later one.
ui-pairing-earlier-not-canceled = The { -hub } couldn't cancel an earlier unused pairing token, so it still works until it lapses.

# Said when the hub didn't answer the page's request about a pairing token or the approval of a waiting machine, or answered with what isn't one.
ui-pairing-unanswered = The { -hub } didn't answer. Try again.

## Settings' "Machines" tab, only on a hub (decision 121): the machines
## waiting for approval, approved there after a review and a step-up, and the
## machines approved.

# The heading of the table of the machines waiting for approval, which names that table.
ui-machines-waiting-heading = Waiting for approval

# The heading of the table of the approved machines, which names that table.
ui-machines-approved-heading = Approved machines

# The header of the column of each machine's hostname.
ui-machines-hostname = Hostname

# The header of the column of each machine's agent ID, which the hub gave it when it paired.
ui-machines-agent-id = Agent ID

# The header of the column of each machine's key fingerprint, in full.
ui-machines-key = Key fingerprint

# The header of the column of how long each machine has waited for approval.
ui-machines-waited = Waiting

# The header of the column of a note on a waiting machine, as when its hostname looks like another machine's.
ui-machines-note = Note

# The header of the column of each waiting machine's Approve button.
ui-machines-action = Action

# The header of the column of how long ago each machine was approved.
ui-machines-approved-since = Approved

# How long a machine has waited for approval.
# $duration (duration): that length of time, such as 1 min 5 s.
ui-machines-waited-for = for { $duration }

# How long ago a machine was approved.
# $duration (duration): that length of time, such as 1 min 5 s.
ui-machines-approved-ago = { $duration } ago

# The name of a machine's key fingerprint, as its row and the review show it.
# $hostname (text): the machine's hostname.
ui-machines-fingerprint = { $hostname }'s key fingerprint

# The note on a waiting machine whose hostname looks like another machine's, as "demo-rnacbook" looks like "demo-macbook".
# $hostname (text): the other machine's hostname.
ui-machines-looks-like = Looks like { $hostname }

# The button that opens the review of approving a waiting machine, and the review's button that approves it.
# $hostname (text): the machine's hostname.
ui-machines-approve = Approve { $hostname }

# The title of the review of approving a waiting machine.
# $hostname (text): the machine's hostname.
ui-machines-review-title = Approve { $hostname }?

# What the review says first.
# $hostname (text): the machine's hostname.
ui-machines-review-body = Approve only a { -machine } you paired. Compare this fingerprint with the one { "ladderback pair" } shows on { $hostname }: once approved, it can sync with this { -hub }.

# The warning in the review when the machine's hostname looks like another machine's.
# $hostname (text): the other machine's hostname.
ui-machines-review-lookalike = This hostname looks like { $hostname }, another { -machine } of this { -hub }. Check that it is the { -machine } you paired.

# Said on the page once a machine is approved.
# $hostname (text): the machine's hostname.
ui-machines-approved = { $hostname } is approved.

# Said in the review while the hub approves the machine, which holds the review still.
# $hostname (text): the machine's hostname.
ui-machines-approving = Approving { $hostname }…

# Said in the review when the step-up is wrong: the machine wasn't approved.
ui-machines-step-up-failed = That isn't right, so the { -machine } wasn't approved. Try again.

# The button that opens the review of rejecting a waiting machine, and the review's button that rejects it.
# $hostname (text): the machine's hostname.
ui-machines-reject = Reject { $hostname }

# The button that opens the review of removing an approved machine, and the review's button that removes it.
# $hostname (text): the machine's hostname.
ui-machines-remove = Remove { $hostname }

# The title of the review of rejecting a waiting machine.
# $hostname (text): the machine's hostname.
ui-machines-reject-title = Reject { $hostname }?

# The title of the review of removing an approved machine.
# $hostname (text): the machine's hostname.
ui-machines-remove-title = Remove { $hostname }?

# What the review of rejecting a waiting machine says first. A paired machine maintains only under its hub's lease, so a rejected one runs no maintenance.
# $hostname (text): the machine's hostname.
ui-machines-reject-body = { $hostname } stops waiting and can't sync with this { -hub }, or pair with it again under its ID. It keeps backing up and runs no maintenance.

# What the review of removing an approved machine says first. A paired machine maintains only under its hub's lease, so a removed one stops maintaining.
# $hostname (text): the machine's hostname.
ui-machines-remove-body = { $hostname } can't sync with this { -hub } any more, or pair with it again under its ID. It keeps backing up and stops running maintenance.

# Said on the page once a waiting machine is rejected.
# $hostname (text): the machine's hostname.
ui-machines-rejected = { $hostname } is rejected.

# Said on the page once an approved machine is removed.
# $hostname (text): the machine's hostname.
ui-machines-removed = { $hostname } is removed.

# Said in the review while the hub rejects the machine, which holds the review still.
# $hostname (text): the machine's hostname.
ui-machines-rejecting = Rejecting { $hostname }…

# Said in the review while the hub removes the machine, which holds the review still.
# $hostname (text): the machine's hostname.
ui-machines-removing = Removing { $hostname }…

# Shown in place of the tables while the page reads the machines.
ui-machines-loading = Loading the { -machine(form: "other") }…

# Shown in place of the waiting machines' table when none is waiting.
ui-machines-none-waiting = No { -machine } is waiting for approval.

# Shown in place of the approved machines' table when none is approved.
ui-machines-none-approved = No { -machine } is approved yet.

# Shown in place of the tables when the hub didn't answer the page's request for its machines.
ui-machines-unreachable = The { -hub } didn't answer, so its { -machine(form: "other") } can't be listed. The page asks again in a few seconds.

# Said politely when live updates on the Machines tab are on again, in place
# of the generic sentence that counts the runs that ended.
# $count (number): how many machines started to wait for approval while the
# updates were paused, which the tab now lists.
ui-machines-resumed =
    { $count ->
        [0] Live updates are on again. No { -machine } started waiting while they were paused.
        [one] Live updates are on again. One { -machine } started waiting while they were paused.
       *[other] Live updates are on again. { $count } { -machine(form: "other") } started waiting while they were paused.
    }

# Said politely when live updates on the Machines tab are on again after a
# pause taken before the tab had listed any machine, so none can be counted
# as having started to wait meanwhile.
ui-machines-resumed-unlisted = Live updates are on again.

# Said politely when a machine pairs with the hub while the Machines tab is open and its live updates are on (#569).
# $hostname (string): the name the machine gave.
ui-machines-started-waiting = { $hostname } is waiting for approval.

# A link to the dashboard of the approved machines, on the Machines tab and, on a hub, on the start page.
ui-machines-dashboard-link = See the approved { -machine(form: "other") }' latest operations

## The hub's dashboard (#569): each approved machine, when it last connected,
## the reports the hub let go of, and its latest operations, as its reports
## told them.

# The dashboard's heading.
ui-dashboard-heading = Dashboard

# The dashboard's title, as the browser's tab and history show it.
ui-dashboard-title = Dashboard – { -brand-name }

# What the dashboard shows, under its heading.
ui-dashboard-intro = Each approved { -machine }'s latest operations, as it reported them to this { -hub }. A { -machine } connects only to send its reports, so this page shows when it last connected, not whether it is online.

# Shown while the dashboard's first read of the machines is on its way.
ui-dashboard-loading = Loading the { -machine(form: "other") }…

# Shown in place of the machines when none is approved.
ui-dashboard-none = No { -machine } is approved yet. Approve one on the Machines tab of Settings.

# Shown in place of the machines when the hub didn't answer the page's request for them.
ui-dashboard-unreachable = The { -hub } didn't answer, so its { -machine(form: "other") } can't be shown. The page asks again in a few seconds.

# When the machine last connected to the hub.
# $when (string): the time, as ui-instant or ui-dashboard-time-elsewhere writes it.
ui-dashboard-last-connected = Last connected { $when }.

# An instant in the viewer's zone, then in the machine's own zone, which differs (#115), written as ui-instant writes one.
# $time (instant): the instant in the viewer's zone.
# $there (instant): the same instant in the machine's zone.
# $zone (string): the machine's IANA time-zone name, such as America/Chicago.
ui-dashboard-time-elsewhere = { DATETIME($time, dateStyle: "short", timeStyle: "medium") } ({ DATETIME($there, dateStyle: "short", timeStyle: "medium") } in { $zone })

# A warning on a machine whose oldest reports the hub let go of past its cap, so some of its operations may be missing.
# $count (number): how many reports it let go of.
ui-dashboard-dropped =
    { $count ->
        [one] The { -hub } let go of this { -machine }'s oldest report to keep within its limit, so an older operation may be missing.
       *[other] The { -hub } let go of { $count } of this { -machine }'s oldest reports to keep within its limit, so older operations may be missing.
    }

# The caption of a machine's table of operations.
# $hostname (string): the machine's name.
ui-dashboard-operations = Latest operations of { $hostname }

# Shown in place of a machine's operations when it reported none.
ui-dashboard-no-operations = No operation reported yet.

# The headers of a machine's operations' columns: when each started, when it ended, how it came out, and the error code it failed with.
ui-dashboard-started = Started
ui-dashboard-ended = Ended
ui-dashboard-outcome = Result
ui-dashboard-code = Error code

# A start or end the hub has no report of: the machine let its start go when its outbox was full, or it hasn't ended.
ui-dashboard-unknown = Not reported

# The name of the progress bar of the operation a machine runs, as its latest report to the hub told it (decision 83).
# $hostname (string): the machine's name.
ui-dashboard-progress = Progress of the operation running on { $hostname }

# The caption of a machine's table of plans, as it last reported them.
# $hostname (string): the machine's name.
ui-dashboard-plans = Plans of { $hostname }

# Shown in place of a machine's plans before its first report of them.
ui-dashboard-no-plans = This { -machine } hasn't reported its plans yet.

# Shown in place of a machine's plans when its report listed none.
ui-dashboard-plans-none = This { -machine } reported no plans.

# The headers of a machine's plans' columns: the plan, how its last run went and when, and when it runs next.
ui-dashboard-plan = Plan
ui-dashboard-last-run = Last run
ui-dashboard-next-run = Next run

# A plan the machine has never run.
ui-dashboard-never-run = Not run yet

# A plan with no run planned, such as one without a schedule.
ui-dashboard-not-planned = None planned

# A warning on a machine that has more plans than it reports to the hub, so the table leaves some out.
# $count (number): how many plans it left out.
ui-dashboard-plans-left-out =
    { $count ->
        [one] This { -machine } has one more plan than it reports to the { -hub }, so that plan isn't shown.
       *[other] This { -machine } has { $count } more plans than it reports to the { -hub }, so those plans aren't shown.
    }

# Said politely when live updates on the dashboard are on again.
# $count (number): how many machines changed while the updates were paused, which the page now shows.
ui-dashboard-resumed =
    { $count ->
        [0] Live updates are on again. No { -machine } changed while they were paused.
        [one] Live updates are on again. One { -machine } changed while they were paused.
       *[other] Live updates are on again. { $count } { -machine(form: "other") } changed while they were paused.
    }

## Settings' "Dry-run allowlist" tab (decision 89): the folders a plan dry
## run an AI agent asks for may read, which only a person sets.

# The tab and the heading of the section of Settings that lists the dry-run allowlist. It names the table of folders too. The same words as the term -dry-run-allowlist, written out because a heading starts with a capital letter.
ui-dry-run-allowlist-heading = Dry-run allowlist

# What that section says above the list.
ui-dry-run-allowlist-body = An AI agent can ask for a plan dry run: { -brand-name } counts the files and bytes a draft plan would back up and tells the AI agent only counts and sizes, never a file's name. It reads only within these folders and the folders inside them.

# Shown in place of the list when it holds no folder.
ui-dry-run-allowlist-none = No folders yet, so a plan dry run an AI agent asks for refuses every source. Add a folder to let it count what a draft plan would back up there.

# The header of the table's column of folders, each row's header.
ui-dry-run-allowlist-column-folder = Folder

# The button of a folder's row that asks before removing it from the list, named for the folder.
# $folder (text): the folder's full path.
ui-dry-run-folder-remove = Remove { $folder }

# The title of the dialog that asks before a folder is removed from the list.
# $folder (text): the folder's full path.
ui-dry-run-folder-remove-title = Remove { $folder } from the { -dry-run-allowlist }

# What that dialog says.
ui-dry-run-folder-remove-body = A plan dry run an AI agent asks for can no longer read this folder, unless another folder in the list holds it. The folder itself, the plans and their backups don't change.

# The dialog's button that removes the folder. Cancel comes first.
ui-dry-run-folder-remove-confirm = Remove the folder

# Said in the dialog while the daemon saves the list without the folder.
ui-dry-run-folder-removing = Removing the folder…

# Announced once the folder is removed from the list.
# $folder (text): the folder's full path.
ui-dry-run-folder-removed = Folder { $folder } removed from the { -dry-run-allowlist }.

# The heading of the form that adds a folder to the list.
ui-dry-run-folder-form-heading = Add a folder

# The label of the field that takes the folder's path.
ui-dry-run-folder-label = Folder

# The hint under that field.
ui-dry-run-folder-hint = The folder's full path. A plan dry run may then read it and every folder inside it.

# The button that adds the folder typed.
ui-dry-run-folder-add = Add the folder

# Said while the daemon saves the list with the folder.
ui-dry-run-folder-adding = Adding the folder…

# Announced once the folder is added to the list.
# $folder (text): the folder's full path.
ui-dry-run-folder-added = Folder { $folder } added to the { -dry-run-allowlist }.

# The field's error when nothing was typed in it.
ui-dry-run-folder-empty = Give the folder's full path.

# The field's error when the list holds the folder typed already.
ui-dry-run-folder-listed = The list holds this folder already.

# The field's error when the daemon refused the folder typed because it is not a full path, with a full path as an example.
# $folder (text): the folder as it was typed.
# $example (text): a full path, the home folder's Documents folder, such as /home/demo/Documents.
ui-dry-run-folder-not-full = { $folder } is not a full path. Give the folder's whole path, such as { $example }.

# The same error where the daemon names no home folder to build an example from.
# $folder (text): the folder as it was typed.
ui-dry-run-folder-not-full-plain = { $folder } is not a full path. Give the folder's whole path, starting at the top of its drive.

# An error alert after the form or in a dialog: the list would hold more folders than it may, so the change was not saved.
# $limit (count): how many folders it may hold.
ui-dry-run-allowlist-too-long = The { -dry-run-allowlist } may hold at most { $limit } folders, so the change was not saved. Remove a folder you no longer need first.

# The button that asks before the home folder of the computer Ladderback runs on is added to the list.
ui-dry-run-home-add = Add home folder

# The title of the dialog that asks before the home folder is added.
ui-dry-run-home-title = Add your home folder to the { -dry-run-allowlist }?

# What that dialog warns of.
# $home (text): the home folder's full path.
# $keys (text): "~/.ssh", the folder of a person's SSH keys, the same in every language.
ui-dry-run-home-body = This lets an AI agent count the files under your whole home folder, { $home }, including { $keys } and browser profiles. It learns only counts and sizes, never a file's name, but a count still tells it whether files are there.

# What that dialog suggests instead, after the warning.
ui-dry-run-home-narrower = A narrower folder is safer, such as the one a draft plan backs up. Cancel, then add it with the Folder field.

# The dialog's button that adds the home folder. Cancel comes first.
ui-dry-run-home-confirm = Add the home folder

## The "Pending changes" page (decision 89): the proposals an AI agent made
## through the API, each approved or rejected as a whole by a person.

# The "Pending changes" page's title, as the browser's tab and history show it.
ui-proposals-title = Pending changes – { -brand-name }

# The "Pending changes" page's heading: its one main heading, which also names the list of proposals.
ui-proposals-heading = Pending changes

# What the "Pending changes" page is for, under its heading.
ui-proposals-body = An AI agent, or another program with an API token, can propose changes to the configuration. Nothing changes until you approve a proposal as a whole here. Reject one to tell the AI agent why, change by change.

# Said under the heading while sign-in is off, as decision 89 requires: nothing then tells a person's browser from a program's request. The Admin password tab is the tab of Settings named as `ui-admin-password-heading` words it; a password set there turns sign-in on only after a restart.
ui-proposals-sign-in-off = Sign-in is off, so approving here is a convenience, not a safety boundary: a program that can reach { -brand-name } could approve its own proposal. To make approval yours alone, set an admin password in the Admin password tab of Settings and restart { -brand-name }.

# Said in the list's place when no proposal is pending.
ui-proposals-none = No proposal is pending.

# The button that opens a pending proposal an API token made.
# $proposal (text): the proposal's ID.
# $proposer (text): the label of the API token that proposed it.
ui-proposals-review = Review proposal { $proposal } from { $proposer }

# The button that opens a pending proposal made with no API token, as with sign-in off.
# $proposal (text): the proposal's ID.
ui-proposals-review-no-token = Review proposal { $proposal }, made without an API token

# The heading of the proposal opened on the page.
# $proposal (text): the proposal's ID.
ui-proposal-heading = Proposal { $proposal }

# The heading of what the proposal's changes do, which Ladderback writes, never the AI agent.
ui-proposal-summary-heading = What it changes

# A line of the summary: the proposal adds a plan.
# $plan (text): the plan's ID.
ui-proposal-adds-plan = Adds the plan { $plan }.

# A line of the summary: the proposal changes a plan, followed by a list of the settings it changes.
# $plan (text): the plan's ID.
ui-proposal-changes-plan = Changes these settings of the plan { $plan }:

# A line of the summary: the proposal removes a plan. Its snapshots stay in the repository.
# $plan (text): the plan's ID.
ui-proposal-removes-plan = Removes the plan { $plan }. Its { -snapshot(form: "other") } stay in the { -repository }.

# A setting a changed plan's summary line lists: the repository it backs up to.
ui-proposal-setting-repository = its { -repository }

# A setting a changed plan's summary line lists: the host its snapshots are recorded as taken on.
ui-proposal-setting-host = its host

# A setting a changed plan's summary line lists: the label its snapshots get.
ui-proposal-setting-label = its label

# A setting a changed plan's summary line lists: the folders it backs up.
ui-proposal-setting-sources = the folders it backs up

# A setting a changed plan's summary line lists: the tags its snapshots get.
ui-proposal-setting-tags = its tags

# A setting a changed plan's summary line lists: when it runs.
ui-proposal-setting-schedule = its schedule

# A setting a changed plan's summary line lists: what happens when a run is due while one is going.
ui-proposal-setting-overlap = what happens when runs overlap

# A setting a changed plan's summary line lists: how a failed run is tried again.
ui-proposal-setting-retry = how a failed run is tried again

# A setting a changed plan's summary line lists: what it leaves out of a backup.
ui-proposal-setting-filter = what it leaves out

# A setting a changed plan's summary line lists: which snapshots it keeps.
ui-proposal-setting-retention = which { -snapshot(form: "other") } it keeps

# The heading of the AI agent's own rationale, shown apart from the summary, as its words, not Ladderback's.
ui-proposal-rationale-heading = The AI agent's words

# Said in the rationale's place when the AI agent gave none.
ui-proposal-rationale-none = The AI agent gave no reason.

# Said when the configuration file has changed since the proposal was made, by a comment edit for example; the diff is rebuilt against the file as it is now.
ui-proposal-changed-since = The configuration file has changed since this proposal was made. The changes below are worked out again against the file as it is now.

# How often a person rejected the proposals of whoever made this one.
# $count (count): how many of its kept proposals were rejected.
ui-proposal-rejected-before = { $count ->
    [0] None of this proposer's proposals was rejected before.
    [one] One of this proposer's proposals was rejected before.
   *[other] { $count } of this proposer's proposals were rejected before.
}

# The heading of the exact changes to the configuration file, with each secret it holds shown as ***.
ui-proposal-diff-heading = Changes to the configuration file

# Said in the place of Approve for a proposal that could delete snapshots, until the person shows its forget preview: approving it comes with that preview.
ui-proposal-needs-forget-preview = This proposal could delete { -snapshot(form: "other") }. Show what it would forget first: you can approve it once the forget preview shows which ones would go.

# Said in the place of Approve once the forget preview couldn't read a repository the proposal touches: it can be approved only from a preview that read them all.
ui-proposal-forget-preview-unread = A repository this proposal touches couldn't be read, so it can't be approved from this preview. Show what it would forget again once the repository can be read.

# The button that reads what the open proposal would change about the snapshots retention forgets, in each repository it touches.
ui-proposal-forget-preview = Show what it would forget

# Said while the forget preview reads the repositories.
ui-proposal-forget-previewing = Reading what it would forget…

# The heading of the forget preview.
ui-proposal-forget-heading = What it would forget

# The heading of one repository in the forget preview.
# $repository (text): the repository's alias, such as demo.
ui-proposal-forget-repository = Repository { $repository }

# Said of a repository this machine maintains neither before nor after the proposal.
ui-proposal-forget-not-maintained = { -brand-name } doesn't maintain this { -repository }, before or after the proposal, so its retention forgets nothing there.

# Said of a repository this machine doesn't maintain now.
ui-proposal-forget-before-not-maintained = Now, { -brand-name } doesn't maintain this { -repository }, so its retention forgets nothing there.

# Said of a repository this machine wouldn't maintain after the proposal.
ui-proposal-forget-after-not-maintained = After the proposal, { -brand-name } wouldn't maintain this { -repository }, so its retention would forget nothing there.

# Said of a repository where no plan owns any snapshots now.
ui-proposal-forget-before-no-snapshots = Now, no plan owns any { -snapshot(form: "other") } in this { -repository }, so retention forgets nothing there.

# Said of a repository where no plan would own any snapshots after the proposal.
ui-proposal-forget-after-no-snapshots = After the proposal, no plan would own any { -snapshot(form: "other") } in this { -repository }, so retention would forget nothing there.

# Said of a repository where a forget is refused now, so it forgets nothing, because a plan's policy keeps nothing.
# $plan (text): the plan's ID.
ui-proposal-forget-before-keeps-nothing = Now, a forget in this { -repository } is refused, because the policy of the plan { $plan } keeps nothing, so retention forgets nothing there.

# Said of a repository where a forget would be refused after the proposal, so it would forget nothing, because a plan's policy would keep nothing.
# $plan (text): the plan's ID.
ui-proposal-forget-after-keeps-nothing = After the proposal, a forget in this { -repository } would be refused, because the policy of the plan { $plan } would keep nothing, so retention would forget nothing there.

# What retention forgets of one plan now.
# $plan (text): the plan's ID. $remove (count): how many of its snapshots retention forgets. $total (count): how many it owns.
ui-proposal-forget-before =
    { $total ->
        [one] Now, retention forgets { $remove } of the 1 { -snapshot(form: "one") } of the plan { $plan }.
       *[other] Now, retention forgets { $remove } of the { $total } { -snapshot(form: "other") } of the plan { $plan }.
    }

# What retention would forget of one plan after the proposal.
# $plan (text): the plan's ID. $remove (count): how many of its snapshots retention would forget. $total (count): how many it would own.
ui-proposal-forget-after =
    { $total ->
        [one] After the proposal, retention would forget { $remove } of the 1 { -snapshot(form: "one") } of the plan { $plan }.
       *[other] After the proposal, retention would forget { $remove } of the { $total } { -snapshot(form: "other") } of the plan { $plan }.
    }

# Said when the proposal forgets neither more nor less in the repository.
ui-proposal-forget-same = The proposal changes nothing retention forgets in this { -repository }.

# The caption of the snapshots the proposal would forget that retention keeps now.
# $plan (text): the plan's ID. $count (count): how many.
ui-proposal-forgets-more =
    { $count ->
        [one] After the proposal, retention would forget 1 { -snapshot(form: "one") } of the plan { $plan } that it keeps now
       *[other] After the proposal, retention would forget { $count } { -snapshot(form: "other") } of the plan { $plan } that it keeps now
    }

# The caption of the snapshots retention forgets now and would keep after the proposal.
# $plan (text): the plan's ID. $count (count): how many.
ui-proposal-forgets-less =
    { $count ->
        [one] After the proposal, retention would no longer forget 1 { -snapshot(form: "one") } of the plan { $plan }
       *[other] After the proposal, retention would no longer forget { $count } { -snapshot(form: "other") } of the plan { $plan }
    }

# The heading of the part of the page that approves the proposal.
ui-proposal-approve-heading = Approve

# The label of the step-up field with sign-in off: the proposal's short ID confirms the approval.
ui-proposal-id = Proposal ID

# The step-up field's hint with sign-in off, before the short ID shown to copy.
ui-proposal-id-hint = Type or paste this proposal's short ID to confirm:

# The button that approves the proposal as the page shows it.
ui-proposal-approve = Approve the proposal

# Said beside Approve while the approval is applied.
ui-proposal-approving = Applying the proposal…

# The step-up field's error with sign-in off, when what was typed is not the proposal's short ID. Nothing was applied.
# $length (count): how many characters the short ID has.
ui-proposal-wrong-id = That is not this proposal's ID, so nothing was applied. Type or paste the { $length } characters shown above.

# The step-up field's error with sign-in on, when what was typed is not the admin password. Nothing was applied.
ui-proposal-wrong-password = That is not the admin password, so nothing was applied. Type it again, or fill it in from your password manager.

# Announced once the proposal is applied.
# $proposal (text): the proposal's ID.
ui-proposal-applied = Proposal { $proposal } is applied, and the configuration file is saved.

# The heading of the part of the page that rejects the proposal.
ui-proposal-reject-heading = Reject

# What rejecting does, under its heading.
ui-proposal-reject-body = Give a reason for each change you want done another way. The AI agent reads them and can propose again.

# The field for the reason a change is rejected.
# $change (count): the change's place in the proposal, from 1.
ui-proposal-reason = Reason for change { $change } (optional)

# The hint under each reason field: what the AI agent does with it.
ui-proposal-reason-hint = What should this change do differently? Leave it blank if the change is fine as it is.

# The button that rejects the proposal with the reasons typed.
ui-proposal-reject = Reject the proposal

# Said beside Reject while the rejection is sent.
ui-proposal-rejecting = Rejecting the proposal…

# Announced once the proposal is rejected.
# $proposal (text): the proposal's ID.
ui-proposal-rejected = Proposal { $proposal } is rejected. The AI agent can read your reasons.

## The Plans page (§4.4): the plans of the configuration file, created,
## changed and deleted in a form whose every change is written to the file.

# The Plans page's title, as the browser's tab and history show it.
ui-plans-title = Plans – { -brand-name }

# The Plans page's heading: its one main heading, which also names the table of plans.
ui-plans-heading = Plans

# What the Plans page is for, under its heading.
ui-plans-body = A plan says what to back up, to which { -repository }, when, and which { -snapshot(form: "other") } to keep. Each change you save here is written to the configuration file, and the file keeps its comments.

# Said in the table's place when the configuration file has no plan.
ui-plans-none = There are no plans yet.

# The header of the column of each plan's repository.
ui-plans-column-repository = Repository

# The button that opens the form for a new plan.
ui-plan-add = Add a plan

# The button that opens the form that changes a plan, named for it.
# $plan (text): the plan's ID, such as demo-home.
ui-plan-change = Change { $plan }

# The button that asks before a plan is deleted, named for it.
# $plan (text): the plan's ID, such as demo-home.
ui-plan-delete = Delete { $plan }

# The heading of the form for a new plan.
ui-plan-form-new = New plan

# The heading of the form that changes a plan.
# $plan (text): the plan's ID, such as demo-home.
ui-plan-form-change = Change the plan { $plan }

# The legend of the form's group of what a plan backs up and leaves out.
ui-plan-group-sources = What to back up

# The legend of the form's group of when a plan runs on its own.
ui-plan-group-schedule = When to back up

# The legend of the form's group of which snapshots forget keeps.
ui-plan-group-retention = Which { -snapshot(form: "other") } to keep

# The legend of the form's group of the commands run around each backup.
ui-plan-group-hooks = Commands to run

# The label of each of the plan form's fields.
# $field (text): the field, one of { "name" }, { "repository" }, { "sources" }, { "globs" }, { "iglobs" }, { "exclude-if-present" }, { "git-ignore" }, { "cron" }, { "keep-last" }, { "keep-hourly" }, { "keep-daily" }, { "keep-weekly" }, { "keep-monthly" }, { "keep-yearly" }, { "run-before" }, { "run-after" }, { "run-failed" } or { "run-finally" }.
ui-plan-field-label = { $field ->
    [repository] Repository
    [sources] Folders to back up
    [globs] Leave out what matches
    [iglobs] Leave out what matches, whatever its case
    [exclude-if-present] Leave out folders holding
    [git-ignore] Leave out what { ".gitignore" } files name
    [cron] Cron schedule
    [keep-last] Keep the last
    [keep-hourly] Keep one an hour for
    [keep-daily] Keep one a day for
    [keep-weekly] Keep one a week for
    [keep-monthly] Keep one a month for
    [keep-yearly] Keep one a year for
    [run-before] Before each backup
    [run-after] After each backup that succeeds
    [run-failed] After each backup that fails
    [run-finally] After every backup
   *[name] Name
  }

# The hint under each of the plan form's fields, describing its input.
# $field (text): the field, as for ui-plan-field-label.
ui-plan-field-hint = { $field ->
    [repository] The alias of the { -repository } the backups go to, as the Repositories page lists it.
    [sources] One folder or file a line.
    [globs] One glob a line, as the { "globs" } setting of rustic takes them.
    [iglobs] One glob a line, matched whatever the case of its letters, as the { "iglobs" } setting of rustic takes them.
    [exclude-if-present] One file name a line: a folder that holds a file of that name is left out, with everything in it.
    [git-ignore] What each { ".gitignore" } file in the folders names is left out of the backup.
    [cron] Five fields, minute, hour, day of the month, month and day of the week, such as { "0 3 * * *" } for each night at 3:00. Leave it empty to run the plan only when it is started.
    [keep-last] How many of the newest { -snapshot(form: "other") } forget keeps.
    [keep-hourly] For how many of the latest hours forget keeps the newest { -snapshot } of each.
    [keep-daily] For how many of the latest days forget keeps the newest { -snapshot } of each.
    [keep-weekly] For how many of the latest weeks forget keeps the newest { -snapshot } of each.
    [keep-monthly] For how many of the latest months forget keeps the newest { -snapshot } of each.
    [keep-yearly] For how many of the latest years forget keeps the newest { -snapshot } of each.
    [run-before] One command line a line, run in order before the backup starts. Other users of this computer may see a command line while it runs, so keep passwords and tokens out of it.
    [run-after] One command line a line, run in order once the backup has succeeded. Other users of this computer may see a command line while it runs, so keep passwords and tokens out of it.
    [run-failed] One command line a line, run in order once the backup has failed. Other users of this computer may see a command line while it runs, so keep passwords and tokens out of it.
    [run-finally] One command line a line, run in order once the backup has ended, however it ended. Other users of this computer may see a command line while it runs, so keep passwords and tokens out of it.
   *[name] Up to 64 lower-case letters, digits and hyphens, starting with a letter or a digit. It is the plan's ID, which tags its { -snapshot(form: "other") }, and can't be changed once the plan is saved.
  }

# The button that saves the plan form.
ui-plan-save = Save the plan

# Said while the daemon saves the plan.
ui-plan-saving = Saving the plan…

# The button that closes the plan form without saving it.
ui-plan-cancel = Close without saving

# Announced once a plan is saved.
# $plan (text): the plan's ID, such as demo-home.
ui-plan-saved = The plan { $plan } is saved.

# Announced once a plan that had a schedule is saved without one: from then on it runs only when it is started by hand.
# $plan (text): the plan's ID, such as demo-home.
ui-plan-saved-unscheduled = The plan { $plan } is saved. It has no schedule now, so it runs only when you start it.

# The heading of the list of what is wrong with the plan form, which takes focus when the form is refused.
# $count (number): how many problems the list names.
ui-plan-problems = { $count ->
    [one] There is { $count } problem with the form
   *[other] There are { $count } problems with the form
  }

# The form's own words when the daemon refused the plan's cron schedule.
ui-plan-cron-invalid = Enter a cron schedule of five fields, such as { "0 3 * * *" }.

# The form's own words when the plan has no name.
ui-plan-name-missing = Enter a name for the plan.

# The form's own words when another plan already has the plan's name.
ui-plan-name-taken = Another plan already has this name. Enter another.

# The form's own words when the plan's name is not one a plan can have.
ui-plan-name-invalid = Enter a name of up to 64 lower-case letters, digits and hyphens, starting with a letter or a digit, such as { "home-docs" }.

# The form's own words when the plan names no repository.
ui-plan-repository-missing = Enter the alias of a { -repository }, as the Repositories page lists it.

# The form's own words when the plan names a repository the configuration doesn't have.
ui-plan-repository-unknown = No { -repository } has this alias. Enter one the Repositories page lists.

# The form's own words when the plan has nothing to back up.
ui-plan-sources-missing = Enter at least one folder or file to back up, one a line.

# The form's own words when a number of snapshots to keep is not a whole number.
ui-plan-count-invalid = Enter a whole number, such as { "7" }, or leave it empty.

# The title of the dialog that asks before a plan is deleted.
# $plan (text): the plan's ID, such as demo-home.
ui-plan-delete-title = Delete the plan { $plan }?

# What that dialog says first: deleting a plan leaves its snapshots where they are.
ui-plan-delete-stay = Its { -snapshot(form: "other") } stay in the { -repository }.

# What that dialog says next: forget removes only what a plan owns, so nothing forgets these snapshots by itself; the Snapshots page can, when they are selected.
ui-plan-delete-forget = Nothing forgets them on its own any more. You can still forget them by selecting them on the Snapshots page.

# What that dialog says last: a plan with the same ID would own these snapshots again.
# $plan (text): the plan's ID, such as demo-home.
ui-plan-delete-successor = A new plan with the ID { $plan } would take these { -snapshot(form: "other") } over and apply its own retention to them.

# The button in that dialog that deletes the plan.
ui-plan-delete-confirm = Delete the plan

# Said in that dialog while the daemon deletes the plan.
ui-plan-deleting = Deleting the plan…

# Announced once a plan is deleted.
# $plan (text): the plan's ID, such as demo-home.
ui-plan-deleted = The plan { $plan } is deleted. Its { -snapshot(form: "other") } stay in the { -repository }.
