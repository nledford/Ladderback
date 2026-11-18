@ui
Feature: The checkbox and the radio group drawn from the tokens
  A checkbox and a radio are drawn alike, each box edged in Field Edge and
  filled in Ladder Blue under a mark once chosen, in the reader's field colors
  under forced colors, and growing with the text. Their behavior is in
  checkbox.feature and radio_group.feature.

  Scenario Outline: A checkbox and a radio are drawn from the tokens, edged in Field Edge and filled in Ladder Blue under a mark in the page's color once chosen, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the checkbox "Send a demo notification" is edged 1 pixel wide in --field-edge
    And the checkbox "Send a demo notification" shows no mark
    And the checkbox "Keep the demo log" is filled in --accent, its mark in --bg
    And the radio "Every day" is edged 1 pixel wide in --field-edge
    And the radio "Every day" shows no mark
    And the radio "Every week" is filled in --accent, its mark in --bg
    And the "Checkbox" demo has no accessibility violations
    And the "Radio group" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a checkbox and a radio take the reader's field colors, a chosen one's mark too, and an invalid one keeps its thicker edge
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then the checkbox "Send a demo notification" is edged 1 pixel wide in FieldText
    And the checkbox "Keep the demo log" is filled in Field, its mark in FieldText
    And the radio "Every week" is filled in Field, its mark in FieldText
    And the checkbox "Accept the demo terms" is edged 2 pixels wide

  Scenario: On a small screen with larger text, a checkbox's and a radio's box grow with the text, never under 24 pixels a side, the mark centered in them
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then the checkbox "Keep the demo log" is a square 1.5 times its text's size, at least 24 pixels a side, its mark centered in it
    And the radio "Every week" is a circle 1.5 times its text's size, at least 24 pixels a side, its mark centered in it
    And the page doesn't scroll sideways
