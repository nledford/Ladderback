@ui
Feature: The badge
  A badge, a count or a tag on a pill of its tone's colors, which says what it
  counts or names in words, never by its color alone, its icon a decoration a
  screen reader skips, and which Tab never stops on.

  Scenario: A badge says what it counts or names in words, with no role, its icon a decoration a screen reader skips, and Tab never stops on it
    When I open the component gallery
    Then every badge of the "Badge" demo says its words with no role and nothing in it to focus, any icon in it a decoration a screen reader skips
    When I skip to the content
    And I press Tab until focus is on the radio "Mornings"
    And I press Tab
    Then focus has passed the "Badge" demo without stopping in it

  Scenario Outline: Each tone of a badge is named in words and drawn on its own pair of tokens, its icon in its words' color, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the badges of the "Badge" demo named for their tones are drawn on their pairs of tokens, at least 4.5:1 apart:
      | badge     | words              | fill               |
      | Primary   | --badge-loading-fg | --badge-loading-bg |
      | Secondary | --badge-empty-fg   | --badge-empty-bg   |
      | Success   | --success-fg       | --success-bg       |
      | Warning   | --warning-fg       | --warning-bg       |
      | Error     | --error-fg         | --error-bg         |

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a badge's words and icon take the system's text color, edged 1 pixel wide in it, so its words tell the tones apart and each badge stands apart from the next
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every badge of the "Badge" demo draws its words and icon in the color of the text around it, edged 1 pixel wide in that color

  Scenario Outline: A badge's icon stands before its words, centered on their line and as tall as the text, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    Then the icon of each badge of the badge demo stands before its words, centered on their line and as tall as the text, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, a badge's words wrap within the room it is given
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then every badge of the "Badge" demo keeps within the room it is given
    And the page doesn't scroll sideways

  # The words wrap between words beside the icon (#1070): a badge too long
  # for its room takes more lines after its icon, which keeps to the start of
  # the first, never a line of its own above the words.
  Scenario: On a small screen, a long badge's words wrap beside its icon
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 100 percent
    When I open the component gallery
    Then the badge "Kept by the demo plan's retention rules" of the "Badge" demo wraps its words beside its icon, which stands at the start of its first line
