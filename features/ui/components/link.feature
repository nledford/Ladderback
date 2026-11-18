@ui
Feature: The link
  A link, always underlined, which goes to a page of the app without
  reloading, says in words when it goes to another site and marks the page the
  reader is on.

  Scenario: Enter on a link to a page of the app goes there without reloading, titles the page and moves focus to its heading
    When I open the component gallery
    And I skip to the content
    And the page carries a mark that a reload would lose
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the link "Demo link to the start page"
    And I press Enter
    Then focus is on the main heading "Welcome to Ladderback"
    And the page is titled "Ladderback"
    And the page still carries its mark
    And the live regions stay silent

  Scenario: A link to the page the reader is on says it is the current page and is bold, and a link elsewhere is neither
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the link "Demo link to this page"
    Then the link "Demo link to this page" is the current page, drawn bold
    And the link "Demo link to the start page" is not the current page, drawn in the weight of the text around it

  Scenario: A link to another site says so in its name after its words, shows an icon after them that a screen reader skips, is marked as leaving the app, and tells that site nothing of the page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the link "Demo rustic documentation (goes to another site)"
    Then the link "Demo rustic documentation (goes to another site)" shows the words "Demo rustic documentation", then an icon a screen reader skips
    And the link "Demo rustic documentation (goes to another site)" is marked external, sends no referrer and opens where the reader is
    And the link "Demo rustic documentation, right to left (goes to another site)" shows the words "Demo rustic documentation, right to left", then an icon a screen reader skips

  Scenario Outline: A link is underlined without a pointer over it, below its letters, in Ladder Blue or the color of the text around it
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then every link of the "Link" demo is underlined below its letters
    And each link of the "Link" demo is drawn in its tone:
      | link                                                            | color              |
      | Demo link to the start page                                     | --accent           |
      | Demo link to this page                                          | --accent           |
      | Demo rustic documentation (goes to another site)                | --accent           |
      | the demo plan format                                            | --accent           |
      | the demo log                                                    | the text around it |
      | Demo rustic documentation, right to left (goes to another site) | --accent           |
    And the "Link" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a link takes the system's link color and keeps its underline
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every link of the "Link" demo is drawn in the system's link color
    And every link of the "Link" demo is underlined below its letters

  Scenario Outline: Under the pointer a link's underline is thicker than at rest
    Given the browser asks for "<colors>"
    When I open the component gallery
    And I rest the pointer on the link "Demo link to the start page"
    Then the underline of the link "Demo link to the start page" is thicker than at rest

    Examples:
      | colors        |
      | light colors  |
      | forced colors |

  Scenario: On a small screen with larger text, a long link wraps onto a second line inside the page
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then the link "Demo rustic documentation (goes to another site)" wraps onto more than one line
    And the page doesn't scroll sideways
    And no text is clipped
