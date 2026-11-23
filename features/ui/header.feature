@ui
Feature: The header bar
  Every page's header is one line where it fits: the product's name, which
  leads to the start page, at its start, and at its end the navigation: the
  links to the pages used most, then the Settings menu, a button that shows
  Settings, Log and, while signed in, Sign out (decision 152). The running
  version is in the footer, not here (decision 142). The menu is a
  disclosure, not an application menu: it opens on a press, never under the
  pointer, and closes with Escape, a page chosen, focus moving out of it or
  a press outside it. While the reader is on one of its pages, its button
  says so. A sign-out that fails says so in the page, not in the header,
  which keeps its height, and isn't said again once the session has ended.
  Below 54rem the line doesn't fit, so everything sits in a menu behind a
  hamburger button, an icon-only one named "Menu": the header is then the
  brand's line, with the button at its end, and, shown, a panel of one row
  for each page and for Sign out, in the same groups (decision 143).

  Scenario: Signed in at 1280 pixels, the product's name, the pages' links and the Settings button share one line, and the version is not in the header
    Given a viewport of 1280 by 720 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    And the link "Ladderback", the pages' links and the button "Settings" share one line
    And the navigation shows these links, in order:
      | link            |
      | Operations      |
      | Plans           |
      | Snapshots       |
      | Repositories    |
      | Pending changes |
    And the button "Sign out" is hidden
    And the banner doesn't show the running version
    When I press Shift+Tab until focus is on the button "Settings"
    Then the button "Settings" is collapsed
    And the button "Settings" is in the navigation, after its links
    And the banner shows no button "Menu"
    When I press Tab
    Then focus is on a control in the main content
    And the page has no accessibility violations

  # The line holds "Pending changes (12)", the count #1476 adds, with 5 %
  # to spare for the wider fonts of a Linux machine: in the macOS system font
  # the line needs about 785 px with it, 815 px with 2rem between the brand
  # and the links, so 54rem (864 px) is the narrowest whole rem where it fits.
  Scenario Outline: The header is one line from 54rem up and the hamburger below it, at <width> pixels
    Given a viewport of <width> by 720 pixels
    When I open the address "/operations"
    Then <layout>
    And the page doesn't scroll sideways
    And no text is clipped

    Examples:
      | width | layout                                                                                 |
      | 864   | the link "Ladderback", the pages' links and the button "Settings" share one line |
      | 863   | the button "Menu" is collapsed                                                         |

  # Inside the page's frame a pseudolocale's longer words don't fit on the
  # brand's line, so the navigation moves whole onto a line of its own under
  # it, the Settings button still at its end.
  Scenario: Signed in with the longer words of a pseudolocale, the pages' links and the Settings button still share one line
    Given a viewport of 1280 by 720 pixels
    And the language chosen in this browser is "en-XA"
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    When I open the address "/repositories"
    Then the pages' links and the banner's one button share the header's last line
    And the page doesn't scroll sideways

  # With the page's text twice as large the line is twice as long, so the
  # navigation wraps under the brand, and the open menu stays in the window.
  Scenario: With 200 percent text at 1280 pixels the header wraps inside the window, and the open Settings menu stays in it
    Given a viewport of 1280 by 720 pixels
    And the reader sets the text size to 200 percent
    When I open the address "/repositories"
    And I skip to the content
    And I press Shift+Tab until focus is on the button "Settings"
    And I press Enter
    Then the button "Settings" is expanded
    And the menu of the button "Settings" lies inside the window
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: The Settings menu shows Settings, Log and Sign out, and Escape closes it and returns focus to its button
    Given a viewport of 1280 by 720 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Shift+Tab until focus is on the button "Settings"
    And I press Enter
    Then the button "Settings" is expanded
    And the menu of the button "Settings" holds these, in order:
      | row      |
      | Settings |
      | Log      |
      | Sign out |
    And the menu of the button "Settings" stands under its button, ending where it ends
    And the page has no accessibility violations
    When I press Tab
    Then focus is on the link "Settings"
    When I press Escape
    Then the button "Settings" is collapsed
    And focus is on the button "Settings"
    And the link "Log" is hidden

  # A page of the menu marks the menu's button, so where the reader is shows
  # with the menu closed.
  Scenario: On a page of the Settings menu its button is marked as the current page's group, bold with a bar, and elsewhere it isn't
    Given a viewport of 1280 by 720 pixels
    When I open the address "/log"
    Then the button "Settings" is marked as the current page's group, drawn bold with a bar at its inline start
    When I open the address "/plans"
    Then the button "Settings" is not marked as the current page's group
    And the link "Plans" is the current page, drawn bold
    And the link "Plans" is marked by a bar at its inline start, in the text's color

  Scenario: Choosing a page from the Settings menu closes it and goes there
    Given a viewport of 1280 by 720 pixels
    When I open the address "/operations"
    And I skip to the content
    And I press Shift+Tab until focus is on the button "Settings"
    And I press Enter
    And I press Tab until focus is on the link "Log"
    And I press Enter
    Then focus is on the heading "Log"
    And the button "Settings" is collapsed
    And the button "Settings" is marked as the current page's group, drawn bold with a bar at its inline start
    And the page has no accessibility violations

  # No focus trap: Tab past the menu's last row leaves it, and the menu
  # closes behind it, so it covers nothing focus lands on.
  Scenario: Tab past the Settings menu's last row leaves it and closes it
    Given a viewport of 1280 by 720 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Shift+Tab until focus is on the button "Settings"
    And I press Enter
    And I press Tab until focus is on the button "Sign out"
    And I press Tab
    Then focus is on a control in the main content
    And the button "Settings" is collapsed

  Scenario: The Settings menu opens on a press, never under the pointer, and a press outside it closes it
    Given a viewport of 1280 by 720 pixels
    When I open the address "/operations"
    And I rest the pointer on the button "Settings"
    Then the button "Settings" is collapsed
    When I press Tab until focus is on the button "Settings"
    And I press Enter
    Then the button "Settings" is expanded
    When I press the pointer on the main heading
    Then the button "Settings" is collapsed

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
    And I press Shift+Tab until focus is on the button "Settings"
    And I press Enter
    And I press Tab until focus is on the button "Sign out"
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

  # Below 54rem Sign out is a row of the menu's panel, so the button shows the
  # panel first; the failed sign-out leaves the panel open, where focus is.
  Scenario: A sign-out that reaches no daemon from the menu below 54rem is an error alert said assertively, and leaves the menu open and the header's height as it was, at 320 pixels with 200 percent text in dark colors
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the browser asks for "dark colors"
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    When I press Shift+Tab until focus is on the button "Menu"
    And I press Enter
    Then the button "Menu" is expanded
    When the height of the header is noted
    And the daemon has stopped
    And I press Tab until focus is on the button "Sign out"
    And I press Enter
    Then the page shows an error alert containing "did not answer"
    And the assertive region comes to contain "did not answer"
    And the header keeps the height noted
    And the button "Menu" is expanded
    And focus is on the button "Sign out"
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: A failed sign-out's error alert isn't shown again once the session has ended another way and the admin signs in again
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/settings#signing-in"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Settings"
    When the next sign-out reaches no daemon
    And I press Shift+Tab until focus is on the button "Settings"
    And I press Enter
    And I press Tab until focus is on the button "Sign out"
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

  # Below 54rem the pages' links and the Settings button no longer fit on the
  # brand's line, so every page and Sign out sit in a panel behind a hamburger
  # button, as on most small screens (decision 143): the brand's line ends
  # with the icon-only button, a finger's target, and the panel stands under
  # it when shown.
  Scenario Outline: On a screen narrower than 54rem the pages' links and Sign out sit behind an icon-only Menu button, at <size> percent text
    Given a viewport of <width> by <height> pixels
    And the reader sets the text size to <size> percent
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    And the header is under 9 rem tall
    And the button "Menu" is collapsed
    And the button "Menu" shows an icon and no words
    And the button "Menu" is at least 44 pixels square
    And the pages' links are hidden
    And the button "Sign out" is hidden
    And the banner doesn't show the running version
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | width | height | size |
      | 390   | 844    | 100  |
      | 320   | 640    | 100  |
      | 320   | 256    | 200  |

  # The panel holds the Settings menu's rows as rows of its own, after the
  # pages', with no button of its own: no menu in a menu.
  Scenario: The Menu button shows the pages' links, then Settings, Log and Sign out, as rows of a panel, Tab reaches each in order, and Escape hides them and returns focus to the button
    Given a viewport of 390 by 844 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    When I press Shift+Tab until focus is on the button "Menu"
    And I press Enter
    Then the button "Menu" is expanded
    And the button "Menu" names the menu panel
    And the link "Ladderback" and the button "Menu" share a line
    And the menu panel is a bordered box with rounded corners, under the brand's line
    And the banner shows no button "Settings"
    And the menu panel's rows are these, in order, each at least 3 rem tall and as wide as the panel:
      | row             |
      | Operations      |
      | Plans           |
      | Snapshots       |
      | Repositories    |
      | Pending changes |
      | Settings        |
      | Log             |
      | Sign out        |
    And the link "Repositories" is the current page, drawn bold
    And the link "Repositories" is marked by a bar at its inline start that no other row has
    When I press Tab
    Then focus is on the link "Operations"
    When I press Tab until focus is on the link "Log"
    And I press Tab
    Then focus is on the button "Sign out"
    When I press Escape
    Then the button "Menu" is collapsed
    And the pages' links are hidden
    And focus is on the button "Menu"
    And the header is under 9 rem tall
    And the page has no accessibility violations

  # The panel's box and the current page's bar are what tell the reader where
  # the menu is and where they are, so they stay in the reader's colors.
  Scenario Outline: The menu panel's border and the current page's bar stand apart from what they lie on, in <colors>
    Given a viewport of 390 by 844 pixels
    And the browser asks for "<colors>"
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    When I press Shift+Tab until focus is on the button "Menu"
    And I press Enter
    Then the menu panel's border and the bar of the link "Repositories" show, at least 3 to 1 against what they lie on
    And the page has no accessibility violations

    Examples:
      | colors        |
      | light colors  |
      | dark colors   |
      | forced colors |

  # The button is drawn below 54rem alone. Where it isn't, Escape has nothing
  # to close and nowhere to return focus to, so it changes nothing.
  Scenario: Escape in the header with the window widened past 54rem leaves the Menu button as it was
    Given a viewport of 390 by 844 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    When I press Shift+Tab until focus is on the button "Menu"
    And I press Enter
    Then the button "Menu" is expanded
    When the window widens to 1280 pixels
    And I press Tab until focus is on the link "Pending changes"
    And I press Escape
    Then focus is on the link "Pending changes"
    When the window narrows to 390 pixels
    Then the button "Menu" is expanded
    And the pages' links are shown below the brand's line, each at least 3 rem tall
    And the page has no accessibility violations

  Scenario: Choosing a page from the menu closes it again
    Given a viewport of 390 by 844 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    When I press Shift+Tab until focus is on the button "Menu"
    And I press Enter
    Then the button "Menu" is expanded
    When I press Tab
    And I press Enter
    Then focus is on the heading "Operations"
    And the button "Menu" is collapsed
    And the pages' links are hidden
    And the page has no accessibility violations

  Scenario: Choosing the current page from the menu closes it too
    Given a viewport of 390 by 844 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    When I press Shift+Tab until focus is on the button "Menu"
    And I press Enter
    Then the button "Menu" is expanded
    When I press Tab until focus is on the link "Repositories"
    And I press Enter
    Then the button "Menu" is collapsed
    And the pages' links are hidden
    And the page has no accessibility violations

  Scenario: Signing out from the menu and in again finds the menu closed, whatever the Menu button showed
    Given a viewport of 390 by 844 pixels
    And a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    When I press Shift+Tab until focus is on the button "Menu"
    And I press Enter
    Then the button "Menu" is expanded
    When I press Tab until focus is on the button "Sign out"
    And I press Enter
    Then the main heading reads "Sign in to Ladderback"
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Enter
    Then focus is on the heading "Repositories"
    And the button "Menu" is collapsed
    And the pages' links are hidden
    And the page has no accessibility violations

  # The header's links are the shared link (§4.5) standing alone, in the
  # color of the text around them, the product's name and the pages' links
  # alike: with nothing around them to be told from, they show no underline
  # until the pointer is on one (#1452, decision 152). The current page is
  # told by its weight and a bar at its start, and the page's own action is
  # the first Ladder Blue on the screen (decision 141). A page of the
  # navigation is current on its own path and on every page below it, as a
  # snapshot's page is below the Snapshots page.
  Scenario Outline: The product's name and the pages' links stand without an underline, in the color of the text around them, and the page the reader is in is marked bold with a bar, in <colors>
    Given a viewport of 1280 by 720 pixels
    And the browser asks for "<colors>"
    When I open the address "/snapshots/demo/3f2a"
    Then no link in the banner is underlined, 6 in all
    And each link in the banner is drawn in its tone:
      | link            | color              |
      | Ladderback      | the text around it |
      | Operations      | the text around it |
      | Plans           | the text around it |
      | Snapshots       | the text around it |
      | Repositories    | the text around it |
      | Pending changes | the text around it |
    And the link "Snapshots" is the current page, drawn bold
    And the link "Snapshots" is marked by a bar at its inline start, in the text's color
    And the link "Ladderback" is not the current page, drawn in the weight of the text around it
    When I rest the pointer on the link "Plans"
    Then the link "Plans" is underlined below its letters
    And the page has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors the product's name and the pages' links take the system's link color, show no underline at rest and keep the current page's bar
    Given a viewport of 1280 by 720 pixels
    And the browser asks for "forced colors"
    When I open the address "/operations"
    Then every link in the banner is drawn in the system's link color, 6 in all
    And no link in the banner is underlined, 6 in all
    And the link "Operations" is the current page, drawn bold
    And the link "Operations" is marked by a bar at its inline start, in the text's color
