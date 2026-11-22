@ui
Feature: The header bar
  Every page's header shows the product's name, which leads to the start page,
  then the pages' links on one line with, while signed in, Sign out at that
  line's end. The running version is in the footer, not here (decision 142).
  Sign out isn't a page, so it stands apart from the pages' links, after them
  in the order of Tab, and no taller than they are, so where the line fits,
  Sign out doesn't make the header taller. A sign-out that fails says so in
  the page, not in the header, which keeps its height, and isn't said again
  once the session has ended. On a narrow screen Sign out wraps under the
  links and stays at the line's end.

  Scenario: Signed in, the pages' links and Sign out share one line, Sign out comes after the pages, and the version is not in the header
    Given a viewport of 1280 by 720 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    And the pages' links and the button "Sign out" share the header's last line
    And the banner doesn't show the running version
    When I press Shift+Tab until focus is on the link "Settings"
    And I press Tab
    Then focus is on the button "Sign out"
    And the button "Sign out" is in the banner, outside the navigation
    When I press Tab
    Then focus is on a control in the main content
    And the page has no accessibility violations

  # Inside the page's frame the pseudolocale's longer words leave the line
  # little room: in DejaVu Sans, the widest font a Linux machine falls back
  # on, the seven links and Sign out take 903 px of the frame's 928, in the
  # macOS system font about 820.
  Scenario: Signed in with the longer words of a pseudolocale, the pages' links and Sign out still share one line
    Given a viewport of 1280 by 720 pixels
    And the language chosen in this browser is "en-XA"
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    When I open the address "/repositories"
    Then the pages' links and the banner's one button share the header's last line
    And the page doesn't scroll sideways

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

  Scenario Outline: On a narrow screen Sign out ends the header's line, at <size> percent text, and the version is not in the header
    Given a viewport of 320 by <height> pixels
    And the reader sets the text size to <size> percent
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    And the button "Sign out" ends its line, at the header's inline end
    And the banner doesn't show the running version
    And the button "Sign out" is at least 2rem tall, the size of the reader's text
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | height | size |
      | 640    | 100  |
      | 256    | 200  |

  # The header's links are the shared link (§4.5), underlined and in the
  # color of the text around them, the product's name and the pages' links
  # alike: the current page is told by its weight, and the page's own action
  # is the first Ladder Blue on the screen (decision 141). A page of the
  # navigation is current on its own path and on every page below it, as a
  # snapshot's page is below the Snapshots page.
  Scenario Outline: The product's name and the pages' links are underlined, in the color of the text around them, and the page the reader is in is marked bold, in <colors>
    Given the browser asks for "<colors>"
    When I open the address "/snapshots/demo/3f2a"
    Then every link in the banner is underlined below its letters, 8 in all
    And each link in the banner is drawn in its tone:
      | link            | color              |
      | Ladderback      | the text around it |
      | Operations      | the text around it |
      | Plans           | the text around it |
      | Pending changes | the text around it |
      | Snapshots       | the text around it |
      | Repositories    | the text around it |
      | Log             | the text around it |
      | Settings        | the text around it |
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
