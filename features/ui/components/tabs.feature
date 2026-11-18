@ui
Feature: The tabs
  A set of tabs is one stop for Tab, in a row above its panel, in a column at
  its start, or in whichever of the two its container has room for. Its arrow
  keys follow the layout shown and the page's direction, and each panel is
  kept as it was while another is shown or the layout changes.

  Scenario: A tab list is one stop for Tab, on its selected tab, and Tab from it goes into that tab's panel
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo overview"
    Then the tab "Demo overview" is selected and the one Tab stop of its list
    And the tab "Demo overview" shows its panel, and every other panel of its list is drawn but hidden
    When I press Tab
    Then focus is on the panel of the tab "Demo overview", which the tab names
    When I press Shift+Tab
    Then focus is on the tab "Demo overview"

  Scenario: The arrow keys move between the tabs and wrap at either end, Home and End go to the first and last, and each tab arrived at is selected
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo overview"
    And I press ArrowRight
    Then focus is on the tab "Demo note"
    And the tab "Demo note" is selected and the one Tab stop of its list
    And the tab "Demo note" shows its panel, and every other panel of its list is drawn but hidden
    When I press ArrowRight
    Then focus is on the tab "Demo history"
    When I press ArrowRight
    Then focus is on the tab "Demo overview"
    And the tab "Demo overview" is selected and the one Tab stop of its list
    When I press ArrowLeft
    Then focus is on the tab "Demo history"
    And the tab "Demo history" is selected and the one Tab stop of its list
    When I press Home
    Then focus is on the tab "Demo overview"
    When I press End
    Then focus is on the tab "Demo history"
    And the tab "Demo history" shows its panel, and every other panel of its list is drawn but hidden
    And the live regions stay silent

  Scenario: Tabs that are chosen by hand move focus with the arrow keys, select a tab only on Enter or Space, and tell the page which tab is chosen
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo log"
    Then the tab list of the tab "Demo log" is described by "These demo tabs are chosen with Enter or Space. The arrow keys only move between them."
    And the demo says "Chosen demo tab: Demo log"
    When I press ArrowRight
    Then focus is on the tab "Demo stats"
    And the tab "Demo log" is selected and the one Tab stop of its list
    And the tab "Demo log" shows its panel, and every other panel of its list is drawn but hidden
    And the demo says "Chosen demo tab: Demo log"
    When I press Tab
    Then focus is on the panel of the tab "Demo log", which the tab names
    When I press Shift+Tab
    Then focus is on the tab "Demo log"
    When I press ArrowRight
    And I press Enter
    Then the tab "Demo stats" is selected and the one Tab stop of its list
    And the tab "Demo stats" shows its panel, and every other panel of its list is drawn but hidden
    And the demo says "Chosen demo tab: Demo stats"
    When I press End
    Then focus is on the tab "Demo plans"
    And the tab "Demo stats" is selected and the one Tab stop of its list
    And the demo says "Chosen demo tab: Demo stats"
    When I press Space
    Then the tab "Demo plans" is selected and the one Tab stop of its list
    And focus is on the tab "Demo plans"
    And the demo says "Chosen demo tab: Demo plans"
    And the live regions stay silent

  Scenario: In tabs that run right to left, the left arrow goes to the next tab and the right arrow to the one before
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "First demo tab"
    Then the tab "Second demo tab" stands to the left of the tab "First demo tab"
    When I press ArrowLeft
    Then focus is on the tab "Second demo tab"
    And the tab "Second demo tab" is selected and the one Tab stop of its list
    When I press ArrowRight
    Then focus is on the tab "First demo tab"
    When I press ArrowRight
    Then focus is on the tab "Third demo tab"
    When I press ArrowLeft
    Then focus is on the tab "First demo tab"

  Scenario: What is typed in a panel stays while another tab is shown, and a panel holding a field is no Tab stop of its own
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo overview"
    And I press ArrowRight
    Then the panel of the tab "Demo note" is no Tab stop of its own
    When I press Tab
    Then focus is on the textbox "Demo note text"
    When I type "Kept while another tab shows"
    And I press Shift+Tab
    Then focus is on the tab "Demo note"
    When I press ArrowRight
    Then the tab "Demo history" shows its panel, and every other panel of its list is drawn but hidden
    When I press ArrowLeft
    Then the textbox "Demo note text" still holds "Kept while another tab shows"

  Scenario Outline: The selected tab is bold with a bar in Ladder Blue on the rule under the list, and the others stand on the rule, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the tab "Demo overview" is bold, with a 3 pixel bar in --accent on the rule under its list
    And the tab "Demo note" is in the text's weight and color, with no bar, on a 1 pixel rule in --muted
    And the tab list of the tab "Demo overview" stands on a 1 pixel rule in --muted
    And the tabs of the list of the tab "Demo overview" touch their neighbours on each line
    And the "Tabs" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors the selected tab keeps its weight and its bar, and the others stand on the rule, in the system's text color
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then the tab "Demo overview" is bold, with a 3 pixel bar in the system's text color on the rule under its list
    And the tab "Demo note" is in the text's weight and color, with no bar, on a 1 pixel rule in the system's text color
    And the tab list of the tab "Demo overview" stands on a 1 pixel rule in the system's text color

  Scenario: On a small screen with larger text the tabs wrap onto more lines, every tab whole and in view on its own rule, above the list's rule
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then every tab of the list of the tab "Demo overview" shows whole inside the window, on more than one line, above the list's rule
    And the tab "Demo overview" stands on a line above the last of its list
    And every tab of the list of the tab "Demo overview" stands on its own rule: the selected one on a 3 pixel bar in --accent, the others on a 1 pixel rule in --muted
    And a tab of the list of the tab "Demo sources" wraps its words onto more than one line
    And every tab of the list of the tab "Demo sources" is as wide as the list, its words on every line at the column's start
    And a tab of the list of the tab "First vertical tab" wraps its words onto more than one line
    And every tab of the list of the tab "First vertical tab" is as wide as the list, its words on every line at the column's start
    And the page doesn't scroll sideways

  Scenario: Horizontal tabs, the default, stand in a row above their panel, say so, and leave the up and down arrows to the page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo overview"
    Then the tab list of the tab "Demo overview" is horizontal, a row above its panel
    When I press ArrowDown
    Then focus is on the tab "Demo overview"
    When I press ArrowUp
    Then focus is on the tab "Demo overview"
    And the tab "Demo overview" is selected and the one Tab stop of its list

  Scenario: Vertical tabs stand in a column at the start of their panel, say so, and the up and down arrows move between them, wrapping at either end
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo sources"
    Then the tab list of the tab "Demo sources" is vertical, a column at the start of its panel
    And the tab "Demo sources" is selected and the one Tab stop of its list
    When I press ArrowDown
    Then focus is on the tab "Demo times"
    And the tab "Demo times" is selected and the one Tab stop of its list
    And the tab "Demo times" shows its panel, and every other panel of its list is drawn but hidden
    When I press ArrowDown
    Then focus is on the tab "Demo hooks"
    When I press ArrowDown
    Then focus is on the tab "Demo sources"
    When I press ArrowUp
    Then focus is on the tab "Demo hooks"
    And the tab "Demo hooks" is selected and the one Tab stop of its list
    When I press Home
    Then focus is on the tab "Demo sources"
    When I press End
    Then focus is on the tab "Demo hooks"
    When I press ArrowLeft
    Then focus is on the tab "Demo hooks"
    When I press ArrowRight
    Then focus is on the tab "Demo hooks"
    When I press Tab
    Then focus is on the panel of the tab "Demo hooks", which the tab names
    And the live regions stay silent

  Scenario Outline: Vertical tabs, right to left too, and responsive tabs in a column stand on a rule at the end of their list, the selected one bold with a bar in Ladder Blue on it, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the tab "Demo sources" is bold, with a 3 pixel bar in --accent on the rule beside its list
    And the tab "Demo times" is in the text's weight and color, with no bar, on a 1 pixel rule in --muted beside its list
    And the tab list of the tab "Demo sources" stands beside a 1 pixel rule in --muted
    And the tab "First vertical tab" is bold, with a 3 pixel bar in --accent on the rule beside its list
    And the tab "Second vertical tab" is in the text's weight and color, with no bar, on a 1 pixel rule in --muted beside its list
    And the tab list of the tab "First vertical tab" stands beside a 1 pixel rule in --muted
    And the tab "Demo general" is bold, with a 3 pixel bar in --accent on the rule beside its list
    And the tab "Demo address" is in the text's weight and color, with no bar, on a 1 pixel rule in --muted beside its list
    And the tab list of the tab "Demo general" stands beside a 1 pixel rule in --muted
    And the "Vertical tabs" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors vertical tabs, right to left too, and responsive tabs in a column keep the selected tab's weight and bar, and the others stand on the rule, in the system's text color
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then the tab "Demo sources" is bold, with a 3 pixel bar in the system's text color on the rule beside its list
    And the tab "Demo times" is in the text's weight and color, with no bar, on a 1 pixel rule in the system's text color beside its list
    And the tab list of the tab "Demo sources" stands beside a 1 pixel rule in the system's text color
    And the tab "First vertical tab" is bold, with a 3 pixel bar in the system's text color on the rule beside its list
    And the tab "Second vertical tab" is in the text's weight and color, with no bar, on a 1 pixel rule in the system's text color beside its list
    And the tab list of the tab "First vertical tab" stands beside a 1 pixel rule in the system's text color
    And the tab "Demo general" is bold, with a 3 pixel bar in the system's text color on the rule beside its list
    And the tab "Demo address" is in the text's weight and color, with no bar, on a 1 pixel rule in the system's text color beside its list
    And the tab list of the tab "Demo general" stands beside a 1 pixel rule in the system's text color

  Scenario: Vertical tabs in a part of the page that runs right to left stand at its start, on the right, and the down arrow still goes to the next tab
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo sources"
    And I press Tab until focus is on the tab "First vertical tab"
    Then the tab list of the tab "First vertical tab" is vertical, a column at the start of its panel
    When I press ArrowDown
    Then focus is on the tab "Second vertical tab"
    And the tab "Second vertical tab" is selected and the one Tab stop of its list
    When I press ArrowUp
    Then focus is on the tab "First vertical tab"
    When I press ArrowUp
    Then focus is on the tab "Third vertical tab"
    When I press ArrowLeft
    Then focus is on the tab "Third vertical tab"
    When I press ArrowRight
    Then focus is on the tab "Third vertical tab"
    And the tab "Third vertical tab" is selected and the one Tab stop of its list

  Scenario Outline: Tabs in a column are as wide as it, their words lined up at its start, <demo>
    When I open the component gallery
    Then the tab list of the tab "<tab>" is vertical, a column at the start of its panel
    And every tab of the list of the tab "<tab>" is as wide as the list, its words on every line at the column's start

    Examples:
      | demo                   | tab                |
      | vertical               | Demo sources       |
      | vertical right to left | First vertical tab |
      | responsive             | Demo general       |

  Scenario: Responsive tabs are vertical where their container has room for the list beside the panel, and horizontal in a narrow column of the same wide page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo general"
    Then the tab list of the tab "Demo general" is vertical, a column at the start of its panel
    When I press ArrowDown
    Then focus is on the tab "Demo address"
    And the tab "Demo address" is selected and the one Tab stop of its list
    When I press Tab until focus is on the tab "First column tab"
    Then the tab list of the tab "First column tab" is horizontal, a row above its panel
    When I press ArrowLeft
    Then focus is on the tab "Second column tab"
    And the tab "Second column tab" is selected and the one Tab stop of its list
    When I press ArrowDown
    Then focus is on the tab "Second column tab"
    When I press ArrowRight
    Then focus is on the tab "First column tab"

  Scenario: Responsive tabs that change layout keep the selected tab, the focus and what was typed in a panel, and their arrow keys follow the new layout
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo general"
    And I press ArrowDown
    And I press Tab
    Then focus is on the textbox "Demo address text"
    When I type "Kept while the layout changes"
    And I press Shift+Tab
    Then focus is on the tab "Demo address"
    When the window narrows to 480 pixels
    Then the tab list of the tab "Demo address" is horizontal, a row above its panel
    And focus is on the tab "Demo address"
    And the tab "Demo address" is selected and the one Tab stop of its list
    When I press ArrowRight
    Then focus is on the tab "Demo about"
    When I press ArrowLeft
    Then the textbox "Demo address text" still holds "Kept while the layout changes"
    When the window widens to 1280 pixels
    Then the tab list of the tab "Demo address" is vertical, a column at the start of its panel
    And focus is on the tab "Demo address"
    And the tab "Demo address" is selected and the one Tab stop of its list
    When I press ArrowUp
    Then focus is on the tab "Demo general"
    And the live regions stay silent

  Scenario Outline: On a small screen with larger text responsive tabs are horizontal, right to left too, every tab whole and in view, in <colors>
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the browser asks for "<colors>"
    When I open the component gallery
    Then the tab list of the tab "Demo general" is horizontal, a row above its panel
    And every tab of the list of the tab "Demo general" shows whole inside the window, on more than one line, above the list's rule
    And the tab list of the tab "First column tab" is horizontal, a row above its panel
    And every tab of the list of the tab "First column tab" shows whole inside the window, on more than one line, above the list's rule
    And the page doesn't scroll sideways
    And the "Responsive tabs" demo has no accessibility violations

    Examples:
      | colors        |
      | light colors  |
      | dark colors   |
      | forced colors |
