@ui
Feature: The navigation bar
  The brand, the pages' links and a group of rows behind a disclosure menu:
  the header of every page (decisions 143, 152 and 160). From 54rem up it is
  one line. Below 54rem a hamburger button ends the brand's line and shows
  every row in a panel that stands over the page, so opening and closing it
  moves nothing on the page. The panel closes as the group's menu does: on
  Escape, which returns focus to the hamburger, on a row chosen, on focus
  leaving the bar and on a press of the pointer outside it. Focus is never
  kept in it, and Tab reaches every row in order with no arrow keys.

  Scenario: From 54rem up the brand, the pages' links and the group's button stand on one line, and the hamburger isn't drawn
    Given a viewport of 1280 by 800 pixels
    When I open the "Navigation bar" demo
    Then the navigation "Demo navigation" and its brand stand on one line
    And the button "Demo menu" is on the page but not drawn
    And the link "Demo bar link to this page" is the current page, drawn bold
    And the button "Demo more pages" is collapsed
    And the "Navigation bar" demo has no accessibility violations

  Scenario: Below 54rem Enter on the hamburger shows every row over the page, Tab reaches each in order, and Escape hides them and returns focus to it
    Given a viewport of 390 by 844 pixels
    When I open the "Navigation bar" demo
    And I press Tab until focus is on the button "Demo menu"
    Then the button "Demo menu" is collapsed
    When I note where the button "Demo after the bar" stands
    And I press Enter
    Then the button "Demo menu" is expanded
    And the rows of the panel of the button "Demo menu" are these, in order, in 2 columns, each at least 2.75 rem tall:
      | row                        |
      | Demo bar link to this page |
      | Demo bar link to the plans |
      | Demo bar link to the log   |
      | Demo bar action            |
    And the button "Demo after the bar" stands where it stood
    And the panel of the button "Demo menu" covers the button "Demo after the bar"
    And the "Navigation bar" demo has no accessibility violations
    When I press Tab
    Then focus is on the link "Demo bar link to this page"
    When I press Tab until focus is on the button "Demo bar action"
    And I press Escape
    Then the button "Demo menu" is collapsed
    And focus is on the button "Demo menu"

  Scenario: Space on the hamburger shows the panel too, and again hides it
    Given a viewport of 390 by 844 pixels
    When I open the "Navigation bar" demo
    And I press Tab until focus is on the button "Demo menu"
    And I press Space
    Then the button "Demo menu" is expanded
    When I press Space
    Then the button "Demo menu" is collapsed
    And focus is on the button "Demo menu"

  Scenario: Tab past the panel's last row leaves the bar and hides the panel
    Given a viewport of 390 by 844 pixels
    When I open the "Navigation bar" demo
    And I press Tab until focus is on the button "Demo menu"
    And I press Enter
    And I press Tab until focus is on the button "Demo bar action"
    And I press Tab
    Then focus is on the button "Demo after the bar"
    And the button "Demo menu" is collapsed

  Scenario: Choosing the current page's row hides the panel and returns focus to the hamburger
    Given a viewport of 390 by 844 pixels
    When I open the "Navigation bar" demo
    And I press Tab until focus is on the button "Demo menu"
    And I press Enter
    And I press Tab until focus is on the link "Demo bar link to this page"
    And I press Enter
    Then the button "Demo menu" is collapsed
    And focus is on the button "Demo menu"

  Scenario: A press of the pointer outside the open panel hides it
    Given a viewport of 390 by 844 pixels
    When I open the "Navigation bar" demo
    And I press Tab until focus is on the button "Demo menu"
    And I press Enter
    Then the button "Demo menu" is expanded
    When I press the pointer on the main heading
    Then the button "Demo menu" is collapsed

  Scenario: On a small screen with larger text the panel's rows stand in one column, inside the window's width
    Given a viewport of 320 by 568 pixels
    And the reader sets the text size to 200 percent
    When I open the "Navigation bar" demo
    And I press Tab until focus is on the button "Demo menu"
    And I press Enter
    Then the rows of the panel of the button "Demo menu" are these, in order, in 1 column, each at least 2.75 rem tall:
      | row                        |
      | Demo bar link to this page |
      | Demo bar link to the plans |
      | Demo bar link to the log   |
      | Demo bar action            |
    And the page doesn't scroll sideways
    And no text is clipped
