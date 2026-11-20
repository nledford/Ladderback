@ui
Feature: The steps
  A list of steps shows a process's steps in order, each with its name, its
  state in words and an icon whose shape is the state's own, the step the
  process stands at marked as the current one; it is no stop for Tab and no
  live region, the announcer says politely where the process has moved, and
  it stands in a row where it has room and in a column on a small screen with
  larger text.

  Scenario: A list of steps reads each step's name and its state in words, in order, the step its process stands at marked as the current one
    When I open the "Steps" demo
    Then the list of steps "Demo hub move" reads, in order:
      | step                                                                  | state   |
      | Check                                                                 | Done    |
      | Copy                                                                  | Done    |
      | Commit the demo move once every demo snapshot has reached the new hub | Current |
      | Switch the machines                                                   | To do   |
      | Clean up                                                              | To do   |
    And the current step of the list of steps "Demo hub move" is "Commit the demo move once every demo snapshot has reached the new hub"
    And the list of steps "Demo pairing" reads, in order:
      | step     | state  |
      | Token    | Done   |
      | Connect  | Failed |
      | Approval | To do  |
    And the current step of the list of steps "Demo pairing" is "Connect"

  Scenario: Each state of a step has an icon of its own before its name, which a screen reader skips
    When I open the "Steps" demo
    Then each state of a step in the steps demo has an icon of its own before its name, which a screen reader skips

  Scenario: Under forced colors every step's icon and connectors stay drawn, in the system's colors
    Given the browser asks for "forced colors"
    When I open the "Steps" demo
    Then every icon and connector of the steps demo is drawn in a color of its own, not the page's background

  Scenario: A list of steps is no stop for Tab, and its next step is said politely while focus stays where it was
    When I open the "Steps" demo
    And I press Tab
    Then focus is on the button "Advance the demo hub move"
    And the list of steps "Demo hub move" is no live region, nor inside one
    And the list of steps "Demo pairing" is no live region, nor inside one
    And the polite region stays empty
    When I press Enter
    Then the polite region reads "Step 4 of 5: Switch the machines"
    And focus is on the button "Advance the demo hub move"
    And the current step of the list of steps "Demo hub move" is "Switch the machines"
    When I press Tab
    Then focus has left the steps demo

  Scenario Outline: Where it has room, a list of steps stands in a row in the order its page reads, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    Then every list of steps of the steps demo stands in a row, in a page that runs <direction>
    And the page has no accessibility violations

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario Outline: On a small screen with larger text, a list of steps stands in a column, every step whole in the window and its long name wrapped, in a page that runs <direction>
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery asking for the locale "<locale>"
    Then every list of steps of the steps demo stands in a column, every step whole inside the window, in a page that runs <direction>
    And a step of the steps demo wraps its name onto more than one line
    And the page doesn't scroll sideways
    And no text is clipped

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |
