@ui
Feature: The pause control
  The control that pauses live updates holds the demo's updates and says once
  what changed when it resumes.

  Scenario: The pause control holds the demo's updates and says once what changed when it resumes
    When I open the component gallery
    And I skip to the content
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
    And I skip to the content
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
    And I skip to the content
    And I press Tab until focus is on the button "Finish a demo run"
    And I press Enter
    Then the polite region reads "Backup finished."
    And the button "Pause the demo updates" has no description
