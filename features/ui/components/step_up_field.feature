@ui
Feature: The step-up field
  The step-up field takes the admin password from a password manager or a
  paste, or shows a short ID to copy and paste back.

  Scenario: The step-up field takes a password from a password manager or paste
    When I open the component gallery
    And I skip to the content
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
    And I skip to the content
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
    And I skip to the content
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
