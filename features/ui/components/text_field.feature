@ui
Feature: The text field
  A text field, its label, description and error above it and wired to it, its
  error announced and drawn in a thicker edge in the Error tone, which takes
  one line, many lines, a password it can show, a number or a code, says in
  words that it is required, read only or unavailable, and is as tall as a
  button of its size.

  Scenario: A text field's error is shown above it, describes it, marks it invalid with a thicker edge in the Error tone, and is announced
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Enter
    Then the field "Repository name" shows the error "A repository name is required."
    And the field "Repository name" is described by "A repository name is required."
    And the field "Repository name" is marked invalid
    And the field "Repository name" is edged 2 pixels wide in --error
    And the polite region reads "A repository name is required."
    When I press Enter
    Then the field "Repository name" shows no error
    And the field "Repository name" is described only by "The name of the repository to check."
    And the field "Repository name" is not marked invalid
    And the field "Repository name" is edged 1 pixel wide in --field-edge

  Scenario: A text field with no known purpose turns the browser's autocomplete off
    When I open the component gallery
    Then the textbox "Demo host" has the autocomplete token "off"

  Scenario: A field already invalid shows its label, its description and its error above it, each describing it
    When I open the component gallery
    Then the field "Demo host" shows its label, its description and its error above it, in that order
    And the field "Demo host" is described by "The demo machine's host name."
    And the field "Demo host" is described by "Enter a host name, such as"
    And the field "Demo host" is marked invalid
    And the field "Demo host" is edged 2 pixels wide in --error

  Scenario: A multi-line text field takes an entry a line and shows its error as a one-line field does
    When I open the "Text field" demo
    And I press Tab until focus is on the textbox "Demo folders"
    And I type "/srv/demo/photos"
    And I press Enter
    And I type "/srv/demo/notes"
    Then the textbox "Demo folders" holds the lines "/srv/demo/photos" and "/srv/demo/notes"
    And the field "Demo folders" is described only by "One folder or file a line."
    And the field "Demo folders" is edged 1 pixel wide in --field-edge
    When I press Tab until focus is on the button "Check the demo folders"
    And I press Enter
    Then the field "Demo folders" shows the error "Enter at least one folder."
    And the field "Demo folders" is marked invalid
    And the field "Demo folders" is edged 2 pixels wide in --error
    And the polite region reads "Enter at least one folder."

  Scenario: A password's Show password button shows it and hides it again, keeping its value, its caret and focus, and says it is pressed
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Tab until focus is on the textbox "Demo password"
    And I type "demo-secret"
    And I press ArrowLeft
    Then the field "Demo password" hides what it holds
    When I press Tab
    Then focus is on the button "Show password"
    And the button "Show password" is not pressed
    And the button "Show password" is described by "Demo password"
    When I press Space
    Then focus is on the button "Show password"
    And the button "Show password" is pressed
    And the field "Demo password" shows what it holds, "demo-secret", its caret 1 character before its end
    And the textbox "Demo password" has the autocomplete token "current-password"
    And the "Text field" demo has no accessibility violations
    When I press Enter
    Then focus is on the button "Show password"
    And the button "Show password" is not pressed
    And the field "Demo password" hides what it holds
    And the textbox "Demo password" still holds "demo-secret"

  Scenario: A number field asks for the keyboard of digits, is never a spin box, and takes what is typed for its form to check
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Tab until focus is on the textbox "Demo copies to keep"
    Then the textbox "Demo copies to keep" asks for the keyboard of digits, and is a text input
    When I type "1a2"
    Then the textbox "Demo copies to keep" still holds "1a2"

  Scenario: A read-only field stays in the Tab order behind a dashed edge, a disabled one is skipped with its Show password button, and each says so in words
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Tab until focus is on the textbox "Demo snapshot ID"
    Then the field "Demo snapshot ID" is described by "Read only: you can select and copy it, but not change it."
    And the field "Demo snapshot ID" is edged with dashes
    When I type "9"
    Then the textbox "Demo snapshot ID" still holds "1a2b3c4d"
    When I press Tab
    Then focus is on the textbox "Demo tag at the Small size"
    And the field "Demo repository location" is described by "Unavailable: it can't be changed now."
    And the field "Demo repository location" is disabled
    And the field "Demo keyring password" is described by "Unavailable: it can't be changed now."
    And the field "Demo keyring password" is disabled

  Scenario: A required field says so in words after its label, which a screen reader skips, and says it once itself
    When I open the component gallery
    Then the field "Demo plan name" shows "(required)" after its label, which a screen reader skips
    And the field "Demo plan name" is required, its name its label alone

  Scenario Outline: A text field is as tall as a button of its size, and the two stand level in a row of actions
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the field "<field>" is <times> times as tall as the page's text
    And the field "<field>" is as tall as the button "<button>" beside it, their feet level

    Examples:
      | field                            | button                                   | times |
      | Demo tag at the Small size       | Add the demo tag at the Small size       | 1.75  |
      | Demo tag at the Medium size      | Add the demo tag at the Medium size      | 2     |
      | Demo tag at the Large size       | Add the demo tag at the Large size       | 2.5   |
      | Demo tag at the Extra large size | Add the demo tag at the Extra large size | 3     |

  Scenario Outline: A text field's icon stands at its start, inside its edge, in the page's direction, and a screen reader skips it
    When I open the component gallery asking for the locale "<locale>"
    Then the field with an icon shows it at its start, inside its edge, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario Outline: A text field's edge is drawn in its own token, in light colors and in dark
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the field "Repository name" is edged 1 pixel wide in --field-edge
    And the field "Demo snapshot ID" is edged with dashes
    And the "Text field" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a text field takes the reader's field colors, a disabled one the color of unavailable text, and invalid and read only keep their edges
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every field of the "Text field" demo is drawn in the reader's field colors, a disabled one in the color of unavailable text
    And the field "Demo host" is edged 2 pixels wide
    And the field "Demo snapshot ID" is edged with dashes
