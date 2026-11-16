@ui
Feature: The shared components in the gallery
  The shared component set (technical design §4.5) is exercised in the
  component gallery, a development page at /_gallery, with keyboard-only
  journeys: a dialog, a table, sorted by keyboard, which keeps a focused
  control in a row through the row's changes and scrolls sideways in a
  frame of its own on a small screen, a checkbox, a
  radio group, a progress bar a keyboard user can reach, whose changes
  are never announced and are told at most every 30 seconds, and which shows
  no percentage while its size isn't known, a toast, the
  control that pauses live updates, a status badge for every state an
  operation can be in, whose icon tells the state by its shape, a
  snapshot's tree, one stop for Tab, whose folders are read as they are
  opened, the step-up field, which takes the admin password from a
  password manager or a paste, or shows a short ID to copy and paste back,
  and the shared vocabulary every component takes: its tones, each
  named in words and drawn in a color of its own, its sizes and its
  placements, which follow the page's direction, a spinner, which a
  label names, which never speaks for itself, which takes its tone, size
  and placement from that vocabulary and which stands still for a reader
  who asks for less motion, an icon, decoration a screen reader skips
  unless a label names it, as tall as the text and in the text's color
  unless its page names a size or a tone, sharing the spinner's sizes, and
  a button, pressed by Enter and Space, drawn in an appearance and a tone
  named in words, as tall as a multiple of the text, which stays in the Tab
  order and ignores presses while it is unavailable or busy, keeps its name
  while a spinner stands in it, and says it is pressed by its state and by
  its weight and edge, a text field, its label, description and error
  above it and wired to it, its error announced and drawn in a thicker
  edge in the Error tone, which takes one line, many lines, a password it
  can show, a number or a code, says in words that it is required, read
  only or unavailable, and is as tall as a button of its size, and
  a tooltip, a short note beside a control that keyboard focus shows at
  once and a resting pointer after a moment, which Escape dismisses, which
  stays while the pointer is over it, which stands on the side its control
  asks for where there is room and never covers the control, and which
  describes the control, or shows an icon-only control's name.
  Every demo state a component can show (loading, empty, error, success,
  on and off) stays clear
  of accessibility violations, and each component's structure is pinned by an
  ARIA snapshot. The text of a control, such as a button's label, grows with
  the reader's text size as the page's does, and the gallery still fits a
  small screen when the text has grown (A4).

  Scenario: A dialog opens from a button, traps focus, closes on Escape and returns focus
    When I open the component gallery
    And I press Tab until focus is on the button "Open the demo dialog"
    And I press Enter
    Then focus is inside the dialog "Demo dialog"
    And I press Tab until focus is on the button "Close the demo dialog"
    And I press Tab
    Then focus is on the first control of the dialog "Demo dialog"
    When I press Escape
    Then focus is on the button "Open the demo dialog"

  Scenario: The step-up field takes a password from a password manager or paste
    When I open the component gallery
    And I press Tab until focus is on the button "Open the demo step-up"
    And I press Enter
    Then focus is inside the dialog "Demo step-up"
    And the textbox "Admin password" has the autocomplete token "current-password"
    When I press Tab until focus is on the textbox "Admin password"
    And I paste "canary-demo-password"
    Then the field "Admin password" holds what was pasted
    And the page has no accessibility violations

  Scenario: A password shown in the step-up field is hidden again when its dialog is opened again
    When I open the component gallery
    And I press Tab until focus is on the button "Open the demo step-up"
    And I press Enter
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-secret"
    And I press Tab
    Then focus is on the button "Show password" described by "Admin password"
    When I press Space
    Then the field "Admin password" shows what it holds, "demo-secret", its caret 0 characters before its end
    When I press Escape
    Then focus is on the button "Open the demo step-up"
    When I press Enter
    Then focus is inside the dialog "Demo step-up"
    And the field "Admin password" hides what it holds
    And the textbox "Admin password" still holds ""
    And the button "Show password" described by "Admin password" is not pressed

  Scenario: The step-up field shows a short ID to copy and takes it back by paste
    Given the page may use the clipboard
    When I open the component gallery
    And I press Tab until focus is on the button "Open the demo short-ID step-up"
    And I press Enter
    Then focus is inside the dialog "Demo short-ID step-up"
    And the dialog "Demo short-ID step-up" shows "1a2b3c4d" as text that can be copied
    When I press Tab until focus is on the button "Copy the ID"
    And I press Enter
    Then the clipboard holds "1a2b3c4d"
    And the dialog "Demo short-ID step-up" says "Copied."
    When I press Tab until focus is on the textbox "Demo ID"
    And I paste "1a2b3c4d"
    Then the field "Demo ID" holds what was pasted
    And the page has no accessibility violations

  Scenario: Until its first sort a table shows its rows in the order given, and no column says it is sorted
    When I open the component gallery
    Then the rows of the table "Demo folders" are "photos, music, documents, notes"
    And no column header of the table "Demo folders" says it is sorted

  Scenario: A table's column is sorted by Enter, then the other way by Space, and each sort is said
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by size"
    And I press Enter
    Then the rows of the table "Demo folders" are "notes, documents, photos, music"
    And the column header "Size" alone of the table "Demo folders" says it is sorted "ascending"
    And the polite region reads "Sorted by size, smallest first."
    When I press Space
    Then the rows of the table "Demo folders" are "music, photos, documents, notes"
    And the column header "Size" alone of the table "Demo folders" says it is sorted "descending"
    And the polite region reads "Sorted by size, largest first."

  Scenario: Rows that tie keep their order, whichever way the table is sorted
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by files"
    And I press Enter
    Then the rows of the table "Demo folders" are "music, notes, documents, photos"
    When I press Enter
    Then the rows of the table "Demo folders" are "photos, documents, music, notes"

  Scenario: A table's sort button sits in its column header apart from the header's text
    When I open the component gallery
    Then the button "Sort by changed" sits inside the column header "Changed", apart from its text

  Scenario: A row's action keeps its focus while its row changes and moves
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by files"
    And I press Enter
    And I press Tab until focus is on the button "Add a file to music"
    And I press Enter
    Then the row "music" of the table "Demo folders" reads "13" in the column "Files"
    And the row "Total" of the table "Demo folders" reads "1,541" in the column "Files"
    And the rows of the table "Demo folders" are "notes, music, documents, photos"
    And focus is kept on the button "Add a file to music"

  Scenario: A table's numbers stand at the end of their column in figures of one width
    When I open the component gallery
    Then the column "Size" of the table "Demo folders" is aligned to its end in figures of one width
    And the column "Files" of the table "Demo folders" is aligned to its end in figures of one width
    And the column "Changed" of the table "Demo folders" is aligned to its start

  Scenario Outline: A table that can't show its rows says why in their place
    When I open the component gallery
    And I press Shift+Tab until focus is on the radio "Demo rows"
    And I press the down arrow until focus is on the radio "<choice>"
    Then the "Table" demo says "<sentence>" in the place of the table "Demo folders"

    Examples:
      | choice           | sentence                                              |
      | Demo empty       | No demo folders yet.                                  |
      | Demo unreachable | Ladderback can't be reached, so no demo folders show. |

  Scenario: A table whose rows are on their way says so beside a spinner
    When I open the component gallery
    And I press Shift+Tab until focus is on the radio "Demo rows"
    And I press the down arrow until focus is on the radio "Demo loading"
    Then the "Table" demo says "Loading the demo folders…" in the place of the table "Demo folders"
    And the "Table" demo shows the text "Loading the demo folders…" beside a spinner a screen reader skips

  Scenario Outline: On a small screen with larger text, a table's frame scrolls by keyboard and each row's header stays in view, <direction>
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the region "<table>"
    And I press <key> until the region "<table>" has scrolled to its end
    Then each row header of the table "<table>" stands whole at the start of its frame
    And the page doesn't scroll sideways

    Examples:
      | direction     | table                       | key        |
      | left to right | Demo folders                | ArrowRight |
      | right to left | Demo folders, right to left | ArrowLeft  |

  Scenario Outline: A table's row headers are filled with the page's background, so the cells scrolled under them don't show through, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then each row header of the table "Demo folders" is filled with the page's background color

    Examples:
      | colors        |
      | light colors  |
      | dark colors   |
      | forced colors |

  Scenario: A checkbox turns on and off by keyboard and is described by its hint
    When I open the component gallery
    And I press Tab until focus is on the checkbox "Send a demo notification"
    Then the checkbox "Send a demo notification" is not checked
    And the checkbox "Send a demo notification" is described by "A demo choice: turning it on changes nothing."
    When I press Space
    Then the checkbox "Send a demo notification" is checked
    When I press Space
    Then the checkbox "Send a demo notification" is not checked

  Scenario: A radio group is one stop for Tab, which lands on the choice made, and the arrow keys choose within it
    When I open the component gallery
    And I press Tab until focus is on the radio "Every week"
    Then the radio "Every week" of the group "Demo check frequency" is checked
    And the group "Demo check frequency" is described by "A demo choice: choosing one changes nothing."
    When I press the down arrow until focus is on the radio "Every month"
    Then the radio "Every month" of the group "Demo check frequency" is checked
    And the radio "Every week" of the group "Demo check frequency" is not checked
    When I press Tab
    Then focus has left the group "Demo check frequency"
    When I press Shift+Tab
    Then focus is on the radio "Every month"

  Scenario: The progress bar is reached by keyboard and its value changes without any announcement
    Given the page's clock can be moved on
    When I open the component gallery
    And I press Tab until focus is on the progressbar "Demo backup progress"
    Then the progress bar "Demo backup progress" reads "Backing up… 40%, 4 GiB of 10 GiB"
    When I press Tab until focus is on the button "Advance the demo progress"
    And I press Enter
    Then the progress bar "Demo backup progress" is drawn at 80 percent
    And the progress bar "Demo backup progress" shows 40 percent
    And the live regions stay silent
    When the page's clock moves on 30 seconds
    Then the progress bar "Demo backup progress" shows 80 percent
    And the progress bar "Demo backup progress" reads "Backing up… 80%, 8 GiB of 10 GiB"
    And the live regions stay silent

  Scenario: A progress bar whose size isn't known yet shows no percentage
    When I open the component gallery
    Then the progress bar "Demo progress of unknown size" shows no percentage
    And the progress bar "Demo progress of unknown size" reads "Working…"
    And the progress bar "Demo progress of a step" reads "Reading the index…"
    And the "Progress" demo has no accessibility violations

  Scenario: A toast appears and is announced politely, and its going is not announced
    When I open the component gallery
    And I press Tab until focus is on the button "Save the demo settings"
    And I press Enter
    Then a toast reads "Settings saved."
    And the polite region reads "Settings saved."
    When the toast "Settings saved." is gone
    Then the polite region reads "Settings saved."

  Scenario: The pause control holds the demo's updates and says once what changed when it resumes
    When I open the component gallery
    And I press Tab until focus is on the button "Pause the demo updates"
    Then the button "Pause the demo updates" is not pressed
    And the button "Pause the demo updates" has no description
    When I press Enter
    Then the button "Pause the demo updates" is pressed
    And focus stays on the button "Pause the demo updates"
    And the button "Pause the demo updates" is described by "Live updates are paused. Nothing has changed since."
    And the polite region reads "Live updates are paused."
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Finish a demo run"
    And I press Enter
    And I press Enter
    Then the button "Pause the demo updates" is described by "Live updates are paused. 2 changes are waiting."
    And the polite region keeps reading "Live updates are paused."
    When I press Shift+Tab until focus is on the button "Pause the demo updates"
    And I press Enter
    Then the button "Pause the demo updates" is not pressed
    And focus stays on the button "Pause the demo updates"
    And the button "Pause the demo updates" has no description
    And the polite region reads "Live updates are on again. 2 runs ended while they were paused, and none of them failed."

  Scenario: A message nobody asked for waits while updates are paused and is said with the resume, once
    When I open the component gallery
    And I press Tab until focus is on the button "Pause the demo updates"
    And I press Enter
    And the polite region reads "Live updates are paused."
    And I press Shift+Tab until focus is on the button "Say politely: backup finished"
    And I press Enter
    And I press Enter
    Then the polite region keeps reading "Live updates are paused."
    When I press Tab until focus is on the button "Pause the demo updates"
    And I press Enter
    Then the polite region reads "Live updates are on again. No run ended while they were paused. Backup finished."

  Scenario: A finished run is said at once while updates are not paused
    When I open the component gallery
    And I press Tab until focus is on the button "Finish a demo run"
    And I press Enter
    Then the polite region reads "Backup finished."
    And the button "Pause the demo updates" has no description

  Scenario: The tree is one stop for Tab, and the arrow keys, Home and End move within it
    When I open the component gallery
    And I press Tab until focus is on the treeitem "documents"
    Then the tree "Demo snapshot" has one tab stop
    And the tree item "documents" is item 1 of 10 at level 1
    When I press ArrowDown
    Then focus is on the tree item "empty"
    When I press End
    Then focus is on the tree item "readme.txt"
    And the tree item "readme.txt" is described by "File, 1.2 KiB"
    When I press Home
    Then focus is on the tree item "documents"
    When I press ArrowDown
    And I press Tab
    Then focus has left the tree "Demo snapshot"
    When I press Shift+Tab
    Then focus is on the tree item "empty"

  Scenario: Right opens a folder, reading it as it opens, and Left closes it
    When I open the component gallery
    And I press Tab until focus is on the treeitem "documents"
    Then the tree item "documents" is collapsed
    When I press ArrowRight
    Then the tree item "documents" is expanded
    And the tree item "notes.txt" is item 1 of 2 at level 2
    And focus is on the tree item "documents"
    When I press ArrowRight
    Then focus is on the tree item "notes.txt"
    When I press ArrowLeft
    Then focus is on the tree item "documents"
    When I press ArrowLeft
    Then the tree item "documents" is collapsed
    And the tree shows no tree item "notes.txt"
    When I press Enter
    Then the tree item "documents" is expanded

  Scenario: A folder holding nothing says it is empty
    When I open the component gallery
    And I press Tab until focus is on the treeitem "documents"
    And I press ArrowDown
    And I press ArrowRight
    Then the tree item "empty" is expanded
    And the tree item "empty" is described by "Folder, empty"

  Scenario: A folder read in pages ends in a row that reads the next page, and focus moves to what it read
    When I open the component gallery
    And I press Tab until focus is on the treeitem "documents"
    And I type "m" in the tree
    And I press ArrowRight
    Then the tree item "alpha.txt" is item 1 of 5 at level 2
    When I press ArrowDown until focus is on the tree item "Show more: 2 of 5 shown"
    And I press Enter
    Then focus is on the tree item "charlie.txt"
    And the tree item "charlie.txt" is item 3 of 5 at level 2
    And the tree has the tree item "Show more: 4 of 5 shown"

  Scenario: A folder still being read is marked busy
    When I open the component gallery
    And I press Tab until focus is on the treeitem "documents"
    And I type "s" in the tree
    And I press ArrowRight
    Then the tree item "slow" is busy
    And the tree item "slow" is described by "Folder, loading"

  Scenario: A folder that can't be read says why and stays closed
    When I open the component gallery
    And I press Tab until focus is on the treeitem "documents"
    And I type "u" in the tree
    And I press ArrowRight
    Then the page alerts "The demo folder could not be read."
    And the tree item "unreadable" is collapsed
    And focus is on the tree item "unreadable"
    And the "Tree" demo has no accessibility violations

  Scenario: Typing the first letters of a name moves focus to it
    When I open the component gallery
    And I press Tab until focus is on the treeitem "documents"
    And I type "re" in the tree
    Then focus is on the tree item "readme.txt"
    When I type "l" in the tree
    Then focus is on the tree item "latest"
    And the tree item "latest" is described by "Link"

  Scenario: A row is named by the name it shows, not by the key that opens it
    When I open the component gallery
    And I press Tab until focus is on the treeitem "documents"
    Then the tree has the tree item "abc[U+202E]txt.exe"

  Scenario: A name written right to left keeps its direction in a left-to-right page
    When I open the component gallery
    And I press Tab until focus is on the treeitem "documents"
    Then the tree item "הערות.txt" shows its name right to left

  Scenario: In a page that runs right to left, Left opens a folder and Right closes it
    When I open the component gallery asking for the locale "ar-XB"
    And I press Tab until focus is on a tree item
    And I press ArrowLeft
    Then the focused tree item is expanded
    When I press ArrowRight
    Then the focused tree item is collapsed

  Scenario Outline: Each component's demo states have no accessibility violations
    When I open the component gallery
    Then the "<component>" demo has no accessibility violations


    Examples:
      | component   |
      | Dialog      |
      | Text field  |
      | Checkbox    |
      | Radio group |
      | Status badge |
      | Progress    |
      | Toast       |
      | Review dialog |
      | Pause control |
      | Tree        |
      | Shared vocabulary |
      | Spinner     |
      | Icon        |
      | Button      |
      | Table       |
      | Tooltip     |
      | Step-up field |

  Scenario Outline: Each component's structure is pinned by an ARIA snapshot
    When I open the component gallery
    Then the "<component>" demo matches its ARIA snapshot

    Examples:
      | component   |
      | Dialog      |
      | Text field  |
      | Checkbox    |
      | Radio group |
      | Status badge |
      | Progress    |
      | Toast       |
      | Review dialog |
      | Pause control |
      | Tree        |
      | Shared vocabulary |
      | Spinner     |
      | Icon        |
      | Button      |
      | Table       |
      | Tooltip     |
      | Step-up field |

  Scenario: A review dialog opens on its dismiss button, keeps Tab inside and returns focus on Escape
    When I open the component gallery
    And I press Tab until focus is on the button "Open a demo review"
    And I press Enter
    Then focus is inside the dialog "Demo review to confirm"
    And focus is on the button "Cancel"
    When I press Tab until focus is on the button "Confirm the demo review now"
    And I press Tab
    Then focus is on the first control of the dialog "Demo review to confirm"
    When I press Escape
    Then focus is on the button "Open a demo review"

  Scenario: A review dialog's second way to go ahead comes after its confirm button
    When I open the component gallery
    And I press Tab until focus is on the button "Open a demo review"
    And I press Enter
    Then focus is on the button "Cancel"
    When I press Tab
    Then focus is on the button "Confirm the demo review"
    When I press Tab
    Then focus is on the button "Confirm the demo review now"
    When I press Tab
    Then focus is on the first control of the dialog "Demo review to confirm"
    And focus is on the button "Cancel"

  Scenario: A confirmed review dialog closes, returns focus and announces after it closed
    When I open the component gallery
    And I press Tab until focus is on the button "Open a demo review"
    And I press Enter
    And I press Tab until focus is on the button "Confirm the demo review"
    And I press Enter
    Then focus is on the button "Open a demo review"
    And the polite region reads "Demo review confirmed."

  Scenario: A review dialog with nothing to do offers only to close
    When I open the component gallery
    And I press Tab until focus is on the button "Open an empty demo review"
    And I press Enter
    Then focus is inside the dialog "Demo review with nothing to do"
    And focus is on the button "Close"
    And the dialog "Demo review with nothing to do" offers only the button "Close"
    When I press Enter
    Then focus is on the button "Open an empty demo review"

  Scenario: A review dialog can open on a button of its own, and still closes on Escape and returns focus
    When I open the component gallery
    And I press Tab until focus is on the button "Open a demo secret"
    And I press Enter
    Then focus is inside the dialog "Demo secret shown once"
    And focus is on the button "Copy the demo secret"
    When I press Tab until focus is on the button "Close"
    And I press Tab
    Then focus is on the first control of the dialog "Demo secret shown once"
    When I press Escape
    Then focus is on the button "Open a demo secret"

  Scenario: A held review dialog says why beside its confirm button, and pressing it only says so
    When I open the component gallery
    And I press Tab until focus is on the button "Open a held demo review"
    And I press Enter
    Then focus is on the button "Cancel"
    When I press Tab until focus is on the button "Confirm the held demo review"
    Then the dialog "Demo review on hold" has its button "Confirm the held demo review" unavailable
    And the button "Confirm the held demo review" is described by "The demo review waits for something that never comes."
    When I press Enter
    Then the dialog "Demo review on hold" politely says "The demo review waits for something that never comes."
    And the dialog "Demo review on hold" stays open
    And focus is on the button "Confirm the held demo review"
    When I press Escape
    Then focus is on the button "Open a held demo review"

  Scenario Outline: An open review dialog has no accessibility violations
    When I open the component gallery
    And I press Tab until focus is on the button "<opener>"
    And I press Enter
    Then focus is inside the dialog "<dialog>"
    And the page has no accessibility violations

    Examples:
      | opener                    | dialog                         |
      | Open a demo review        | Demo review to confirm         |
      | Open an empty demo review | Demo review with nothing to do |
      | Open a demo secret        | Demo secret shown once         |
      | Open a held demo review   | Demo review on hold            |

  Scenario Outline: A button's label grows with the reader's text size
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "<button>"
    Then the label of the button "<button>" is as large as the page's text
    And the page has no accessibility violations

    Examples:
      | button                    |
      | Open the demo dialog      |
      | Advance the demo progress |
      | Pause the demo updates    |
      | Filled Primary            |

  # The table's sort button sits past the 40 Tab presses a journey allows
  # from the page's start, but within them by Shift+Tab.
  Scenario: A table's sort button's label grows with the reader's text size
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by name"
    Then the label of the button "Sort by name" is as large as the page's text
    And the page has no accessibility violations

  Scenario: The gallery fits a small screen with larger text, its buttons' labels grown with it
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then the label of the button "Open the demo dialog" is as large as the page's text
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: Each state of a status badge has an icon of its own beside its text, which a screen reader skips
    When I open the component gallery
    Then each of the 7 badges of the "Status badge" demo has an icon of its own, in its text's color, which a screen reader skips

  Scenario: No status badge moves, the running one's icon either, however long the operation runs
    When I open the component gallery
    Then none of the 7 badges of the "Status badge" demo moves

  Scenario: A status badge's icon grows with the reader's text size
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the icon of each badge of the "Status badge" demo is as tall as its text

  Scenario: Each tone is named in words, and draws its text and its swatch in a color of its own, which a screen reader skips
    When I open the component gallery
    Then each of the 6 tones of the "Shared vocabulary" demo is named in words, its swatch drawn in its text's color and skipped by a screen reader
    And the 5 tones of the "Shared vocabulary" demo other than Inherit each have a color of their own
    And the tone "Inherit" of the "Shared vocabulary" demo takes the color of the text around it

  Scenario: Under forced colors every tone draws its text and its swatch in the system's text color, so its words tell it apart
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then each of the 6 tones of the "Shared vocabulary" demo is named in words, its swatch drawn in its text's color and skipped by a screen reader
    And the 6 tones of the "Shared vocabulary" demo all draw in the color of the text around them

  Scenario Outline: Start, Center and End place a part at its line's start, middle and end, in the page's direction
    When I open the component gallery asking for the locale "<locale>"
    Then the placements of the vocabulary demo put each part where they say, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: A spinner with a visible label is read as its label, and a screen reader skips its drawing
    When I open the component gallery
    Then the "Spinner" demo shows the text "Loading the demo snapshots…" beside a spinner a screen reader skips
    And the button "Save the demo notes" holds a spinner a screen reader skips

  Scenario: A spinner in a line of text stands on the line's baseline, its drawing centered on the text
    When I open the component gallery
    Then the spinner labelled "Loading the demo snapshots…" stands in its line of text, its label on the line's baseline and its drawing centered on the text

  Scenario: A spinner whose label is hidden is an image named by its label
    When I open the component gallery
    Then the "Spinner" demo has the image "Demo spinner at the Medium size"
    And the "Spinner" demo shows no text "Demo spinner at the Medium size"

  Scenario: A spinner is never a live region of its own, which the page's announcer is
    When I open the component gallery
    Then the "Spinner" demo has spinners but no live region

  Scenario: A spinner turns steadily about once a second, and stands still when the reader asks for less motion
    When I open the component gallery
    Then every spinner of the "Spinner" demo turns steadily, once in 0.8 to 1 second
    When the reader asks for less motion
    Then no spinner of the "Spinner" demo moves

  Scenario: A spinner turns its drawing inside the drawing's box, which keeps its length as it turns
    When I open the component gallery
    Then no spinner of the "Spinner" demo reaches out of its drawing's box as it turns

  Scenario Outline: A spinner's size is a multiple of the reader's text size, Small unless its page names another
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the image "<spinner>" is <times> times as tall as the page's text

    Examples:
      | spinner                              | times |
      | Demo spinner at the Small size       | 1     |
      | Demo spinner at the Medium size      | 1.5   |
      | Demo spinner at the Large size       | 2     |
      | Demo spinner at the Extra large size | 3     |

  Scenario: A spinner's tone colors its drawing only, Primary unless its page names another, and its label stays in the text's color
    When I open the component gallery
    Then each spinner of the "Spinner" demo draws its arc in its tone and its track in a faint version of it:
      | spinner                            | tone               |
      | Demo spinner in the Primary tone   | --accent           |
      | Demo spinner in the Secondary tone | --muted            |
      | Demo spinner in the Success tone   | --success          |
      | Demo spinner in the Warning tone   | --warning          |
      | Demo spinner in the Error tone     | --error            |
      | Demo spinner in the Inherit tone   | the text around it |
    And the label of every spinner of the "Spinner" demo is in the color of the text around it

  Scenario: A spinner placed Start, Center or End sits on a line of its own, at the line's start, middle or end
    When I open the component gallery
    Then the spinners on lines of their own read "Loading at the start of the line…", "Loading in the middle of the line…" and "Loading at the end of the line…", in that order
    And the spinners on lines of their own sit at the start, the middle and the end of lines that run left to right

  Scenario: In a page that runs right to left, a spinner's line starts at the right
    When I open the component gallery asking for the locale "ar-XB"
    Then the spinners on lines of their own sit at the start, the middle and the end of lines that run right to left

  Scenario Outline: On a small screen with larger text, a long label on a line of its own wraps beside its spinner, not under it
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery asking for the locale "<locale>"
    Then the label of each spinner on a line of its own wraps onto more lines beside its drawing, which stands centered on the first, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: Under forced colors, a spinner's arc takes the text's color, as every tone does, and its track the color of unavailable text
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every spinner of the "Spinner" demo draws its arc in the color of the text around it and its track in GrayText

  Scenario: An icon beside words is decoration a screen reader skips, in the words' color, as tall as them and centered on them
    When I open the component gallery
    Then the "Icon" demo shows the text "The next demo backup starts at 02:00 every night." beside an icon a screen reader skips, in the text's color, as tall as it and centered on it

  Scenario: An icon-only button is named by its own label, and the icon inside it is skipped by a screen reader
    When I open the component gallery
    And I press Tab until focus is on the button "Demo notifications"
    Then the button "Demo notifications" shows only an icon, which a screen reader skips
    And the button "Demo notifications" is not pressed
    And the button "Demo notifications" shows a crossed-out bell
    When I press Space
    Then the button "Demo notifications" is pressed
    And the button "Demo notifications" shows only an icon, which a screen reader skips
    And the button "Demo notifications" shows a bell, not crossed out
    When I press Space
    Then the button "Demo notifications" is not pressed
    And the button "Demo notifications" shows a crossed-out bell

  Scenario: An icon given a label is an image named by it
    When I open the component gallery
    Then the "Icon" demo has the image "Demo icon at the Medium size"
    And the "Icon" demo shows no text "Demo icon at the Medium size"

  Scenario Outline: An icon's size is a multiple of the reader's text size, Small unless its page names another, as a spinner's of the same size is
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the image "<icon>" is <times> times as tall as the page's text
    And the image "<icon>" is as tall as the image "<spinner>"

    Examples:
      | icon                              | spinner                              | times |
      | Demo icon at the Small size       | Demo spinner at the Small size       | 1     |
      | Demo icon at the Medium size      | Demo spinner at the Medium size      | 1.5   |
      | Demo icon at the Large size       | Demo spinner at the Large size       | 2     |
      | Demo icon at the Extra large size | Demo spinner at the Extra large size | 3     |

  Scenario: An icon's tone colors its glyph, named in words beside it, and an icon in no tone takes the color of the text around it
    When I open the component gallery
    Then each tone of the "Icon" demo, named in words, draws its icon in it:
      | tone      | color              |
      | Primary   | --accent           |
      | Secondary | --muted            |
      | Success   | --success          |
      | Warning   | --warning          |
      | Error     | --error            |
      | Inherit   | the text around it |

  Scenario: Under forced colors every icon is drawn in the color of the text around it, as every tone is
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every icon of the "Icon" demo is drawn in the color of the text around it

  Scenario: A button is pressed by Enter and by Space
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Enter
    Then the "Button" demo shows the sentence "The demo buttons were pressed once."
    When I press Space
    Then the "Button" demo shows the sentence "The demo buttons were pressed 2 times."

  Scenario: A button draws its appearance in its tone, both named in words, Outlined in Secondary unless its page names others
    When I open the component gallery
    Then each button of the "Button" demo draws its appearance in its tone:
      | button                         | label              | fill     | edge               |
      | Filled Primary                 | --on-fill          | --accent | --accent           |
      | Filled Secondary               | --on-fill          | --muted  | --muted            |
      | Filled Success                 | --on-fill          | --success | --success         |
      | Filled Warning                 | --on-fill          | --warning | --warning         |
      | Filled Error                   | --on-fill          | --error  | --error            |
      | Outlined Primary               | --accent           | none     | --accent           |
      | Outlined Secondary             | --muted            | none     | --muted            |
      | Outlined Success               | --success          | none     | --success          |
      | Outlined Warning               | --warning          | none     | --warning          |
      | Outlined Error                 | --error            | none     | --error            |
      | Outlined Inherit               | the text around it | none     | the text around it |
      | Ghost Primary                  | --accent           | none     | none               |
      | Ghost Secondary                | --muted            | none     | none               |
      | Ghost Success                  | --success          | none     | none               |
      | Ghost Warning                  | --warning          | none     | none               |
      | Ghost Error                    | --error            | none     | none               |
      | Ghost Inherit                  | the text around it | none     | none               |
      | Demo button at the Medium size | --muted            | none     | --muted            |

  Scenario Outline: A button is a multiple of the reader's text size tall, Medium unless its page names another
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the button "<button>" is <times> times as tall as the page's text

    Examples:
      | button                              | times |
      | Demo button at the Small size       | 1.75  |
      | Demo button at the Medium size      | 2     |
      | Demo button at the Large size       | 2.5   |
      | Demo button at the Extra large size | 3     |
      | Demo details at the Small size      | 1.75  |
      | Demo details at the Extra large size | 3    |

  Scenario Outline: An icon-only button is named by its label, and stays square and at least 24 pixels a side at every size
    When I open the component gallery
    Then the button "<button>" shows only an icon, which a screen reader skips
    And the button "<button>" is square, at least 24 pixels a side

    Examples:
      | button                               |
      | Demo details at the Small size       |
      | Demo details at the Medium size      |
      | Demo details at the Large size       |
      | Demo details at the Extra large size |

  Scenario Outline: A button's icon stands before its label, as tall as its text, in the page's direction
    When I open the component gallery asking for the locale "<locale>"
    Then every button with an icon and words shows its icon before its words, as tall as them, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: An unavailable button stays in the Tab order, says it is unavailable and ignores Enter and Space
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Delete the demo snapshot"
    Then the button "Delete the demo snapshot" says it is unavailable, and is not disabled
    When I press Enter
    And I press Space
    Then focus stays on the button "Delete the demo snapshot"
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."

  Scenario: A busy button keeps its name and its place in the Tab order, shows a spinner a screen reader skips, and ignores presses
    When I open the component gallery
    And the widths of the buttons "Check the demo repository" and "Refresh the demo" are noted
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then the button "Demo work in progress" is pressed
    And the button "Save the demo plan" is busy, a spinner a screen reader skips before its label
    And the button "Check the demo repository" is busy, a spinner a screen reader skips in its icon's place
    And the button "Refresh the demo" is busy, a spinner a screen reader skips in its icon's place
    And the buttons "Check the demo repository" and "Refresh the demo" keep the widths noted
    And the "Button" demo has no accessibility violations
    When I press Tab until focus is on the button "Check the demo repository"
    And I press Enter
    And I press Space
    Then focus stays on the button "Check the demo repository"
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."
    When I press Shift+Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then the button "Check the demo repository" is not busy, its icon back in its place

  Scenario: A toggle button keeps its one name and says it is pressed, drawn bolder with an inset edge
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Show the demo's hidden files"
    Then the button "Show the demo's hidden files" is not pressed
    And the button "Show the demo's hidden files" is drawn at its resting weight and edge
    When I press Space
    Then the button "Show the demo's hidden files" is pressed
    And the button "Show the demo's hidden files" is drawn bolder, with an inset edge
    And the "Button" demo has no accessibility violations
    When I press Enter
    Then the button "Show the demo's hidden files" is not pressed
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."

  Scenario: A submit button sends its form from the keyboard
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Enter
    Then the polite region comes to contain "Demo form sent."
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."

  Scenario: A busy submit button sends no form, from Enter or from Space
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Space
    And I press Tab until focus is on the button "Send the demo form"
    Then the button "Send the demo form" is busy, a spinner a screen reader skips before its label
    When I press Enter
    And I press Space
    Then focus stays on the button "Send the demo form"
    And the polite region stays empty

  Scenario: Under forced colors a button takes the reader's button colors, an unavailable one the color of unavailable text, and pressed keeps its weight and edge
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every button of the "Button" demo is drawn in the reader's button colors, an unavailable one in the color of unavailable text
    When I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Show the demo's hidden files"
    And I press Space
    Then the button "Show the demo's hidden files" is drawn bolder, with an inset edge
    When I press Shift+Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then the button "Demo work in progress" is drawn bolder, with an inset edge
    And every pressed button of the "Button" demo is drawn in the reader's button colors, a Ghost one's edge too

  Scenario: Under forced colors a busy button keeps the reader's button text, apart from an unavailable one
    Given the browser asks for "forced colors"
    When I open the component gallery
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then every busy button of the "Button" demo is drawn in the reader's button colors, its label and edge in the button text, not the color of unavailable text

  Scenario: Keyboard focus on a control shows its tooltip at once, beside the control, which it describes
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." shows beside the button "Prune the demo repository", without covering it
    And the button "Prune the demo repository" is described by "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing."
    And the page has no accessibility violations
    When I press Shift+Tab
    Then no tooltip shows

  Scenario: Escape hides a tooltip and leaves focus on its control, which it still describes
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    And I press Escape
    Then no tooltip shows
    And focus stays on the button "Prune the demo repository"
    And the button "Prune the demo repository" is described by "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing."

  Scenario: A resting pointer shows a tooltip after a moment, which stays while the pointer moves onto it, until Escape
    When I open the component gallery
    And I rest the pointer on the button "Prune the demo repository"
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." shows only once the pointer has rested a moment
    When I move the pointer onto the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing."
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." still shows a moment later
    When I press Escape
    Then no tooltip shows

  Scenario: A tooltip goes once the pointer has left both its control and the tooltip
    When I open the component gallery
    And I rest the pointer on the button "Prune the demo repository"
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." shows only once the pointer has rested a moment
    When I move the pointer away from the button "Prune the demo repository" and its tooltip
    Then no tooltip shows

  Scenario Outline: A tooltip with room on the side its control asks for stands there, in the direction of the control's line
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "<button>"
    Then the tooltip of the button "<button>" stands <where> it

    Examples:
      | button                                | where           |
      | Demo note above                       | above           |
      | Demo note below                       | below           |
      | Demo note at the start                | to the left of  |
      | Demo note at the end                  | to the right of |
      | Demo note at the start, right to left | to the right of |
      | Demo note at the end, right to left   | to the left of  |

  Scenario: At the foot of the page, a tooltip asked to stand below its control has no room there and stands above it
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo note at the foot of the page"
    Then the tooltip of the button "Demo note at the foot of the page" stands above it

  Scenario: On a small screen with larger text, a tooltip with no room at either end of its control stands above or below it, inside the window
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo note at the end"
    Then the tooltip of the button "Demo note at the end" stands above or below it, inside the window
    When I press Shift+Tab until focus is on the button "Demo note at the start"
    Then the tooltip of the button "Demo note at the start" stands above or below it, inside the window

  Scenario: A long note wraps onto lines of its own, as wide as the window allows, never cut and never over its control
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo long note"
    Then the tooltip of the button "Demo long note" wraps onto lines within the window's width, none of its words cut, without covering the button

  Scenario: A tooltip's words are a step smaller than the page's and grow with the reader's text, wrapping at about 20 of their widths
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo long note"
    Then the tooltip of the button "Demo long note" is set a step smaller than the page's text and wraps at 20 times its text size

  Scenario: An icon-only control's tooltip shows the control's name, which a screen reader hears once, from the control
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo schedule"
    Then the button "Demo schedule" shows only an icon, which a screen reader skips
    And the tooltip of the button "Demo schedule" shows "Demo schedule", which a screen reader skips
    And the button "Demo schedule" has no description

  Scenario Outline: A tooltip is drawn in the page's colors swapped, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" is filled with the page's text color, its words in the page's background color, with no shadow

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a tooltip takes the system's colors and an edge in its text color
    Given the browser asks for "forced colors"
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" is drawn in the page's own colors, edged in its text color

  Scenario: A tooltip fades in quickly, and appears at once for a reader who asks for less motion
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" fades in over 100 ms
    When the reader asks for less motion
    Then the tooltip of the button "Prune the demo repository" appears without fading

  Scenario: A tooltip follows its control as the page scrolls
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo note above"
    And the page scrolls up a few lines
    Then the tooltip of the button "Demo note above" stands beside it, without covering it

  Scenario: A text field's error is shown above it, describes it, marks it invalid with a thicker edge in the Error tone, and is announced
    When I open the component gallery
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Enter
    Then the field "Repository name" shows the error "A repository name is required."
    And the field "Repository name" is described by "A repository name is required."
    And the field "Repository name" is marked invalid
    And the field "Repository name" is edged 2 pixels wide in --error
    And the polite region reads "A repository name is required."
    When I press Enter
    Then the field "Repository name" shows no error
    And the field "Repository name" is described only by "The name of the repository to check."
    And the field "Repository name" is not marked invalid
    And the field "Repository name" is edged 1 pixel wide in --field-edge

  Scenario: A text field with no known purpose turns the browser's autocomplete off
    When I open the component gallery
    Then the textbox "Demo host" has the autocomplete token "off"

  Scenario: A field already invalid shows its label, its description and its error above it, each describing it
    When I open the component gallery
    Then the field "Demo host" shows its label, its description and its error above it, in that order
    And the field "Demo host" is described by "The demo machine's host name."
    And the field "Demo host" is described by "Enter a host name, such as"
    And the field "Demo host" is marked invalid
    And the field "Demo host" is edged 2 pixels wide in --error

  Scenario: A multi-line text field takes an entry a line and shows its error as a one-line field does
    When I open the component gallery
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the textbox "Demo folders"
    And I type "/srv/demo/photos"
    And I press Enter
    And I type "/srv/demo/notes"
    Then the textbox "Demo folders" holds the lines "/srv/demo/photos" and "/srv/demo/notes"
    And the field "Demo folders" is described only by "One folder or file a line."
    And the field "Demo folders" is edged 1 pixel wide in --field-edge
    When I press Tab until focus is on the button "Check the demo folders"
    And I press Enter
    Then the field "Demo folders" shows the error "Enter at least one folder."
    And the field "Demo folders" is marked invalid
    And the field "Demo folders" is edged 2 pixels wide in --error
    And the polite region reads "Enter at least one folder."

  Scenario: A password's Show password button shows it and hides it again, keeping its value, its caret and focus, and says it is pressed
    When I open the component gallery
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the textbox "Demo password"
    And I type "demo-secret"
    And I press ArrowLeft
    Then the field "Demo password" hides what it holds
    When I press Tab
    Then focus is on the button "Show password"
    And the button "Show password" is not pressed
    And the button "Show password" is described by "Demo password"
    When I press Space
    Then focus is on the button "Show password"
    And the button "Show password" is pressed
    And the field "Demo password" shows what it holds, "demo-secret", its caret 1 character before its end
    And the textbox "Demo password" has the autocomplete token "current-password"
    And the "Text field" demo has no accessibility violations
    When I press Enter
    Then focus is on the button "Show password"
    And the button "Show password" is not pressed
    And the field "Demo password" hides what it holds
    And the textbox "Demo password" still holds "demo-secret"

  Scenario: A number field asks for the keyboard of digits, is never a spin box, and takes what is typed for its form to check
    When I open the component gallery
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the textbox "Demo copies to keep"
    Then the textbox "Demo copies to keep" asks for the keyboard of digits, and is a text input
    When I type "1a2"
    Then the textbox "Demo copies to keep" still holds "1a2"

  Scenario: A read-only field stays in the Tab order behind a dashed edge, a disabled one is skipped with its Show password button, and each says so in words
    When I open the component gallery
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the textbox "Demo snapshot ID"
    Then the field "Demo snapshot ID" is described by "Read only: you can select and copy it, but not change it."
    And the field "Demo snapshot ID" is edged with dashes
    When I type "9"
    Then the textbox "Demo snapshot ID" still holds "1a2b3c4d"
    When I press Tab
    Then focus is on the textbox "Demo tag at the Small size"
    And the field "Demo repository location" is described by "Unavailable: it can't be changed now."
    And the field "Demo repository location" is disabled
    And the field "Demo keyring password" is described by "Unavailable: it can't be changed now."
    And the field "Demo keyring password" is disabled

  Scenario: A required field says so in words after its label, which a screen reader skips, and says it once itself
    When I open the component gallery
    Then the field "Demo plan name" shows "(required)" after its label, which a screen reader skips
    And the field "Demo plan name" is required, its name its label alone

  Scenario Outline: A text field is as tall as a button of its size, and the two stand level in a row of actions
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the field "<field>" is <times> times as tall as the page's text
    And the field "<field>" is as tall as the button "<button>" beside it, their feet level

    Examples:
      | field                            | button                                   | times |
      | Demo tag at the Small size       | Add the demo tag at the Small size       | 1.75  |
      | Demo tag at the Medium size      | Add the demo tag at the Medium size      | 2     |
      | Demo tag at the Large size       | Add the demo tag at the Large size       | 2.5   |
      | Demo tag at the Extra large size | Add the demo tag at the Extra large size | 3     |

  Scenario Outline: A text field's icon stands at its start, inside its edge, in the page's direction, and a screen reader skips it
    When I open the component gallery asking for the locale "<locale>"
    Then the field with an icon shows it at its start, inside its edge, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario Outline: A text field's edge is drawn in its own token, in light colors and in dark
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the field "Repository name" is edged 1 pixel wide in --field-edge
    And the field "Demo snapshot ID" is edged with dashes
    And the "Text field" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a text field takes the reader's field colors, a disabled one the color of unavailable text, and invalid and read only keep their edges
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every field of the "Text field" demo is drawn in the reader's field colors, a disabled one in the color of unavailable text
    And the field "Demo host" is edged 2 pixels wide
    And the field "Demo snapshot ID" is edged with dashes
