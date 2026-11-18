@ui
Feature: The banner
  A banner, a page-wide notice above a page's content, shows its tone by an
  icon named in words, is no live region and no stop for Tab, and its Dismiss
  button takes it away, leaves focus where its page names and is remembered in
  this browser until a newer edition of it comes.

  Scenario: A banner is no live region and no stop for Tab, and its icon names its tone in words
    When I open the component gallery
    Then no banner of the "Banner" demo is a live region, nor holds one
    And the banners of the "Banner" demo show these icons, each named for its tone, one shape per tone, in this order:
      | tone    | name        |
      | primary | Information |
      | warning | Warning     |
      | primary | Information |
    When I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab
    Then focus is on the button "Dismiss" of the "Banner" demo

  Scenario Outline: <key> on a banner's Dismiss button takes the banner away, says nothing, leaves focus on the control its page names and keeps it away on the next load
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the button "Dismiss" of the "Banner" demo
    And I press <key>
    Then the "Banner" demo shows no banner with the words "A demo update is ready."
    And focus is on the button "Show a newer demo notice"
    And the polite region stays empty
    And the assertive region stays empty
    When I reload the page
    Then the "Banner" demo shows 2 banners, none with the words "A demo update is ready."

    Examples:
      | key   |
      | Enter |
      | Space |

  Scenario: A newer edition of a dismissed banner shows again
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the button "Dismiss" of the "Banner" demo
    And I press Enter
    And I press Enter
    Then the "Banner" demo shows a banner with the words "A demo update is ready."
    And focus stays on the button "Show a newer demo notice"

  Scenario: A banner dismissed in a browser that keeps nothing goes for now and shows again on the next load
    Given this browser keeps nothing a page stores
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the button "Dismiss" of the "Banner" demo
    And I press Enter
    Then the "Banner" demo shows no banner with the words "A demo update is ready."
    And focus is on the button "Show a newer demo notice"
    When I reload the page
    Then the "Banner" demo shows a banner with the words "A demo update is ready."

  Scenario Outline: A banner lies on Ledger Gray, square, its words in the text's color and its icon and start edge in its tone, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then every banner of the "Banner" demo lies square on --surface with its words in --fg, its icon and its 0.25 rem start edge in its tone's color

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a banner is edged all round in the text's color, its start edge thicker, and its icon in the text's color
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every banner of the "Banner" demo is edged 1 pixel wide in CanvasText, its start edge 0.25 rem, and its icon in the text's color

  Scenario Outline: A banner's edge and icon stand at the start of its line and its Dismiss button at the end, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    Then every banner of the banner demo has its edge on its start side, its icon before its words and its Dismiss button after them, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, a banner's words wrap inside it, its icon and Dismiss button inside it too
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then every banner of the "Banner" demo holds its icon, its words and its Dismiss button inside its edge
    And the page doesn't scroll sideways
    And no text is clipped
