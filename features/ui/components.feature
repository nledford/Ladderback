@ui
Feature: The shared components in the gallery
  The shared component set (technical design §4.5) is exercised in the
  component gallery, a development page at /_gallery, with keyboard-only
  journeys: a dialog, a sortable data table, a form field with an announced
  error, a checkbox, a progress bar whose changes are never announced, a
  toast and the control that pauses live updates. Every demo state a
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

  Scenario: A checkbox turns on and off by keyboard and is described by its hint
    When I open the component gallery
    And I press Tab until focus is on the checkbox "Send a demo notification"
    Then the checkbox "Send a demo notification" is not checked
    And the checkbox "Send a demo notification" is described by "A demo choice: turning it on changes nothing."
    When I press Space
    Then the checkbox "Send a demo notification" is checked
    When I press Space
    Then the checkbox "Send a demo notification" is not checked

  Scenario: The progress bar's value changes without any announcement
    When I open the component gallery
    And I press Tab until focus is on the button "Advance the demo progress"
    And I press Enter
    Then the progress bar "Demo backup progress" shows 80 percent
    And the live regions stay silent

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
      | Checkbox    |
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
      | Checkbox    |
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
    When I press Tab until focus is on the button "Confirm the demo review"
    And I press Tab
    Then focus is on the first control of the dialog "Demo review to confirm"
    When I press Escape
    Then focus is on the button "Open a demo review"

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
