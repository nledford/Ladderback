@ui
Feature: The card
  The card (technical design §4.5; DESIGN.md, The Summary Card Rule) is the
  shared frame of a summary of one thing that stands on its own: an article
  named by its title, a heading at the level its caller gives, which takes
  focus from a script alone; then its body; then, where it has any, its
  actions, a row of controls at its foot and its inline end, which wraps
  where the card is narrow. It is never a control: never a link, a button or
  a stop for Tab, so a keyboard user passes only through its actions. It is
  flat: the page's color inside a 1px edge in Field Edge with the small
  corner, no stripe and no shadow. It takes no tone and no size.

  Scenario: A card is an article named by its title, which takes focus from a script alone
    When I open the "Card" demo
    Then the demo's card "Demo summary" is an article named by its heading at level 3
    And the demo's card "Demo summary with controls" is an article named by its heading at level 3
    And the heading of the demo's card "Demo summary" takes focus from a script

  Scenario: A card without actions holds nothing that takes focus
    When I open the "Card" demo
    Then the demo's card "Demo summary" is no control and takes no focus
    And the demo's card "Demo summary" holds nothing that takes focus

  Scenario: Tab passes through a card's actions only, in their order
    When I open the "Card" demo
    Then the demo's card "Demo summary with controls" is no control and takes no focus
    When I press Tab
    Then focus is on the button "Act on the demo summary with controls"
    When I press Tab
    Then focus is on the link "Open the demo summary with controls"
    When I press Tab
    Then focus has left the "Card" demo

  Scenario Outline: A card's actions stand at its foot and its inline end, inside it, at <width> pixels with text at <size> percent
    Given a viewport of <width> by 800 pixels
    And the reader sets the text size to <size> percent
    When I open the "Card" demo
    Then the actions of the demo's card "Demo summary with controls" stand at its foot and its inline end, each inside it
    And the page doesn't scroll sideways

    Examples:
      | width | size |
      | 320   | 200  |
      | 1280  | 100  |

  Scenario Outline: A card is drawn flat, the page's color inside a 1px Field Edge with the small corner and no shadow, in <colors>
    Given the browser asks for "<colors>"
    When I open the "Card" demo
    Then each card of the "Card" demo is drawn on the page's background inside a 1px edge in "--field-edge" with the corner "--radius-small", and no shadow
    And the page has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |
