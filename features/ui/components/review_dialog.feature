@ui
Feature: The review dialog
  A review dialog opens on its dismiss button, keeps Tab inside, says why it
  is held, and returns focus as it closes.

  Scenario: A review dialog opens on its dismiss button, keeps Tab inside and returns focus on Escape
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Enter
    Then focus is inside the dialog "Demo review to confirm"
    And focus is on the button "Cancel"
    And the dialog "Demo review to confirm" is titled by a heading at level 3
    When I press Tab until focus is on the button "Confirm the demo review now"
    And I press Tab
    Then focus is on the first control of the dialog "Demo review to confirm"
    When I press Escape
    Then focus is on the button "Open a demo review"

  Scenario: A review dialog's second way to go ahead comes after its confirm button
    When I open the component gallery
    And I skip to the content
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
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Enter
    And I press Tab until focus is on the button "Confirm the demo review"
    And I press Enter
    Then focus is on the button "Open a demo review"
    And the polite region reads "Demo review confirmed."

  Scenario: A review dialog with nothing to do offers only to close
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open an empty demo review"
    And I press Enter
    Then focus is inside the dialog "Demo review with nothing to do"
    And focus is on the button "Close"
    And the dialog "Demo review with nothing to do" offers only the button "Close"
    When I press Enter
    Then focus is on the button "Open an empty demo review"

  Scenario: A review dialog can open on a button of its own, and still closes on Escape and returns focus
    When I open the component gallery
    And I skip to the content
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
    And I skip to the content
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
    And I skip to the content
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
