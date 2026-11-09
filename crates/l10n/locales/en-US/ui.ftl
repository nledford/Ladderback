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
ui-start-body = Nothing is set up yet. Repositories, plans and backups arrive in later releases.

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
