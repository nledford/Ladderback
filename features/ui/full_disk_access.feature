@ui
Feature: The Full Disk Access warning on every page
  When the agent's Full Disk Access probe finds the grant missing
  (`features/execution/full_disk_access.feature`), every page shows a
  warning banner that says backups read less than they should, with a link
  to the steps on the start page. The banner can't be dismissed while the
  grant is missing. On the start page, a browser on the same Mac gets a
  button that opens the System Settings pane and shows the installed program
  in Finder; any other browser gets the steps. "Check again" asks the agent
  to run the probe again, and the banner and the steps go once the grant is
  found. A grant found or unknown shows no warning.

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

  Scenario Outline: A grant found or unknown shows no warning
    Given the agent has found Full Disk Access <state>
    When I open the address "<page>"
    Then the page doesn't warn of Full Disk Access

    Examples:
      | state   | page          |
      | granted | /             |
      | unknown | /             |
      | granted | /operations   |
      | unknown | /operations   |
      | granted | /settings     |
      | unknown | /settings     |

  Scenario: On a narrow screen with large text, the warning wraps and stays whole
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the agent has found Full Disk Access missing
    When I open the web UI
    Then the page warns that Full Disk Access is missing
    And the page doesn't scroll sideways
    And the page has no accessibility violations

  # The banner is the same on every page the navigation reaches, the start
  # page's own included, where the steps follow it.
  Scenario Outline: A missing grant is a warning banner on every page
    Given the agent has found Full Disk Access missing
    When I open the address "<page>"
    Then the page shows the Full Disk Access banner, its link "How to fix this" leading to the start page
    And the page shows a warning banner that says "so backups leave out Mail, Messages, Safari and other programs' data"
    And the page has no accessibility violations

    Examples:
      | page         |
      | /            |
      | /operations  |
      | /plans       |
      | /proposals   |
      | /snapshots   |
      | /repositories |
      | /log         |
      | /settings    |

  Scenario: The banner can't be dismissed while the grant is missing
    Given the agent has found Full Disk Access missing
    When I open the address "/settings"
    Then the page shows the Full Disk Access banner, its link "How to fix this" leading to the start page
    And the Full Disk Access banner offers no button

  Scenario: The banner's link reaches the steps by keyboard alone
    Given the agent has found Full Disk Access missing
    When I open the Operations page
    And I skip to the content
    And I press Tab
    Then focus is on the link "How to fix this"
    When I press Enter
    Then focus is on the main heading "Welcome to Ladderback"
    When I press Tab until focus is on the button "Check again"
    Then the page lists the steps to grant Full Disk Access
    And the page has no accessibility violations

  Scenario: Check again finding the grant takes the banner off every page
    Given the agent has found Full Disk Access missing
    And the agent finds the grant when it checks again
    When I open the web UI
    Then the page shows the Full Disk Access banner, its link "How to fix this" leading to the start page
    When I skip to the content
    And I press Tab until focus is on the button "Check again"
    And I press Enter
    Then the page shows no Full Disk Access banner
    And focus is on the main heading "Welcome to Ladderback"
    When I press Shift+Tab until focus is on the link "Operations"
    And I press Enter
    Then focus is on the main heading "Operations"
    And the page shows no Full Disk Access banner

  Scenario: On a narrow screen with large text, the banner wraps and stays whole on another page
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the agent has found Full Disk Access missing
    When I open the Operations page
    Then the page shows the Full Disk Access banner, its link "How to fix this" leading to the start page
    And the page doesn't scroll sideways
    And the page has no accessibility violations

  # The heading takes focus when a reader goes to a page, so a screen reader
  # reads the banner above it as the heading's description (A3).
  Scenario: A reader who moves to another page hears the warning after its heading
    Given the agent has found Full Disk Access missing
    When I open the web UI
    And I press Tab until focus is on the link "Operations"
    And I press Enter
    Then focus is on the main heading "Operations"
    And the page's heading is described by "so backups leave out Mail"

  Scenario: The banner follows what the agent finds as the reader moves between pages
    Given the agent has found Full Disk Access granted
    When I open the web UI
    Then the page doesn't warn of Full Disk Access
    When the agent then finds Full Disk Access missing
    And I press Tab until focus is on the link "Operations"
    And I press Enter
    Then focus is on the main heading "Operations"
    And the page shows the Full Disk Access banner, its link "How to fix this" leading to the start page
