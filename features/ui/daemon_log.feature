@ui
Feature: The Log page
  The Log page shows the recent log: Ladderback's own log lines since the
  daemon started, newest first, as the log writes them, secrets masked
  (§4.4, #618). A choice of levels narrows the lines without moving focus, and
  "Copy for a bug report" puts the lines shown on the clipboard as the log
  writes them, with the running version first.

  Background:
    Given the recent log holds:
      | at                       | level | source                | message                                        |
      | 2026-11-15T09:00:00.000Z | info  | ladderback            | Ladderback starting                            |
      | 2026-11-15T09:00:01.250Z | warn  | ladderback::config    | repository nas: a demo warning                 |
      | 2026-11-15T09:00:02.500Z | error | ladderback::notify    | the notification to demo-channel failed        |
      | 2026-11-15T09:00:03.750Z | info  | ladderback::scheduler | the plan home runs next at 2026-11-15 10:00:00 |

  Scenario: The Log link in the navigation moves focus to the page's heading, and the page lists the lines newest first
    When I open the web UI
    And I press Tab until focus is on the link "Log"
    And I press Enter
    Then focus is on the main heading "Log"
    And the page is titled "Log – Ladderback"
    And the table "Log" lists, from the top, the messages:
      | the plan home runs next at 2026-11-15 10:00:00 |
      | the notification to demo-channel failed        |
      | repository nas: a demo warning                 |
      | Ladderback starting                            |
    And the page has no accessibility violations

  Scenario: Choosing a level shows only the lines at that level or worse, and focus stays on the choice
    When I open the address "/log"
    And I skip to the content
    And I press Tab until focus is on the radio "All levels"
    And I press the down arrow until focus is on the radio "Warnings and errors"
    Then the radio "Warnings and errors" of the group "Show" is checked
    And the table "Log" lists, from the top, the messages:
      | the notification to demo-channel failed |
      | repository nas: a demo warning          |
    When I press the down arrow until focus is on the radio "Errors only"
    Then the table "Log" lists, from the top, the messages:
      | the notification to demo-channel failed |
    And focus is on the radio "Errors only"
    And the page has no accessibility violations

  Scenario: Copy for a bug report puts the lines shown on the clipboard as the log writes them
    Given the page may use the clipboard
    When I open the address "/log"
    And I skip to the content
    And I press Tab until focus is on the radio "All levels"
    And I press the down arrow until focus is on the radio "Warnings and errors"
    And I press Tab until focus is on the button "Copy for a bug report"
    And I press Enter
    Then the polite region comes to contain "Copied the lines shown."
    And the clipboard holds the running version, then the lines:
      """
      [2026-11-15T09:00:02.500Z ERROR ladderback::notify] the notification to demo-channel failed
      [2026-11-15T09:00:01.250Z WARN  ladderback::config] repository nas: a demo warning
      """

  Scenario: A read of the recent log the daemon refuses is said as an alert, in the lines' place
    Given the daemon can't read its sessions when the recent log is read
    When I open the address "/log"
    Then the page shows an alert containing "could not keep or forget sessions"
    And the main heading reads "Log"
    And the page has no accessibility violations

  Scenario: A read of the recent log that reaches no daemon says Ladderback can't be reached
    Given the read of the recent log reaches no daemon
    When I open the address "/log"
    Then the page shows the heading "Cannot reach Ladderback"
    And the page has no accessibility violations

  Scenario Outline: The Log page fits a small screen, at <size> percent text in <colors>
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to <size> percent
    And the browser asks for "<colors>"
    When I open the address "/log"
    Then the main heading reads "Log"
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | size | colors       |
      | 100  | light colors |
      | 200  | dark colors  |
