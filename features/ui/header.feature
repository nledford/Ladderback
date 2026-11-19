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
    And I skip to the content
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

  Scenario Outline: A sign-out that reaches no daemon is an error alert said assertively, and leaves the header's height as it was, at <width> pixels with <size> percent text in <colors>
    Given a viewport of <width> by <height> pixels
    And the reader sets the text size to <size> percent
    And the browser asks for "<colors>"
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    When the height of the header is noted
    And the daemon has stopped
    And I press Shift+Tab until focus is on the button "Sign out"
    And I press Enter
    Then the page shows an error alert containing "did not answer"
    And the assertive region comes to contain "did not answer"
    And the header keeps the height noted
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | width | height | size | colors       |
      | 1280  | 720    | 100  | light colors |
      | 320   | 256    | 200  | dark colors  |

  Scenario: A failed sign-out's error alert isn't shown again once the session has ended another way and the admin signs in again
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/settings#signing-in"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Settings"
    When the next sign-out reaches no daemon
    And I press Shift+Tab until focus is on the button "Sign out"
    And I press Enter
    Then the page shows an error alert containing "did not answer"
    When I press Tab until focus is on the button "Sign out everywhere"
    And I press Enter
    And I press Tab until focus is on the button "Sign out every browser"
    And I press Enter
    Then focus is on the heading "Sign in to Ladderback"
    When I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Settings"
    And the page shows no error alert containing "did not answer"

  # The version's width depends on the build's commit, whose digits differ in
  # width, so the scenario measures the widest and the narrowest version a
  # build between releases can show, never this build's own commit.
  Scenario Outline: On a narrow screen the running version and Sign out end the header's line, at <size> percent text, with the <width> version
    Given a viewport of 320 by <height> pixels
    And the reader sets the text size to <size> percent
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And the running version is the <width> one a build between releases can show
    And I skip to the content
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
      | height | size | width     |
      | 640    | 100  | widest    |
      | 640    | 100  | narrowest |
      | 256    | 200  | widest    |

  # The header's links are the shared link (§4.5): underlined, the product's
  # name in the color of the text around it, the pages' links in Ladder Blue.
  # A page of the navigation is current on its own path and on every page
  # below it, as a snapshot's page is below the Snapshots page.
  Scenario Outline: The product's name and the pages' links are underlined, and the page the reader is in is marked bold, in <colors>
    Given the browser asks for "<colors>"
    When I open the address "/snapshots/demo/3f2a"
    Then every link in the banner is underlined below its letters, 8 in all
    And each link in the banner is drawn in its tone:
      | link            | color              |
      | Ladderback      | the text around it |
      | Operations      | --accent           |
      | Plans           | --accent           |
      | Pending changes | --accent           |
      | Snapshots       | --accent           |
      | Repositories    | --accent           |
      | Log             | --accent           |
      | Settings        | --accent           |
    And the link "Snapshots" is the current page, drawn bold
    And the link "Ladderback" is not the current page, drawn in the weight of the text around it
    And the page has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors the product's name and the pages' links take the system's link color and keep their underline
    Given the browser asks for "forced colors"
    When I open the address "/operations"
    Then every link in the banner is drawn in the system's link color, 8 in all
    And every link in the banner is underlined below its letters, 8 in all
    And the link "Operations" is the current page, drawn bold
