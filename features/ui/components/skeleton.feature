@ui
Feature: The skeleton
  A skeleton, the still shape of content on its way, which a screen reader and
  Tab both skip, beside the words that say the wait in a region that is busy
  until the content takes its place, as tall as it was.

  Scenario: A skeleton is hidden from a screen reader, has no role and is never a stop for Tab
    When I open the component gallery
    Then every skeleton of the "Skeleton" demo is hidden from a screen reader, with no role and nothing inside it to focus
    When I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab
    Then focus is on the button "Show the demo plan"

  Scenario: A region says its wait in words beside its skeleton while it is busy, and once the content comes it is no longer busy, keeps its height and leaves focus where it was
    When I open the component gallery
    Then the region of the "Skeleton" demo is busy and shows the words "Loading the demo plan…" beside a skeleton
    When I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And the height of the region of the "Skeleton" demo is noted
    And I press Enter
    Then the region of the "Skeleton" demo is no longer busy and shows "Backs up the demo folder." in place of its skeleton
    And the region of the "Skeleton" demo is as tall as it was, within 2 pixels
    And focus stays on the button "Show the demo plan"
    And the polite region stays empty
    And the assertive region stays empty

  Scenario: A skeleton never moves, whether or not the reader asks for less motion
    When I open the component gallery
    Then no skeleton of the "Skeleton" demo moves, fades or changes
    When the reader asks for less motion
    Then no skeleton of the "Skeleton" demo moves, fades or changes

  Scenario Outline: A skeleton's lines fill their room, each as tall as the text on a line of its own, the last one shorter, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    Then the skeleton lines of the skeleton demo fill their room, each 1 em tall on a line of the text, the last one shorter and at the line's start, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, a skeleton's block grows with the text and keeps to the room it is given
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then the skeleton block of the "Skeleton" demo is 2.5 rem tall and 20 rem wide, or as wide as its room where that is less
    And the page doesn't scroll sideways

  Scenario Outline: A skeleton is Pencil Gray, which shows on the page and on Ledger Gray alike, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then every skeleton of the "Skeleton" demo is filled in --skeleton, at least 1.4:1 apart from what lies behind it, on the page and on --surface

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a skeleton is edged 1 pixel wide in the color of unavailable text, so its shape still shows
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every skeleton of the "Skeleton" demo is edged 1 pixel wide in GrayText
