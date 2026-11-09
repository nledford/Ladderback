@ui
Feature: Accessibility of the app shell
  Moving between pages and the page's own changes work for a screen reader and
  for the keyboard alone (A1, A3). When a reader navigates to another page,
  focus moves to that page's main heading and the title changes, and the
  announcer, the part of the page that tells a screen reader about changes,
  stays silent: the focused heading is read anyway. The announcer's own
  behavior is tried in the component gallery, a development page at
  `/_gallery`.

  Scenario: Navigating to another page moves focus to its heading and changes the title
    When I open the component gallery
    Then the page is titled "Component gallery – Ladderback"
    And the page has no accessibility violations
    When I press Tab until focus is on the link "Back to the start page"
    And I press Enter
    Then focus is on the main heading "Welcome to Ladderback"
    And the page is titled "Ladderback"
    And the live regions stay silent

  Scenario: A page that does not exist has a heading of its own and takes focus
    When I open the component gallery
    And I press Tab until focus is on the link "Open a page that does not exist"
    And I press Enter
    Then focus is on the main heading "Page not found"
    And the page is titled "Page not found – Ladderback"
    And the page has no accessibility violations
    And the live regions stay silent

  Scenario Outline: A page that does not exist is in the language of the page
    When I open the address "/_gallery?locale=<locale>"
    And I press Tab
    And I press Tab
    And I press Tab
    And I press Enter
    Then focus is on the main heading
    And every visible text is transformed
    And the page title is transformed

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario: Opening a page directly sets its title and leaves focus to the skip link
    When I open the address "/no/such/page"
    Then the page is titled "Page not found – Ladderback"
    When I press Tab
    Then focus is on the link "Skip to content"

  Scenario: A polite message reaches the polite region
    When I open the component gallery
    And I press Tab until focus is on the button "Say politely: backup finished"
    And I press Enter
    Then the polite region reads "Backup finished."
    And the assertive region is empty
    And the page has no accessibility violations

  Scenario: Polite messages sent within two seconds arrive merged
    When I open the component gallery
    And I press Tab until focus is on the button "Say politely: backup finished"
    And I press Enter
    And I press Tab until focus is on the button "Say politely: agent back online"
    And I press Enter
    Then the polite region reads "Backup finished. Agent back online."

  Scenario: An assertive message reaches the assertive region
    When I open the component gallery
    And I press Tab until focus is on the button "Say urgently: restore failed"
    And I press Enter
    Then the assertive region reads "Restore failed."
    And the polite region is empty

  Scenario: A polite message sent while a modal is open is spoken after it closes
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Enter
    Then focus is inside the dialog "Modal stand-in"
    When I press Tab until focus is on the button "Say politely in the modal: backup finished"
    And I press Enter
    Then the polite region stays empty
    When I press Tab until focus is on the button "Close the modal stand-in"
    And I press Enter
    Then the polite region reads "Backup finished."

  Scenario: An urgent message sent while a modal is open is spoken after it closes
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Enter
    Then focus is inside the dialog "Modal stand-in"
    When I press Tab until focus is on the button "Say urgently in the modal: restore failed"
    And I press Enter
    Then the assertive region stays empty
    When I press Tab until focus is on the button "Close the modal stand-in"
    And I press Enter
    Then the assertive region reads "Restore failed."

  Scenario: The gallery has no accessibility violations while a modal is open
    When I open the component gallery
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Enter
    Then focus is inside the dialog "Modal stand-in"
    And the page has no accessibility violations
