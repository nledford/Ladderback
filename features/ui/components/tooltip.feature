@ui
Feature: The tooltip
  A tooltip, a short note beside a control that keyboard focus shows at once
  and a resting pointer after a moment, which Escape dismisses, which stays
  while the pointer is over it, which stands on the side its control asks for
  where there is room and never covers the control, and which describes the
  control, or shows an icon-only control's name.

  Scenario: Keyboard focus on a control shows its tooltip at once, beside the control, which it describes
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." shows beside the button "Prune the demo repository", without covering it
    And the button "Prune the demo repository" is described by "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing."
    And the page has no accessibility violations
    When I press Shift+Tab
    Then no tooltip shows

  Scenario: Escape hides a tooltip and leaves focus on its control, which it still describes
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    And I press Escape
    Then no tooltip shows
    And focus stays on the button "Prune the demo repository"
    And the button "Prune the demo repository" is described by "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing."

  Scenario: A resting pointer shows a tooltip after a moment, which stays while the pointer moves onto it, until Escape
    When I open the component gallery
    And I rest the pointer on the button "Prune the demo repository"
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." shows only once the pointer has rested a moment
    When I move the pointer onto the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing."
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." still shows a moment later
    When I press Escape
    Then no tooltip shows

  Scenario: A tooltip goes once the pointer has left both its control and the tooltip
    When I open the component gallery
    And I rest the pointer on the button "Prune the demo repository"
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." shows only once the pointer has rested a moment
    When I move the pointer away from the button "Prune the demo repository" and its tooltip
    Then no tooltip shows

  Scenario Outline: A tooltip with room on the side its control asks for stands there, in the direction of the control's line
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "<button>"
    Then the tooltip of the button "<button>" stands <where> it

    Examples:
      | button                                | where           |
      | Demo note above                       | above           |
      | Demo note below                       | below           |
      | Demo note at the start                | to the left of  |
      | Demo note at the end                  | to the right of |
      | Demo note at the start, right to left | to the right of |
      | Demo note at the end, right to left   | to the left of  |

  Scenario: At the foot of the page, a tooltip asked to stand below its control has no room there and stands above it
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo note at the foot of the page"
    Then the tooltip of the button "Demo note at the foot of the page" stands above it

  Scenario: On a small screen with larger text, a tooltip with no room at either end of its control stands above or below it, inside the window
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo note at the end"
    Then the tooltip of the button "Demo note at the end" stands above or below it, inside the window
    When I press Shift+Tab until focus is on the button "Demo note at the start"
    Then the tooltip of the button "Demo note at the start" stands above or below it, inside the window

  Scenario: A long note wraps onto lines of its own, as wide as the window allows, never cut and never over its control
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo long note"
    Then the tooltip of the button "Demo long note" wraps onto lines within the window's width, none of its words cut, without covering the button

  Scenario: A tooltip's words are a step smaller than the page's and grow with the reader's text, wrapping at about 20 of their widths
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo long note"
    Then the tooltip of the button "Demo long note" is set a step smaller than the page's text and wraps at 20 times its text size

  Scenario: An icon-only control's tooltip shows the control's name, which a screen reader hears once, from the control
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo schedule"
    Then the button "Demo schedule" shows only an icon, which a screen reader skips
    And the tooltip of the button "Demo schedule" shows "Demo schedule", which a screen reader skips
    And the button "Demo schedule" has no description

  Scenario Outline: A tooltip is drawn in the page's colors swapped, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" is filled with the page's text color, its words in the page's background color, with no shadow

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a tooltip takes the system's colors and an edge in its text color
    Given the browser asks for "forced colors"
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" is drawn in the page's own colors, edged in its text color

  # A contract change (decision 127): the fade is a change of opacity, which
  # the reduced-motion rule keeps; until then the tooltip appeared at once.
  Scenario: A tooltip fades in quickly, and still fades for a reader who asks for less motion
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" fades in over 100 ms
    When the reader asks for less motion
    Then the tooltip of the button "Prune the demo repository" fades in over 100 ms
    And nothing on the page moves, turns or changes size

  Scenario: A tooltip fades out faster than it fades in
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" fades in over 100 ms
    When I press Escape
    Then the tooltip of the button "Prune the demo repository" fades out over 60 ms

  Scenario: A tooltip follows its control as the page scrolls
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo note above"
    And the page scrolls up a few lines
    Then the tooltip of the button "Demo note above" stands beside it, without covering it
