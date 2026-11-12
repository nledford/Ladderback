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

# The heading of the gallery's checkbox demo.
ui-gallery-checkbox-heading = Checkbox

# The label of the demo checkbox that starts off.
ui-gallery-demo-notification = Send a demo notification

# The description under the demo checkbox that starts off.
ui-gallery-demo-notification-hint = A demo choice: turning it on changes nothing.

# The label of the demo checkbox that starts on. It has no description.
ui-gallery-demo-log = Keep the demo log

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

# Announced politely when the demo review is confirmed.
ui-gallery-demo-review-confirmed = Demo review confirmed.

# The accessible name and heading of the review demo that has nothing to do.
ui-gallery-demo-empty-review = Demo review with nothing to do

# What the review demo that has nothing to do says.
ui-gallery-demo-empty-review-body = There is nothing to change.

# The heading of the pause control's demo.
ui-gallery-pause-heading = Pause control

# The demo's pause control: it pauses the demo's own updates.
ui-gallery-pause-demo = Pause the demo updates

# A button that ends a demo run: said at once while updates are not paused,
# counted while they are.
ui-gallery-finish-demo-run = Finish a demo run

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

# The operations table's caption.
ui-operations-caption = Operations

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
# row's plan.
ui-cancel-plan = Cancel { $plan }

# The row header of a maintenance run: what ran and the repository it ran on,
# by the alias this agent gave the repository when the run started.
# $kind is the wire text of what ran: forget, prune, check or adopt; any other
# kind is an operation.
# $repository (text): the repository's alias. It is shown as it came.
ui-operation-of =
    { $kind ->
        [forget] Forget in { $repository }
        [prune] Prune of { $repository }
        [check] Check of { $repository }
        [adopt] Adoption in { $repository }
       *[other] Operation on { $repository }
    }

# The row header of a run the log names no plan or repository for, such as a
# maintenance run an earlier version logged: what ran.
# $kind is the wire text of what ran: backup, forget, prune, check or adopt;
# any other kind is an operation.
ui-operation =
    { $kind ->
        [backup] Backup
        [forget] Forget
        [prune] Prune
        [check] Check
        [adopt] Adoption
       *[other] Operation
    }

# The button that cancels a running or queued maintenance run, named as its
# row is, so each Cancel says which run it stops.
# $kind is the wire text of what runs: forget, prune, check or adopt; any other
# kind is an operation.
# $repository (text): the repository's alias. It is shown as it came.
ui-cancel-operation-of =
    { $kind ->
        [forget] Cancel the forget in { $repository }
        [prune] Cancel the prune of { $repository }
        [check] Cancel the check of { $repository }
        [adopt] Cancel the adoption in { $repository }
       *[other] Cancel the operation on { $repository }
    }

# The button that cancels a running or queued run the log names no plan or
# repository for.
# $kind is the wire text of what runs: backup, forget, prune, check or adopt;
# any other kind is an operation.
ui-cancel-operation =
    { $kind ->
        [backup] Cancel the backup
        [forget] Cancel the forget
        [prune] Cancel the prune
        [check] Cancel the check
        [adopt] Cancel the adoption
       *[other] Cancel the operation
    }

# Announced politely on the Operations page when a run it watched ended well,
# warnings or not. $kind is the wire text of what ran: backup, forget, prune,
# check or adopt; any other kind is an operation.
ui-operation-finished =
    { $kind ->
        [backup] Backup finished.
        [forget] Forget finished.
        [prune] Prune finished.
        [check] Check finished.
        [adopt] Adoption finished.
       *[other] Operation finished.
    }

# Announced politely on the Operations page when a run it watched was
# cancelled. $kind is the wire text of what ran: backup, forget, prune, check
# or adopt; any other kind is an operation.
ui-operation-cancelled =
    { $kind ->
        [backup] Backup cancelled.
        [forget] Forget cancelled.
        [prune] Prune cancelled.
        [check] Check cancelled.
        [adopt] Adoption cancelled.
       *[other] Operation cancelled.
    }

# Announced assertively on the Operations page when a run it watched ended
# badly. $kind is the wire text of what ran: backup, forget, prune, check or
# adopt; any other kind is an operation.
ui-operation-failed =
    { $kind ->
        [backup] Backup failed.
        [forget] Forget failed.
        [prune] Prune failed.
        [check] Check failed.
        [adopt] Adoption failed.
       *[other] Operation failed.
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

# An instant, as the page's locale writes it.
ui-instant = { DATETIME($when, dateStyle: "short", timeStyle: "medium") }

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

# What the maintenance section says instead, with no buttons, of a repository this agent does not maintain: its entry in the configuration file asks for maintenance by the hub or by nothing. What the person types is a string literal: keep it one, untranslated.
ui-maintenance-elsewhere = This agent does not maintain this repository, so it cannot forget, prune or check it, or adopt history into its plans. To maintain it from this agent, set { "maintenance = \"local\"" } in the repository's entry in the configuration file.

# The button that reviews a forget: the snapshots the plans' retention no longer keeps.
ui-maintenance-forget = Forget snapshots

# The button that reviews a prune: the data no snapshot uses.
ui-maintenance-prune = Prune unused data

# The button that reviews a check of the repository.
ui-maintenance-check = Check the repository

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

# Said assertively, and shown, when a check ran to its end and found errors, which it lists below this line with what else it found. An error is damage in the repository, or a part of the repository, or of this agent's copy of it, that could not be read or did not match. $errors (count): how many errors it found.
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

# The heading of the section of history that no plan of this agent owns.
ui-orphaned-heading = Orphaned history

# What the orphaned section says about its groups.
ui-orphaned-body = No plan of this agent owns these snapshots now. A forget leaves them alone, and adopting a group rewrites it into a plan.

# The heading of an orphaned group. $owner is both when the group carries the tags of an agent and a plan, plan or agent when it carries one of them, and neither when it carries none; $plan and $agent are the tags.
ui-orphaned-group =
    { $owner ->
        [both] Plan { $plan } of agent { $agent }
        [plan] Plan { $plan } of an unknown agent
        [agent] An unknown plan of agent { $agent }
       *[neither] History of an unknown agent and plan
    }

# The caption of the table of an orphaned group's snapshots; $owner, $plan and $agent are as in the group's heading.
ui-orphaned-caption =
    { $owner ->
        [both] Snapshots of plan { $plan } of agent { $agent }
        [plan] Snapshots of plan { $plan } of an unknown agent
        [agent] Snapshots of an unknown plan of agent { $agent }
       *[neither] Snapshots of an unknown agent and plan
    }

# The button that adopts an orphaned group, named by its agent and plan, into a plan of this agent.
ui-adopt-orphaned = Adopt history of plan { $plan } from agent { $agent } into plan { $into }

# What an orphaned group says when it lacks the tag of an agent or of a plan, which an adoption names.
ui-orphaned-untagged = This history lacks the tag of an agent or a plan, so it cannot be adopted.

# What an orphaned group says when no plan of this agent could take it.
ui-orphaned-no-plan = No plan of this agent could take this history, so it cannot be adopted.

# The heading of the section of history that no agent tagged.
ui-unmanaged-heading = Unmanaged history

# What the unmanaged section says about its groups.
ui-unmanaged-body = No agent tagged these snapshots. A forget leaves them alone, and a plan that backs up the same host, label and paths can adopt them.

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

# What an unmanaged group says when no plan of this agent could adopt it.
ui-unmanaged-no-plan = No plan of this agent backs up this host, label and paths, so this history cannot be adopted.

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

# What a forget says when no plan of this agent backs up to the repository.
ui-review-no-plans = No plan of this agent backs up to this repository, so a forget has nothing to keep or remove.

# What a forget leaves alone when it finds only orphaned history; $snapshots counts the snapshots.
ui-review-leaves-orphaned = Left alone: orphaned snapshots ({ $snapshots }), which no plan of this agent owns.

# What a forget leaves alone when it finds only unmanaged history; $snapshots counts the snapshots.
ui-review-leaves-unmanaged = Left alone: unmanaged snapshots ({ $snapshots }), which no agent tagged.

# What a forget leaves alone when it finds both kinds of history, counted by $orphaned and $unmanaged.
ui-review-leaves-both = Left alone: orphaned snapshots ({ $orphaned }), which no plan of this agent owns, and unmanaged snapshots ({ $unmanaged }), which no agent tagged.

# What an adoption does; $snapshots is how many snapshots become part of the plan $into.
ui-review-adopts =
    { $snapshots ->
        [one] 1 snapshot becomes part of plan { $into }. It gets a new ID and keeps its tree.
       *[other] { $snapshots } snapshots become part of plan { $into }. They get new IDs and keep their trees.
    }

# What an adoption says when no snapshot is left to adopt into the plan $into.
ui-review-adopts-none = No snapshots are left to adopt into plan { $into }.

# What a prune's dry run found: packs marked as unused now, packs deleted now, packs recovered, and the bytes the deleted ones free.
ui-review-prunes = Packs marked as unused now: { $marked }. Packs deleted now, which were marked earlier: { $deleted }, freeing { $bytes }. Packs recovered: { $recovered }.

# How long a pack that is marked now waits for a later prune to delete it, in whole days.
ui-review-prune-wait-days =
    { $days ->
        [one] Packs marked now are deleted by a later prune, after 1 day.
       *[other] Packs marked now are deleted by a later prune, after { $days } days.
    }

# How long a pack that is marked now waits for a later prune to delete it, in hours.
ui-review-prune-wait-hours =
    { $hours ->
        [one] Packs marked now are deleted by a later prune, after 1 hour.
       *[other] Packs marked now are deleted by a later prune, after { $hours } hours.
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

# The hint under the password field. It says where the typed password goes: the configuration file, and the history of it that { -brand-name } keeps.
ui-repositories-password-hint = { -brand-name } keeps it in its configuration file, and in the copies it keeps of that file's earlier versions. It never shows it again.

# The label of the checkbox that asks this agent to maintain a repository it connects: run its forget, prune and check. Off unless the person turns it on.
ui-repositories-maintain-label = Maintain this repository from this agent

# The hint under that checkbox. It says what maintaining means, that a repository created here is always maintained from here, and why to leave it off: only one place may maintain a repository, and another agent or the hub may already do it.
ui-repositories-maintain-hint = When this is on, this agent forgets, prunes and checks the repository you connect, and adopts history into its plans. Only one place may maintain a repository, so leave this off when another agent or the hub already does. A repository you create here under a name is always maintained from here.

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
