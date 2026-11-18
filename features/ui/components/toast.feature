@ui
Feature: The toast
  A toast answers what the reader did without taking focus, its icon naming
  its tone, and is announced politely while its going is not. It goes by
  itself only when nothing in it needs the reader, and never while the reader
  is on it.

  Scenario: A toast answers the reader politely without taking focus, is no live region, and goes by itself unannounced
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Save the demo settings"
    And I press Enter
    Then the toast reads "Settings saved." after its icon "Success"
    And the toast is no live region, nor holds one
    And focus stays on the button "Save the demo settings"
    And the polite region reads "Settings saved."
    When the toast "Settings saved." is gone
    Then the polite region reads "Settings saved."

  Scenario Outline: A toast in the <tone> tone names its tone by its icon, is said in the <region> region and takes no focus
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "<button>"
    And I press Enter
    Then the toast reads "<words>" after its icon "<name>"
    And the <region> region reads "<words>"
    And focus stays on the button "<button>"
    And the "Toast" demo has no accessibility violations

    Examples:
      | tone    | button                    | words                                                          | name        | region    |
      | primary | Queue a demo check        | Demo check queued.                                             | Information | polite    |
      | success | Save the demo settings    | Settings saved.                                                | Success     | polite    |
      | warning | Back up the demo plan     | Demo backup finished with warnings: 2 files couldn't be read. | Warning     | polite    |
      | error   | Restore the demo snapshot | The demo restore failed.                                       | Error       | assertive |

  Scenario Outline: A toast in the <tone> tone goes by itself <lifetime> seconds after it arrives
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "<button>"
    And I press Enter
    And the page's clock moves on 5 seconds
    Then the toast reads "<words>" after its icon "<name>"
    When the page's clock moves on 2 seconds
    Then the "Toast" demo shows no toast

    Examples:
      | tone    | button                 | words              | name        | lifetime |
      | primary | Queue a demo check     | Demo check queued. | Information | 6        |
      | success | Save the demo settings | Settings saved.    | Success     | 6        |

  Scenario Outline: A toast in the <tone> tone needs the reader, so it stays until the reader dismisses it
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "<button>"
    And I press Enter
    And the page's clock moves on 600 seconds
    Then the toast reads "<words>" after its icon "<name>"

    Examples:
      | tone    | button                    | words                                                          | name    |
      | warning | Back up the demo plan     | Demo backup finished with warnings: 2 files couldn't be read. | Warning |
      | error   | Restore the demo snapshot | The demo restore failed.                                       | Error   |

  Scenario: A toast waits while focus is on it, and has its whole time again once focus leaves
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Save the demo settings"
    And I press Enter
    And the page's clock moves on 3 seconds
    And I press Tab until focus is on the toast's Dismiss button
    And the page's clock moves on 600 seconds
    Then the toast reads "Settings saved." after its icon "Success"
    When I press Tab
    And the page's clock moves on 5 seconds
    Then the toast reads "Settings saved." after its icon "Success"
    When the page's clock moves on 2 seconds
    Then the "Toast" demo shows no toast

  Scenario: A toast waits while the pointer rests on it, and has its whole time again once the pointer leaves
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Save the demo settings"
    And I press Enter
    And the page's clock moves on 3 seconds
    And I rest the pointer on the toast
    And the page's clock moves on 600 seconds
    Then the toast reads "Settings saved." after its icon "Success"
    When I move the pointer off the toast
    And the page's clock moves on 5 seconds
    Then the toast reads "Settings saved." after its icon "Success"
    When the page's clock moves on 2 seconds
    Then the "Toast" demo shows no toast

  Scenario Outline: Escape on a toast's Dismiss button leaves the toast to the reader, and <key> takes it away, says nothing more and hands focus to the control its page names, the failure staying on the page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Restore the demo snapshot"
    And I press Enter
    Then the assertive region reads "The demo restore failed."
    And the "Toast" demo shows an alert with the words "The demo restore failed: its target folder can't be written to. Choose another folder and restore again."
    When I press Tab until focus is on the toast's Dismiss button
    And I press Escape
    Then the toast reads "The demo restore failed." after its icon "Error"
    When I press <key>
    Then the "Toast" demo shows no toast
    And focus is on the button "Save the demo settings"
    And the polite region stays empty
    And the "Toast" demo shows an alert with the words "The demo restore failed: its target folder can't be written to. Choose another folder and restore again."

    Examples:
      | key   |
      | Enter |
      | Space |

  Scenario: A new toast takes the place of the one shown
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Restore the demo snapshot"
    And I press Enter
    And I press Shift+Tab until focus is on the button "Save the demo settings"
    And I press Enter
    Then the toast reads "Settings saved." after its icon "Success"
    When I press Tab until focus is on the button "Back up the demo plan"
    And I press Enter
    Then the toast reads "Demo backup finished with warnings: 2 files couldn't be read." after its icon "Warning"

  Scenario Outline: A toast in the <tone> tone lies on its fill with its words, its icon and its Dismiss button in its words' color, edged in its tone, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "<button>"
    And I press Enter
    Then the toast lies on <fill> with its words, its icon and its Dismiss button in <words>, edged 1 pixel wide in <edge>

    Examples:
      | tone    | colors       | button                    | fill         | words        | edge      |
      | primary | light colors | Queue a demo check        | --surface    | --fg         | --accent  |
      | success | light colors | Save the demo settings    | --success-bg | --success-fg | --success |
      | warning | light colors | Back up the demo plan     | --warning-bg | --warning-fg | --warning |
      | error   | light colors | Restore the demo snapshot | --error-bg   | --error-fg   | --error   |
      | primary | dark colors  | Queue a demo check        | --surface    | --fg         | --accent  |
      | success | dark colors  | Save the demo settings    | --success-bg | --success-fg | --success |
      | warning | dark colors  | Back up the demo plan     | --warning-bg | --warning-fg | --warning |
      | error   | dark colors  | Restore the demo snapshot | --error-bg   | --error-fg   | --error   |

  Scenario: Under forced colors a toast is edged in the text's color, its icon in the text's color too
    Given the browser asks for "forced colors"
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Restore the demo snapshot"
    And I press Enter
    Then the toast is edged 1 pixel wide in CanvasText, its icon in the text's color

  Scenario Outline: A toast's icon stands at the start of its line and its Dismiss button at the end, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    And I skip to the content
    And I press Enter on the first button of the toast demo
    Then the toast has its icon before its words and its Dismiss button after them, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, a toast's words wrap inside it, its icon and Dismiss button inside it too
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Back up the demo plan"
    And I press Enter
    Then the toast holds its icon, its words and its Dismiss button inside its edge
    And the page doesn't scroll sideways
    And no text is clipped
