@ui
Feature: The button
  A button, pressed by Enter and Space, drawn in an appearance and a tone
  named in words, as tall as a multiple of the text, which stays in the Tab
  order and ignores presses while it is unavailable or busy, keeps its name
  while a spinner stands in it, and says it is pressed by its state and by its
  weight and edge.

  Scenario: A button is pressed by Enter and by Space
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Enter
    Then the "Button" demo shows the sentence "The demo buttons were pressed once."
    When I press Space
    Then the "Button" demo shows the sentence "The demo buttons were pressed 2 times."

  Scenario: A button draws its appearance in its tone, both named in words, Outlined in Secondary unless its page names others
    When I open the component gallery
    Then each button of the "Button" demo draws its appearance in its tone:
      | button                         | label              | fill     | edge               |
      | Filled Primary                 | --on-fill          | --accent | --accent           |
      | Filled Secondary               | --on-fill          | --muted  | --muted            |
      | Filled Success                 | --on-fill          | --success | --success         |
      | Filled Warning                 | --on-fill          | --warning | --warning         |
      | Filled Error                   | --on-fill          | --error  | --error            |
      | Outlined Primary               | --accent           | none     | --accent           |
      | Outlined Secondary             | --muted            | none     | --muted            |
      | Outlined Success               | --success          | none     | --success          |
      | Outlined Warning               | --warning          | none     | --warning          |
      | Outlined Error                 | --error            | none     | --error            |
      | Outlined Inherit               | the text around it | none     | the text around it |
      | Ghost Primary                  | --accent           | none     | none               |
      | Ghost Secondary                | --muted            | none     | none               |
      | Ghost Success                  | --success          | none     | none               |
      | Ghost Warning                  | --warning          | none     | none               |
      | Ghost Error                    | --error            | none     | none               |
      | Ghost Inherit                  | the text around it | none     | none               |
      | Demo button at the Medium size | --muted            | none     | --muted            |

  Scenario Outline: A button is a multiple of the reader's text size tall, Medium unless its page names another
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the button "<button>" is <times> times as tall as the page's text

    Examples:
      | button                              | times |
      | Demo button at the Small size       | 1.75  |
      | Demo button at the Medium size      | 2     |
      | Demo button at the Large size       | 2.5   |
      | Demo button at the Extra large size | 3     |
      | Demo details at the Small size      | 1.75  |
      | Demo details at the Extra large size | 3    |

  Scenario Outline: An icon-only button is named by its label, and stays square and at least 24 pixels a side at every size
    When I open the component gallery
    Then the button "<button>" shows only an icon, which a screen reader skips
    And the button "<button>" is square, at least 24 pixels a side

    Examples:
      | button                               |
      | Demo details at the Small size       |
      | Demo details at the Medium size      |
      | Demo details at the Large size       |
      | Demo details at the Extra large size |

  Scenario Outline: A button's icon stands before its label, as tall as its text, in the page's direction
    When I open the component gallery asking for the locale "<locale>"
    Then every button with an icon and words shows its icon before its words, as tall as them, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: An unavailable button stays in the Tab order, says it is unavailable and ignores Enter and Space
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Delete the demo snapshot"
    Then the button "Delete the demo snapshot" says it is unavailable, and is not disabled
    When I press Enter
    And I press Space
    Then focus stays on the button "Delete the demo snapshot"
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."

  Scenario: A busy button keeps its name and its place in the Tab order, shows a spinner a screen reader skips, and ignores presses
    When I open the component gallery
    And the widths of the buttons "Check the demo repository" and "Refresh the demo" are noted
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then the button "Demo work in progress" is pressed
    And the button "Save the demo plan" is busy, a spinner a screen reader skips before its label
    And the button "Check the demo repository" is busy, a spinner a screen reader skips in its icon's place
    And the button "Refresh the demo" is busy, a spinner a screen reader skips in its icon's place
    And the buttons "Check the demo repository" and "Refresh the demo" keep the widths noted
    And the "Button" demo has no accessibility violations
    When I press Tab until focus is on the button "Check the demo repository"
    And I press Enter
    And I press Space
    Then focus stays on the button "Check the demo repository"
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."
    When I press Shift+Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then the button "Check the demo repository" is not busy, its icon back in its place

  Scenario: A toggle button keeps its one name and says it is pressed, drawn bolder with an inset edge
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo's hidden files"
    Then the button "Show the demo's hidden files" is not pressed
    And the button "Show the demo's hidden files" is drawn at its resting weight and edge
    When I press Space
    Then the button "Show the demo's hidden files" is pressed
    And the button "Show the demo's hidden files" is drawn bolder, with an inset edge
    And the "Button" demo has no accessibility violations
    When I press Enter
    Then the button "Show the demo's hidden files" is not pressed
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."

  Scenario: A submit button sends its form from the keyboard
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Enter
    Then the polite region comes to contain "Demo form sent."
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."

  Scenario: A busy submit button sends no form, from Enter or from Space
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Space
    And I press Tab until focus is on the button "Send the demo form"
    Then the button "Send the demo form" is busy, a spinner a screen reader skips before its label
    When I press Enter
    And I press Space
    Then focus stays on the button "Send the demo form"
    And the polite region stays empty

  Scenario: Under forced colors a button takes the reader's button colors, an unavailable one the color of unavailable text, and pressed keeps its weight and edge
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every button of the "Button" demo is drawn in the reader's button colors, an unavailable one in the color of unavailable text
    When I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo's hidden files"
    And I press Space
    Then the button "Show the demo's hidden files" is drawn bolder, with an inset edge
    When I press Shift+Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then the button "Demo work in progress" is drawn bolder, with an inset edge
    And every pressed button of the "Button" demo is drawn in the reader's button colors, a Ghost one's edge too

  Scenario: Under forced colors a busy button keeps the reader's button text, apart from an unavailable one
    Given the browser asks for "forced colors"
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then every busy button of the "Button" demo is drawn in the reader's button colors, its label and edge in the button text, not the color of unavailable text
