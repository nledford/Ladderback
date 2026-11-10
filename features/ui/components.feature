@ui
Feature: The shared components in the gallery
  The shared component set (technical design §4.5) is exercised in the
  component gallery, a development page at /_gallery, with keyboard-only
  journeys: a dialog, a sortable data table, a form field with an announced
  error, a progress bar whose changes are never announced, and a toast. Every
  demo state a component can show (loading, empty, error, success) stays clear
  of accessibility violations, and each component's structure is pinned by an
  ARIA snapshot.

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

  Scenario Outline: Each component's demo states have no accessibility violations
    When I open the component gallery
    Then the "<component>" demo has no accessibility violations


    Examples:
      | component   |
      | Dialog      |
      | Data table  |
      | Form field  |
      | Status badge |
      | Progress    |
      | Toast       |
      | Review dialog |

  Scenario Outline: Each component's structure is pinned by an ARIA snapshot
    When I open the component gallery
    Then the "<component>" demo matches its ARIA snapshot

    Examples:
      | component   |
      | Dialog      |
      | Data table  |
      | Form field  |
      | Status badge |
      | Progress    |
      | Toast       |
      | Review dialog |

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
