@ui
Feature: The disclosure menu
  A button that shows a panel of links under it, and hides it again: the
  header's Settings menu (decision 152). It is a disclosure, not an
  application menu, so its rows are links and buttons Tab reaches in order,
  with no arrow keys of their own. Its button says whether the panel shows
  and names it, opens it on a press and never under the pointer, and is
  marked while the page the reader is on is one of the panel's. Escape
  closes the panel and returns focus to the button, and so does a row's
  button pressed, as a menu button's item does; the panel closes as well
  when a row's link is chosen, when focus moves out of it and when the
  pointer presses outside it. Focus is never kept in it.

  Scenario: Enter on the button shows the panel's rows, Tab reaches each in order, and Escape hides them and returns focus to the button
    When I open the "Disclosure menu" demo
    And I press Tab until focus is on the button "Demo pages"
    Then the button "Demo pages" is collapsed
    When I press Enter
    Then the button "Demo pages" is expanded
    And the menu of the button "Demo pages" holds these, in order:
      | row                              |
      | Demo menu link to this page      |
      | Demo menu link to the start page |
      | Demo menu action                 |
    And the menu of the button "Demo pages" stands under its button, ending where it ends
    And the "Disclosure menu" demo has no accessibility violations
    When I press Tab
    Then focus is on the link "Demo menu link to this page"
    When I press Tab
    Then focus is on the link "Demo menu link to the start page"
    When I press Shift+Tab
    And I press Shift+Tab
    Then focus is on the button "Demo pages"
    And the button "Demo pages" is expanded
    When I press Tab until focus is on the button "Demo menu action"
    And I press Escape
    Then the button "Demo pages" is collapsed
    And focus is on the button "Demo pages"

  Scenario: Space on the button shows the panel too, and again hides it
    When I open the "Disclosure menu" demo
    And I press Tab until focus is on the button "Demo pages"
    And I press Space
    Then the button "Demo pages" is expanded
    When I press Space
    Then the button "Demo pages" is collapsed
    And focus is on the button "Demo pages"

  Scenario: Tab past the panel's last row leaves the menu and hides the panel
    When I open the "Disclosure menu" demo
    And I press Tab until focus is on the button "Demo pages"
    And I press Enter
    And I press Tab until focus is on the button "Demo menu action"
    And I press Tab
    Then focus is on the button "Demo after the menu"
    And the button "Demo pages" is collapsed

  Scenario: Choosing a row's link hides the panel, and the current page's link returns focus to the button
    When I open the "Disclosure menu" demo
    And I press Tab until focus is on the button "Demo pages"
    And I press Enter
    And I press Tab until focus is on the link "Demo menu link to this page"
    And I press Enter
    Then the button "Demo pages" is collapsed
    And focus is on the button "Demo pages"

  Scenario: Pressing a row's button hides the panel and returns focus to the button
    When I open the "Disclosure menu" demo
    And I press Tab until focus is on the button "Demo pages"
    And I press Enter
    And I press Tab until focus is on the button "Demo menu action"
    And I press Enter
    Then the button "Demo pages" is collapsed
    And focus is on the button "Demo pages"

  Scenario: While the page the reader is on is one of the panel's, the button is marked as its group, bold with a bar at its inline start
    When I open the "Disclosure menu" demo
    Then the button "Demo pages" is marked as the current page's group, drawn bold with a bar at its inline start
    And the button "Demo other pages" is not marked as the current page's group

  Scenario: The pointer resting on the button shows nothing, and a press outside the open panel hides it
    When I open the "Disclosure menu" demo
    And I rest the pointer on the button "Demo pages"
    Then the button "Demo pages" is collapsed
    When I press Tab until focus is on the button "Demo pages"
    And I press Enter
    Then the button "Demo pages" is expanded
    When I press the pointer on the main heading
    Then the button "Demo pages" is collapsed

  Scenario Outline: The panel's edge stands apart from the page under it, in <colors>
    Given the browser asks for "<colors>"
    When I open the "Disclosure menu" demo
    And I press Tab until focus is on the button "Demo pages"
    And I press Enter
    Then the menu of the button "Demo pages" has an edge at least 3 to 1 against what it lies on
    And the "Disclosure menu" demo has no accessibility violations

    Examples:
      | colors        |
      | light colors  |
      | dark colors   |
      | forced colors |

  Scenario: On a small screen with larger text the open panel stays inside the window
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the "Disclosure menu" demo
    And I press Tab until focus is on the button "Demo pages"
    And I press Enter
    Then the menu of the button "Demo pages" lies inside the window
    And the page doesn't scroll sideways
    And no text is clipped
