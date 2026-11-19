@ui
Feature: Choosing the width of the pages
  Settings' Page width section chooses whether each page's content stays
  narrow, at most 60rem wide and centered, or uses the window's whole width.
  Narrow is the choice until another is made. The choice is kept in the
  browser that made it, as the language is (decision 101): it suits one screen,
  so it is neither Ladderback's nor the daemon's. It applies as the page
  starts, and as soon as it is chosen, with no reload. In Full width the
  tables use the window's width, while paragraphs and forms keep a readable
  line length of about 70 characters. A window 320 px wide shows the page
  across its width with either choice.

  Scenario: Settings offers Narrow and Full width, and Narrow keeps a wide window's page at 60rem
    Given a viewport of 1600 by 900 pixels
    When I open the address "/settings#page-width"
    Then the group "Width of each page" offers the radios, in order:
      | Narrow     |
      | Full width |
    And the radio "Narrow" of the group "Width of each page" is checked
    And the group "Width of each page" is described by "Kept in this browser, and applied as soon as it's chosen. Full width lets tables use the whole window, while paragraphs and forms keep a readable line length."
    And the main content is no wider than 60 rem
    And the main content is centered in the window
    And the page has no accessibility violations

  Scenario: Full width chosen by keyboard alone widens the page at once, and after a reload
    Given a viewport of 1600 by 900 pixels
    When I open the address "/settings#page-width"
    And I skip to the content
    And I press Tab until focus is on the radio "Narrow"
    And I press the down arrow until focus is on the radio "Full width"
    Then the radio "Full width" of the group "Width of each page" is checked
    And the main content is as wide as the window
    And no paragraph or form of the main content is wider than 70 characters
    And the page has no accessibility violations
    When I reload the page
    Then the radio "Full width" of the group "Width of each page" is checked
    And the main content is as wide as the window

  Scenario: Narrow chosen again by keyboard alone brings the page back to 60rem, and keeps it there
    Given a viewport of 1600 by 900 pixels
    And the page width chosen in this browser is "Full width"
    When I open the address "/settings#page-width"
    Then the main content is as wide as the window
    When I skip to the content
    And I press Tab until focus is on the radio "Full width"
    And I press the up arrow until focus is on the radio "Narrow"
    Then the main content is no wider than 60 rem
    When I reload the page
    Then the radio "Narrow" of the group "Width of each page" is checked
    And the main content is no wider than 60 rem

  Scenario: In Full width, the Operations table uses the window's width
    Given a viewport of 1600 by 900 pixels
    And the daemon reports two finished backups of "demo-home"
    And the page width chosen in this browser is "Full width"
    When I open the address "/operations"
    Then the main content is as wide as the window
    And the table "Operations" is at least 1500 pixels wide
    And the page has no accessibility violations

  Scenario Outline: A window 320 pixels wide shows the page across its width with either choice
    Given a viewport of 320 by 640 pixels
    And the page width chosen in this browser is "<width>"
    When I open the address "/settings#page-width"
    Then the radio "<width>" of the group "Width of each page" is checked
    And the main content is as wide as the window

    Examples:
      | width      |
      | Narrow     |
      | Full width |

  Scenario: A browser that keeps nothing says so in an error alert, said assertively, and the page stays narrow
    Given a viewport of 1600 by 900 pixels
    And this browser keeps nothing a page stores
    When I open the address "/settings#page-width"
    And I skip to the content
    And I press Tab until focus is on the radio "Narrow"
    And I press the down arrow until focus is on the radio "Full width"
    Then the page shows an error alert containing "This browser didn't keep the page width, so the page stays as it was."
    And the assertive region comes to contain "This browser didn't keep the page width, so the page stays as it was."
    And the radio "Narrow" of the group "Width of each page" is checked
    And the main content is no wider than 60 rem
    And the page has no accessibility violations
