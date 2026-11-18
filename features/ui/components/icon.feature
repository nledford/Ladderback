@ui
Feature: The icon
  An icon, decoration a screen reader skips unless a label names it, as tall
  as the text and in the text's color unless its page names a size or a tone,
  sharing the spinner's sizes.

  Scenario: An icon beside words is decoration a screen reader skips, in the words' color, as tall as them and centered on them
    When I open the component gallery
    Then the "Icon" demo shows the text "The next demo backup starts at 02:00 every night." beside an icon a screen reader skips, in the text's color, as tall as it and centered on it

  Scenario: An icon-only button is named by its own label, and the icon inside it is skipped by a screen reader
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Demo notifications"
    Then the button "Demo notifications" shows only an icon, which a screen reader skips
    And the button "Demo notifications" is not pressed
    And the button "Demo notifications" shows a crossed-out bell
    When I press Space
    Then the button "Demo notifications" is pressed
    And the button "Demo notifications" shows only an icon, which a screen reader skips
    And the button "Demo notifications" shows a bell, not crossed out
    When I press Space
    Then the button "Demo notifications" is not pressed
    And the button "Demo notifications" shows a crossed-out bell

  Scenario: An icon given a label is an image named by it
    When I open the component gallery
    Then the "Icon" demo has the image "Demo icon at the Medium size"
    And the "Icon" demo shows no text "Demo icon at the Medium size"

  Scenario Outline: An icon's size is a multiple of the reader's text size, Small unless its page names another, as a spinner's of the same size is
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the image "<icon>" is <times> times as tall as the page's text
    And the image "<icon>" is as tall as the image "<spinner>"

    Examples:
      | icon                              | spinner                              | times |
      | Demo icon at the Small size       | Demo spinner at the Small size       | 1     |
      | Demo icon at the Medium size      | Demo spinner at the Medium size      | 1.5   |
      | Demo icon at the Large size       | Demo spinner at the Large size       | 2     |
      | Demo icon at the Extra large size | Demo spinner at the Extra large size | 3     |

  Scenario: An icon's tone colors its glyph, named in words beside it, and an icon in no tone takes the color of the text around it
    When I open the component gallery
    Then each tone of the "Icon" demo, named in words, draws its icon in it:
      | tone      | color              |
      | Primary   | --accent           |
      | Secondary | --muted            |
      | Success   | --success          |
      | Warning   | --warning          |
      | Error     | --error            |
      | Inherit   | the text around it |

  Scenario: Under forced colors every icon is drawn in the color of the text around it, as every tone is
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every icon of the "Icon" demo is drawn in the color of the text around it
