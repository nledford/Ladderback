@ui
Feature: The radio group
  A radio group is one stop for Tab, chosen within by the arrow keys, marks a
  choice's label written in another language with that language and its
  direction, and shows its error under its legend. How a radio is drawn,
  beside a checkbox, is in choices.feature.

  Scenario: A radio group is one stop for Tab, which lands on the choice made, and the arrow keys choose within it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the radio "Every week"
    Then the radio "Every week" of the group "Demo check frequency" is checked
    And the group "Demo check frequency" is described by "A demo choice: choosing one changes nothing."
    When I press the down arrow until focus is on the radio "Every month"
    Then the radio "Every month" of the group "Demo check frequency" is checked
    And the radio "Every week" of the group "Demo check frequency" is not checked
    When I press Tab
    Then focus has left the group "Demo check frequency"
    When I press Shift+Tab
    Then focus is on the radio "Every month"

  Scenario Outline: A radio group marks a choice's label written in another language with that language and the way it runs
    When I open the component gallery
    Then the label of the radio "<name>" is in the language "<language>", running <direction>

    Examples:
      | name    | language | direction |
      | English | en-US    | ltr       |
      | 日本語     | ja       | ltr       |
      | العربية | ar       | rtl       |

  Scenario: A radio group's error stands under its legend, describes the group, and marks the group invalid, each choice edged thicker in the Error tone
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the radio "Every week"
    And I press Tab until focus is on the radio "Mornings"
    Then the group "Demo backup window" is described by "Choose a demo backup window."
    And the group "Demo backup window" shows its legend, its error and its choices, in that order
    And the radio group "Demo backup window" is marked invalid, and its radios are not
    And the radio "Mornings" is edged 2 pixels wide in --error
    And the radio "Mornings" of the group "Demo backup window" is not checked
    When I press Space
    Then the radio "Mornings" of the group "Demo backup window" is checked
