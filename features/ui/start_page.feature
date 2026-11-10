@ui
Feature: Start page
  Every page shares one skeleton: a skip link, landmarks and a single main
  heading. It works with a screen reader and with the keyboard alone (A1, A2,
  A4).

  Scenario Outline: The start page works when the browser asks for <setting>
    Given the browser asks for "<setting>"
    When I open the web UI
    Then the page has no accessibility violations
    And the page is titled "Ladderback"
    And the main heading reads "Welcome to Ladderback"
    When I press Tab
    Then focus is on the link "Skip to content"
    And no text is clipped

    Examples:
      | setting        |
      | light colors   |
      | dark colors    |
      | forced colors  |
      | more contrast  |
      | reduced motion |

  Scenario: The skip link moves keyboard focus to the content
    When I open the web UI
    And I press Tab
    Then focus is on the link "Skip to content"
    When I press Enter
    Then focus is in the main content

  Scenario: The start page reflows on a small screen
    Given a viewport of 320 by 256 pixels
    When I open the web UI
    Then the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: The start page reflows on a small screen with larger text
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the web UI
    Then the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations
    When I press Tab
    Then focus is on the link "Skip to content"

  Scenario: The start page keeps its text when the reader increases text spacing
    Given a viewport of 320 by 256 pixels
    And the reader increases the text spacing
    When I open the web UI
    Then the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: The start page copes with larger text and wider spacing together
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the reader increases the text spacing
    When I open the web UI
    Then the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: The page runs under the daemon's content security policy
    When I open the web UI
    Then the page has a content security policy
    And the browser blocked nothing the page asked for

  Scenario: The browser refuses a script the page doesn't carry
    When I open the web UI
    And a script the page doesn't carry is added to it
    Then the script didn't run and the browser said it blocked it
