@ui
Feature: The checkbox
  A checkbox turns on and off by keyboard, is described by its hint and shows
  its error under its description. How a checkbox is drawn, beside a radio,
  is in choices.feature.

  Scenario: A checkbox turns on and off by keyboard and is described by its hint
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the checkbox "Send a demo notification"
    Then the checkbox "Send a demo notification" is not checked
    And the checkbox "Send a demo notification" is described by "A demo choice: turning it on changes nothing."
    When I press Space
    Then the checkbox "Send a demo notification" is checked
    When I press Space
    Then the checkbox "Send a demo notification" is not checked

  Scenario: A checkbox's error stands under its description, describes it, and marks it invalid with a thicker edge in the Error tone
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the checkbox "Accept the demo terms"
    Then the checkbox "Accept the demo terms" stands before its label, its description and its error under the label, in that order
    And the checkbox "Accept the demo terms" is described by "The demo terms change nothing. Accept the demo terms to go on."
    And the checkbox "Accept the demo terms" is marked invalid
    And the checkbox "Accept the demo terms" is edged 2 pixels wide in --error
    When I press Space
    Then the checkbox "Accept the demo terms" is checked
