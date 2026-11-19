@ui
Feature: The table
  A table, sorted by keyboard, keeps a focused control in a row through the
  row's changes, says why it has no rows, and scrolls sideways in a frame of
  its own on a small screen.

  Scenario: Until its first sort a table shows its rows in the order given, and no column says it is sorted
    When I open the component gallery
    Then the rows of the table "Demo folders" are "photos, music, documents, notes"
    And no column header of the table "Demo folders" says it is sorted

  Scenario: A table's column is sorted by Enter, then the other way by Space, and each sort is said
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by size"
    And I press Enter
    Then the rows of the table "Demo folders" are "notes, documents, photos, music"
    And the column header "Size" alone of the table "Demo folders" says it is sorted "ascending"
    And the polite region reads "Sorted by size, smallest first."
    When I press Space
    Then the rows of the table "Demo folders" are "music, photos, documents, notes"
    And the column header "Size" alone of the table "Demo folders" says it is sorted "descending"
    And the polite region reads "Sorted by size, largest first."

  Scenario: Rows that tie keep their order, whichever way the table is sorted
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by files"
    And I press Enter
    Then the rows of the table "Demo folders" are "music, notes, documents, photos"
    When I press Enter
    Then the rows of the table "Demo folders" are "photos, documents, music, notes"

  Scenario: A table's sort button sits in its column header apart from the header's text
    When I open the component gallery
    Then the button "Sort by changed" sits inside the column header "Changed", apart from its text

  Scenario: A row's action keeps its focus while its row changes and moves
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by files"
    And I press Enter
    And I press Tab until focus is on the button "Add a file to music"
    And I press Enter
    Then the row "music" of the table "Demo folders" reads "13" in the column "Files"
    And the row "Total" of the table "Demo folders" reads "1,541" in the column "Files"
    And the rows of the table "Demo folders" are "notes, music, documents, photos"
    And focus is kept on the button "Add a file to music"

  Scenario: A table's numbers stand at the end of their column in figures of one width
    When I open the component gallery
    Then the column "Size" of the table "Demo folders" is aligned to its end in figures of one width
    And the column "Files" of the table "Demo folders" is aligned to its end in figures of one width
    And the column "Changed" of the table "Demo folders" is aligned to its start

  Scenario Outline: A table that can't show its rows says why in their place
    When I open the component gallery
    And I press Shift+Tab until focus is on the radio "Demo rows"
    And I press the down arrow until focus is on the radio "<choice>"
    Then the "Table" demo says "<sentence>" in the place of the table "Demo folders"

    Examples:
      | choice           | sentence                                              |
      | Demo empty       | No demo folders yet.                                  |
      | Demo unreachable | Ladderback can't be reached, so no demo folders show. |

  Scenario: A table whose rows are on their way says so beside a spinner, just before the busy region where the table will stand
    When I open the component gallery
    And I press Shift+Tab until focus is on the radio "Demo rows"
    And I press the down arrow until focus is on the radio "Demo loading"
    Then the "Table" demo says "Loading the demo folders…" in the place of the table "Demo folders"
    And the "Table" demo shows the text "Loading the demo folders…" beside a spinner a screen reader skips
    And the "Table" demo says "Loading the demo folders…" just before its busy region, outside it

  Scenario Outline: On a small screen with larger text, a table's frame scrolls by keyboard and each row's header stays in view, <direction>
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the region "<table>"
    And I press <key> until the region "<table>" has scrolled to its end
    Then each row header of the table "<table>" stands whole at the start of its frame
    And the page doesn't scroll sideways

    Examples:
      | direction     | table                       | key        |
      | left to right | Demo folders                | ArrowRight |
      | right to left | Demo folders, right to left | ArrowLeft  |

  Scenario Outline: A table's row headers are filled with the page's background, so the cells scrolled under them don't show through, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then each row header of the table "Demo folders" is filled with the page's background color

    Examples:
      | colors        |
      | light colors  |
      | dark colors   |
      | forced colors |
