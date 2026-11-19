@ui
Feature: The Full Disk Access warning on the start page
  When the agent's Full Disk Access probe finds the grant missing
  (`features/execution/full_disk_access.feature`), the start page warns that
  backups read less than they should and says how to grant it. A browser on
  the same Mac gets a button that opens the System Settings pane and shows
  the installed program in Finder; any other browser gets the steps. "Check
  again" asks the agent to run the probe again, and the warning goes once the
  grant is found; while it is still missing, a toast in the warning says so,
  since the warning itself doesn't change. A grant found or unknown shows no
  warning.

  Scenario: A missing grant is a warning with Open Settings and Check again
    Given the agent has found Full Disk Access missing
    And the browser runs on the agent's own Mac
    When I open the web UI
    Then the page warns that Full Disk Access is missing
    And the page offers the button "Open Full Disk Access settings"
    And the page offers the button "Check again"
    And the page has no accessibility violations

  Scenario: A browser on another computer gets the steps, not the button
    Given the agent has found Full Disk Access missing
    When I open the web UI
    Then the page warns that Full Disk Access is missing
    And the page lists the steps to grant Full Disk Access
    And the page offers no button "Open Full Disk Access settings"

  Scenario: Check again, by keyboard, clears the warning once the grant is found
    Given the agent has found Full Disk Access missing
    And the agent finds the grant when it checks again
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the button "Check again"
    And I press Enter
    Then the page doesn't warn of Full Disk Access
    And focus is on the main heading "Welcome to Ladderback"
    And the page has no accessibility violations

  Scenario: Check again that finds the grant still missing is answered in a toast in the warning, said once, and focus stays on it
    Given the agent has found Full Disk Access missing
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the button "Check again"
    And I press Enter
    Then the toast reads "Full Disk Access is still missing." after its icon "Information"
    And the toast is no live region, nor holds one
    And focus stays on the button "Check again"
    And the page warns that Full Disk Access is missing
    And the polite region reads "Full Disk Access is still missing."
    And the polite region keeps reading "Full Disk Access is still missing."
    And the page has no accessibility violations

  Scenario Outline: A grant found or unknown shows no warning
    Given the agent has found Full Disk Access <state>
    When I open the web UI
    Then the page doesn't warn of Full Disk Access

    Examples:
      | state   |
      | granted |
      | unknown |

  Scenario: On a narrow screen with large text, the warning wraps and stays whole
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the agent has found Full Disk Access missing
    When I open the web UI
    Then the page warns that Full Disk Access is missing
    And the page doesn't scroll sideways
    And the page has no accessibility violations

  Scenario: The warning is a warning banner
    Given the agent has found Full Disk Access missing
    When I open the web UI
    Then the page shows a warning banner that says "Full Disk Access is missing"
    And the page warns that Full Disk Access is missing
    And the page has no accessibility violations
