@ui
Feature: The shared vocabulary
  The vocabulary every component takes: its tones, each named in words and
  drawn in a color of its own, a pair of colors for each status tone, its
  words on its fill, its sizes and its placements, which follow the page's
  direction.

  Scenario: Each tone is named in words, and draws its text and its swatch in a color of its own, which a screen reader skips
    When I open the component gallery
    Then each of the 6 tones of the "Shared vocabulary" demo is named in words, its swatch drawn in its text's color and skipped by a screen reader
    And the 5 tones of the "Shared vocabulary" demo other than Inherit each have a color of their own
    And the tone "Inherit" of the "Shared vocabulary" demo takes the color of the text around it

  Scenario: Under forced colors every tone draws its text and its swatch in the system's text color, so its words tell it apart
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then each of the 6 tones of the "Shared vocabulary" demo is named in words, its swatch drawn in its text's color and skipped by a screen reader
    And the 6 tones of the "Shared vocabulary" demo all draw in the color of the text around them

  Scenario Outline: Each status tone has a pair of colors of its own, its words on its fill, named in words, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then each status pair of the "Shared vocabulary" demo is named in words, drawn on its fill:
      | pair    | words        | fill         |
      | Success | --success-fg | --success-bg |
      | Warning | --warning-fg | --warning-bg |
      | Error   | --error-fg   | --error-bg   |

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario Outline: Start, Center and End place a part at its line's start, middle and end, in the page's direction
    When I open the component gallery asking for the locale "<locale>"
    Then the placements of the vocabulary demo put each part where they say, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |
