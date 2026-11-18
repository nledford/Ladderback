@ui
Feature: The dialog
  A dialog opens from a button, keeps focus inside it, closes on Escape and
  returns focus to the button.

  Scenario: A dialog opens from a button, traps focus, closes on Escape and returns focus
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the demo dialog"
    And I press Enter
    Then focus is inside the dialog "Demo dialog"
    And I press Tab until focus is on the button "Close the demo dialog"
    And I press Tab
    Then focus is on the first control of the dialog "Demo dialog"
    When I press Escape
    Then focus is on the button "Open the demo dialog"

  Scenario: A dialog can start on a button of its own, hand Escape to its page, and close once its page has answered
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the demo warning"
    And I press Enter
    Then the dialog "Demo warning" opens with focus on the button "Keep the demo warning"
    And the dialog "Demo warning" is described as saying "Escape keeps this warning open"
    And the page has no accessibility violations
    When I press Escape
    Then the dialog "Demo warning" politely says "The page keeps the demo warning open."
    And the dialog "Demo warning" stays open
    When I press Tab
    Then focus is on the first control of the dialog "Demo warning"
    And focus is on the button "Answer the demo warning"
    When I press Enter
    Then the dialog "Demo warning" is closed
    And focus is on the button "Open the demo warning"

  Scenario: A dialog can be titled under its section and return focus to the control it names, even one drawn anew
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Turn the demo setting on"
    And I press Enter
    Then the dialog "Demo setting" opens with focus on the button "Keep the demo setting"
    And the dialog "Demo setting" is titled by a heading at level 3
    And the dialog "Demo setting" is described as saying "drawn anew"
    And the page has no accessibility violations
    When I press Tab
    Then focus is on the button "Change the demo setting"
    When I press Enter
    Then the dialog "Demo setting" is closed
    And focus is on the button "Turn the demo setting off"
