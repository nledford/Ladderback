@ui
Feature: The header bar
  Every page's header shows the product's name, which leads to the start page,
  then the pages' links on one line with, at that line's end, the running
  version and, while signed in, Sign out. Sign out isn't a page, so it stands
  apart from the pages' links, after them in the order of Tab, and no taller
  than they are, so where the line fits, Sign out doesn't make the header
  taller. A sign-out that fails says so in the page, not in the header, which
  keeps its height, and isn't said again once the session has ended. On a
  narrow screen the version and Sign out wrap under the links, together where
  they fit, and stay at the line's end.

  Scenario: Signed in, the pages' links, the running version and Sign out share one line, and Sign out comes after the pages
    Given a viewport of 1280 by 720 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    And the pages' links, the running version and the button "Sign out" share the header's last line
    When I press Shift+Tab until focus is on the link "Settings"
    And I press Tab
    Then focus is on the button "Sign out"
    And the button "Sign out" is in the banner, outside the navigation
    When I press Tab
    Then focus is on a control in the main content
    And the page has no accessibility violations

  Scenario Outline: A sign-out that reaches no daemon says so and leaves the header's height as it was, at <width> pixels with <size> percent text in <colors>
    Given a viewport of <width> by <height> pixels
    And the reader sets the text size to <size> percent
    And the browser asks for "<colors>"
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    When the height of the header is noted
    And the daemon has stopped
    And I press Shift+Tab until focus is on the button "Sign out"
    And I press Enter
    Then the page shows an alert containing "did not answer"
    And the header keeps the height noted
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | width | height | size | colors       |
      | 1280  | 720    | 100  | light colors |
      | 320   | 256    | 200  | dark colors  |

  Scenario Outline: On a narrow screen the running version and Sign out end the header's line, at <size> percent text
    Given a viewport of 320 by <height> pixels
    And the reader sets the text size to <size> percent
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    And the running version and the button "Sign out" share a line where they fit, at the header's inline end
    And the button "Sign out" is at least 2rem tall, the size of the reader's text
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | height | size |
      | 640    | 100  |
      | 256    | 200  |
