@ui
Feature: The alert
  An alert, an inline message about a page's state whose icon names its tone
  in words, which is no live region and no stop for Tab, and whose Dismiss
  button takes it away and leaves focus where its page names, its words
  wrapping whole inside it on a small screen.

  Scenario: An alert is no live region and no stop for Tab, and its icon names its tone in words
    When I open the component gallery
    Then no alert of the "Alert" demo is a live region, nor holds one
    And the alerts of the "Alert" demo show these icons, each named for its tone, one shape per tone, in this order:
      | tone    | name        |
      | primary | Information |
      | success | Success     |
      | warning | Warning     |
      | error   | Error       |
      | primary | Information |
    When I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the link "the demo operations"
    And I press Tab
    Then focus is on the button "Dismiss" of the "Alert" demo

  Scenario Outline: <key> on an alert's Dismiss button takes the alert away, says nothing and leaves focus on the control its page names
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the button "Dismiss" of the "Alert" demo
    And I press <key>
    Then the "Alert" demo shows no alert with the words "This demo notice goes when you dismiss it."
    And focus is on the button "Show the demo notice again"
    And the polite region stays empty
    And the assertive region stays empty
    When I press Enter
    Then the "Alert" demo shows an alert with the words "This demo notice goes when you dismiss it."
    And focus stays on the button "Show the demo notice again"

    Examples:
      | key   |
      | Enter |
      | Space |

  Scenario Outline: An alert lies on Ledger Gray, its words in the text's color and its icon and start edge in its tone, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then every alert of the "Alert" demo lies on --surface with its words in --fg, its icon and its 0.25 rem start edge in its tone's color

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors an alert is edged all round in the text's color, its start edge thicker, and its icon in the text's color
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every alert of the "Alert" demo is edged 1 pixel wide in CanvasText, its start edge 0.25 rem, and its icon in the text's color

  Scenario Outline: An alert's edge and icon stand at the start of its line and its Dismiss button at the end, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    Then every alert of the alert demo has its edge on its start side, its icon before its words and its Dismiss button after them, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, an alert's words wrap inside it, its icon and Dismiss button inside it too
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then every alert of the "Alert" demo holds its icon, its words and its Dismiss button inside its edge
    And the page doesn't scroll sideways
    And no text is clipped

  # A long path breaks beside the icon (#1071), rather than moving the words
  # under it, as the path's whole width once did.
  Scenario Outline: On a small screen with larger text, an alert's words stay beside its icon, a long path's too, in a page that runs <direction>
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery asking for the locale "<locale>"
    Then every alert of the alert demo keeps its icon at the start of its words' first line, each line of its words after it

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  # A word that fits on a line of its own wraps whole, never "Ladderbac" over
  # "k"; only a word longer than the line, as the error's long path is, may
  # break inside. Dismiss keeps to the end of its line, a line of its own if
  # it doesn't fit beside the words (#972).
  Scenario Outline: On a small screen with larger text, an alert's words wrap only between words and its Dismiss button keeps to the end, in a page that runs <direction>
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery asking for the locale "<locale>"
    Then every alert of the alert demo breaks its words only between them, unless a word is longer than its line
    And the Dismiss button of the alert demo stands at the end of its line, in a page that runs <direction>
    And the page doesn't scroll sideways
    And no text is clipped

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |
