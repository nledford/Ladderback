### The web UI's own text (technical design §8.4): what every page shares, and
### the start page. A message is named `ui-<where>-<what>`.

# The first link of every page. It moves the keyboard to the page's main content.
ui-skip-link = Skip to content

# The product's name, in the header of every page.
ui-brand = { -brand-name }

# The start page's title, as the browser's tab and history show it.
ui-start-title = { -brand-name }

# The start page's heading: the one main heading of the page.
ui-start-heading = Welcome to { -brand-name }

# The start page's text, until there is something to set up.
ui-start-body = Nothing is scheduled yet. Plans and their schedules arrive in later releases.

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
ui-gallery-polite-other-button = Say politely: agent back online

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

# What the second polite button announces.
ui-gallery-agent-online = Agent back online.

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

# The heading of the gallery's data table demo.
ui-gallery-table-heading = Data table

# The data table demo's caption.
ui-gallery-demo-repositories = Demo repositories

# The column header of the data table demo.
ui-gallery-repository-column = Repository

# The button in the table header that sorts the rows.
ui-gallery-sort-by-repository = Sort by repository

# Announced politely when the table is sorted ascending.
ui-gallery-sorted-ascending = Sorted by repository, ascending.

# A repository row of the data table demo.
ui-gallery-demo-repo-anchor = anchor

# A repository row of the data table demo.
ui-gallery-demo-repo-harbor = harbor

# A repository row of the data table demo.
ui-gallery-demo-repo-moor = moor

# The heading of the gallery's form field demo.
ui-gallery-form-heading = Form field

# The label of the demo form field.
ui-gallery-repository-name = Repository name

# The hint under the demo form field, describing its input.
ui-gallery-repository-hint = The name of the repository to check.

# The button that checks the demo form field.
ui-gallery-check-repository = Check the repository name

# The error the demo form field shows when its input is empty.
ui-gallery-repository-required = A repository name is required.

# The heading of the gallery's status badge demo.
ui-gallery-badge-heading = Status badge

# The loading badge of the status badge demo.
ui-gallery-badge-loading = Loading

# The empty badge of the status badge demo.
ui-gallery-badge-empty = Empty

# The error badge of the status badge demo.
ui-gallery-badge-error = Error

# The success badge of the status badge demo.
ui-gallery-badge-success = Success

# The heading of the gallery's progress demo.
ui-gallery-progress-heading = Progress

# The demo progress bar's accessible name.
ui-gallery-demo-backup-progress = Demo backup progress

# The button that advances the demo progress.
ui-gallery-advance-progress = Advance the demo progress

# The heading of the gallery's toast demo.
ui-gallery-toast-heading = Toast

# The button that saves the demo settings and shows a toast.
ui-gallery-save-settings = Save the demo settings

# The toast shown after the demo settings are saved.
ui-gallery-settings-saved = Settings saved.

## The operations page: the backup history, live (§4.4).

# The operations page's title, as the browser's tab and history show it.
ui-operations-title = Operations – { -brand-name }

# The operations page's heading: its one main heading.
ui-operations-heading = Operations

# The operations page's text, until its history loads.
ui-operations-body = The history of every backup, live.

# The operations table's caption.
ui-operations-caption = Operations

# The operations table's column of plan names.
ui-column-plan = Plan

# The operations table's column of statuses.
ui-column-status = Status

# The operations table's column of the instants runs started.
ui-column-started = Started

# The operations table's column of what a row offers to do.
ui-column-actions = Actions

# The button that starts a plan's backup.
ui-start-backup = Start backup

# The button that cancels a plan's running or queued backup, named with its
# row's plan.
ui-cancel-plan = Cancel { $plan }

# The button in the operations header that re-sorts the rows.
ui-sort-by-started = Sort by started

# Announced politely when the rows are sorted newest first.
ui-sorted-newest = Sorted by started, newest first.

# Announced politely when the rows are sorted oldest first.
ui-sorted-oldest = Sorted by started, oldest first.

# What the operations page says when the daemon holds no operations.
ui-no-operations = No operations yet.

# The label of a running backup's progress bar.
ui-backup-progress = Backup progress

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

# Announced politely when a backup ends well.
ui-backup-finished = Backup finished.

# Announced politely when a backup is cancelled.
ui-backup-cancelled = Backup cancelled.

# Announced assertively when a backup ends badly.
ui-backup-failed = Backup failed.

# An instant, as the page's locale writes it.
ui-instant = { DATETIME($when, dateStyle: "short", timeStyle: "medium") }

# The heading a live page shows when the daemon cannot be reached at all.
ui-unreachable-heading = Cannot reach { -brand-name }

# What that page says about its state.
ui-unreachable-body = The daemon that serves this page is not answering. It comes back on its own.

# What a live page says while its first answer is still on its way.
ui-loading = Loading…

## The snapshots page: one repository's snapshots, newest first (§4.4).

# The snapshots page's title, as the browser's tab and history show it.
ui-snapshots-title = Snapshots – { -brand-name }

# The snapshots page's heading: its one main heading.
ui-snapshots-heading = Snapshots

# The snapshots page's text, until a repository is chosen.
ui-snapshots-body = The snapshots of a repository, newest first.

# The snapshots table's caption.
ui-snapshots-caption = Snapshots

# The snapshots table's column of snapshot IDs.
ui-column-snapshot = Snapshot

# The snapshots table's column of the instants snapshots were taken.
ui-column-time = Time

# The button that shows one repository's snapshots, named with the repository.
ui-snapshots-of = Snapshots of { $alias }

# What the snapshots page says when the repository holds no snapshots.
ui-no-snapshots = No snapshots yet.

# Announced politely when the settle-poll after a start or a cancel ran out
# of its budget without the store ever saying how the run ended.
ui-still-waiting = Still waiting for Ladderback to say how the run ended.

## The navigation every page shares.

# The accessible name of the navigation in the header, the list of links to the pages.
ui-nav-label = Pages

# The navigation's link to the operations page.
ui-nav-operations = Operations

# The navigation's link to the snapshots page.
ui-nav-snapshots = Snapshots

# The navigation's link to the repositories page.
ui-nav-repositories = Repositories

## The repositories page: the repositories Ladderback backs up to, and the
## form that tests, connects or creates one (§4.4).

# The repositories page's title, as the browser's tab and history show it.
ui-repositories-title = Repositories – { -brand-name }

# The repositories page's heading: its one main heading.
ui-repositories-heading = Repositories

# The repositories page's text, above the list.
ui-repositories-body = The repositories { -brand-name } backs up to.

# The repositories table's caption.
ui-repositories-caption = Repositories

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

# The hint under the password field. It says where the typed password goes.
ui-repositories-password-hint = { -brand-name } keeps it in its configuration file and never shows it again.

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
