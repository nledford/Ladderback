@ui
Feature: The shared components in the gallery
  The shared component set (technical design §4.5) is exercised in the
  component gallery, a development page at /_gallery, with keyboard-only
  journeys: a dialog, a sortable data table, a form field with an announced
  error, a multi-line form field that takes an entry a line, a checkbox, a
  radio group, a progress bar a keyboard user can reach, whose changes
  are never announced and are told at most every 30 seconds, and which shows
  no percentage while its size isn't known, a toast, the
  control that pauses live updates, and a status badge for every state an
  operation can be in, whose icon tells the state by its shape. Every demo state a
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
