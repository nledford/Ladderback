@ui
Feature: The shared components in the gallery
  The shared component set (technical design §4.5) is exercised in the
  component gallery, a development page at /_gallery, with keyboard-only
  journeys: a dialog, a sortable data table, a form field with an announced
  error, a multi-line form field that takes an entry a line, a checkbox, a
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
  placements, which follow the page's direction, and a spinner, which a
  label names, which never speaks for itself, which takes its tone, size
  and placement from that vocabulary and which stands still for a reader
  who asks for less motion. Every demo state a
  component can show (loading, empty, error, success, on and off) stays clear
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

  Scenario: The table can be sorted by keyboard and the sort state is announced
    When I open the component gallery
    And I press Tab until focus is on the button "Sort by repository"
    And I press Enter
    Then the table "Demo repositories" is sorted by repository ascending
    And the polite region reads "Sorted by repository, ascending."

  Scenario: The table's sort button sits in its column header apart from the header's text
    When I open the component gallery
    Then the button "Sort by repository" sits inside the column header "Repository", apart from its text

  Scenario: A form field error is shown, describes the input and is announced
    When I open the component gallery
    And I press Tab until focus is on the button "Check the repository name"
    And I press Enter
    Then the field "Repository name" shows the error "A repository name is required."
    And the field "Repository name" is described by "A repository name is required."
    And the field "Repository name" is marked invalid
    And the polite region reads "A repository name is required."
    When I press Enter
    Then the field "Repository name" shows no error
    And the field "Repository name" is described only by "The name of the repository to check."
    And the field "Repository name" is not marked invalid

  Scenario: A form field with no known purpose turns the browser's autocomplete off
    When I open the component gallery
    Then the textbox "Repository name" has the autocomplete token "off"

  Scenario: A multi-line form field takes an entry a line and shows its error as a form field does
    When I open the component gallery
    And I press Tab until focus is on the textbox "Demo folders"
    And I type "/srv/demo/photos"
    And I press Enter
    And I type "/srv/demo/notes"
    Then the textbox "Demo folders" holds the lines "/srv/demo/photos" and "/srv/demo/notes"
    And the field "Demo folders" is described only by "One folder or file a line."
    When I press Tab until focus is on the button "Check the demo folders"
    And I press Enter
    Then the field "Demo folders" shows the error "Enter at least one folder."
    And the field "Demo folders" is marked invalid
    And the polite region reads "Enter at least one folder."

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
      | Data table  |
      | Form field  |
      | Multi-line form field |
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
      | Step-up field |

  Scenario Outline: Each component's structure is pinned by an ARIA snapshot
    When I open the component gallery
    Then the "<component>" demo matches its ARIA snapshot

    Examples:
      | component   |
      | Dialog      |
      | Data table  |
      | Form field  |
      | Multi-line form field |
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

  Scenario Outline: A button's label grows with the reader's text size
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Tab until focus is on the button "<button>"
    Then the label of the button "<button>" is as large as the page's text
    And the page has no accessibility violations

    Examples:
      | button                    |
      | Open the demo dialog      |
      | Sort by repository        |
      | Check the repository name |
      | Pause the demo updates    |

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
    And the button "Saving the demo settings…" holds a spinner a screen reader skips

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
