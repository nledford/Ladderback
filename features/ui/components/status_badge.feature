@ui
Feature: The status badge
  A status badge for every state an operation can be in, whose icon tells the
  state by its shape, drawn in its status tone's pair where the operation
  succeeded or failed.

  Scenario: Each state of a status badge has an icon of its own beside its text, which a screen reader skips
    When I open the component gallery
    Then each of the 7 badges of the "Status badge" demo has an icon of its own, in its text's color, which a screen reader skips

  Scenario: No status badge moves, the running one's icon either, however long the operation runs
    When I open the component gallery
    Then none of the 7 badges of the "Status badge" demo moves

  Scenario: A status badge's icon grows with the reader's text size
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the icon of each badge of the "Status badge" demo is as tall as its text

  Scenario Outline: A status badge of an operation that succeeded, succeeded with warnings or failed is drawn in its status tone's pair, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then each of these badges of the "Status badge" demo is drawn on its fill:
      | badge                   | words        | fill         |
      | Succeeded               | --success-fg | --success-bg |
      | Succeeded with warnings | --warning-fg | --warning-bg |
      | Failed                  | --error-fg   | --error-bg   |

    Examples:
      | colors       |
      | light colors |
      | dark colors  |
