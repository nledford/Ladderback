@ui
Feature: The machine card
  The machine card (technical design §4.5; §16, decision 175) is one machine
  at a glance on the hub's dashboard: an article named by its heading, the
  machine's hostname, which marks the hub's own card "This hub" in words. A
  card whose machine needs attention says so first, in words after an icon
  hidden from a screen reader; never by color alone, and never by a colored
  edge or fill. Then whether the machine is online, its status in a status
  badge, the progress of the operation it runs in the shared progress bar,
  which is never announced, and its facts in a description list. Its
  controls are the caller's, after its facts: they and a running machine's
  progress bar are its stops for Tab, and the card itself is never one, nor
  a control. It is flat: the page's color in a
  1px edge in Field Edge with the small corner, and no shadow (the Flat
  Ledger Rule). Cards stand in a grid that wraps by its own width: one
  column at 320 px with 200 % text, several side by side on a wide page.

  Scenario: A card is an article named by its heading, and the card itself is never a stop for Tab
    When I open the "Machine card" demo
    And I press Tab
    Then focus is on the button "Back up demo-hub"
    When I press Tab
    Then focus is on the link "Open demo-hub"
    When I press Tab
    Then focus is on the link "Open demo-laptop"
    And the article "demo-hub This hub" holds the heading "demo-hub This hub" at level 3
    And the article "demo-laptop" tells its facts in a description list

  Scenario: The attention mark says why in words, after an icon a screen reader doesn't hear
    When I open the "Machine card" demo
    Then the article "demo-laptop" starts with the words "Needs attention: The last run of photos failed."
    And the attention mark of the article "demo-laptop" has an icon hidden from a screen reader
    And the article "demo-hub This hub" has no attention mark

  Scenario: The progress bar of a running machine's card is never announced
    When I open the "Machine card" demo
    Then the progress bar "Progress of the operation running on demo-desktop" shows 25 percent
    And the page has no live region but the announcer's

  Scenario Outline: A card is flat, in the page's color inside a 1px Field Edge, with no shadow, in <colors>
    Given the browser asks for "<colors>"
    When I open the "Machine card" demo
    Then each card of the demo is drawn on the page's background inside a 1px edge in "--field-edge", with no shadow
    And the page has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: At 320 pixels with text at 200 percent the demo's cards stand in one column inside the page
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the "Machine card" demo
    Then the demo's cards stand in one column, each inside the page
    And the page doesn't scroll sideways

  Scenario: On a wide page the demo's cards stand side by side
    Given a viewport of 1280 by 800 pixels
    When I open the "Machine card" demo
    Then the demo's cards stand side by side
