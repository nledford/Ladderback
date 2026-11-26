@ui
Feature: The table
  A table, sorted by keyboard, keeps a focused control in a row through the
  row's changes, says why it has no rows, scrolls sideways in a frame of its
  own on a small screen, and lets the reader hide, show and reorder its
  columns, a choice this browser keeps.

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
    And the page doesn't show "Loading the demo folders…"

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

  # The Columns panel (decision 140): a table may let the reader hide, show
  # and reorder its columns after the row header's, by keyboard through the
  # panel's checkboxes and Move buttons, or by dragging an item of its list
  # by its handle. This browser keeps the choice for that table.
  Scenario: The Columns button opens a panel that lists every column after the row header's, in the table's order
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    Then the button "Columns" says its panel is open
    And the Columns panel of the "Table" demo lists "Size, Changed, Files, Actions", each shown
    And the button "Move Size up" says it is unavailable, and is not disabled, wherever focus is
    And the button "Move Actions down" says it is unavailable, and is not disabled, wherever focus is
    And the "Table" demo has no accessibility violations

  Scenario: A column hidden in the Columns panel leaves the table, its total with it, and shows again
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    And I press Tab until focus is on the checkbox "Size"
    And I press Space
    Then the column headers of the table "Demo folders" are "Name, Changed, Files, Actions"
    And the polite region reads "Size hidden."
    And the row "Total" of the table "Demo folders" reads "1,540" in the column "Files"
    When I press Space
    Then the column headers of the table "Demo folders" are "Name, Size, Changed, Files, Actions"
    And the polite region reads "Size shown."

  Scenario: Hiding the column a table is sorted by returns its rows to the order given
    When I open the "Table" demo
    And I press Tab until focus is on the button "Sort by size"
    And I press Enter
    And I press Shift+Tab until focus is on the button "Columns"
    And I press Enter
    And I press Tab until focus is on the checkbox "Size"
    And I press Space
    Then the rows of the table "Demo folders" are "photos, music, documents, notes"
    And no column header of the table "Demo folders" says it is sorted

  Scenario: A column moves by its Move buttons, and focus stays on the button pressed until its column can't move that way
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    And I press Tab until focus is on the button "Move Files up"
    And I press Enter
    Then the column headers of the table "Demo folders" are "Name, Size, Files, Changed, Actions"
    And the polite region reads "Files moved to position 2 of 4."
    And focus is kept on the button "Move Files up"
    And the row "Total" of the table "Demo folders" reads "1,540" in the column "Files"
    When I press Enter
    Then the column headers of the table "Demo folders" are "Name, Files, Size, Changed, Actions"
    And the polite region reads "Files moved to position 1 of 4."
    And focus is kept on the button "Move Files down"
    And the button "Move Files up" says it is unavailable, and is not disabled, wherever focus is

  Scenario: Reset columns shows every column again, in the order given
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    And I press Tab until focus is on the checkbox "Size"
    And I press Space
    And I press Tab until focus is on the button "Move Files up"
    And I press Enter
    And I press Tab until focus is on the button "Reset columns"
    And I press Enter
    Then the column headers of the table "Demo folders" are "Name, Size, Changed, Files, Actions"
    And the checkbox "Size" is checked
    And the polite region comes to contain "Columns reset: every column shown, in the order given."
    And focus is kept on the button "Reset columns"

  Scenario: The columns chosen for a table are kept in this browser
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    And I press Tab until focus is on the checkbox "Changed"
    And I press Space
    And I press Tab until focus is on the button "Move Actions up"
    And I press Enter
    And I reload the page
    Then the column headers of the table "Demo folders" are "Name, Size, Actions, Files"

  Scenario: Escape closes the Columns panel and hands focus back to its button
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    And I press Tab until focus is on the checkbox "Files"
    And I press Escape
    Then focus is on the button "Columns"
    And the button "Columns" says its panel is closed

  # Dragging is never the only way (2.5.7): the Move buttons above do it
  # all by keyboard. This is the pointer's own behavior, so its step moves
  # and presses the pointer through support/pointer.ts.
  Scenario: A column dragged by its handle in the Columns panel takes the place it is dropped at
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    And I drag the column "Files" by its handle onto the top half of the column "Size" in the Columns panel
    Then the column headers of the table "Demo folders" are "Name, Files, Size, Changed, Actions"
    And the polite region reads "Files moved to position 1 of 4."

  # A line shows where the held column would land, and only there, under
  # forced colors too, where a border kept transparent elsewhere would be
  # painted in the text's color.
  Scenario Outline: A column held by its handle shows one line where it would land, in <colors>
    Given the browser asks for "<colors>"
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    And I hold the column "Files" by its handle over the top half of the column "Size" in the Columns panel
    Then the Columns panel draws one line, before the column "Size"
    When I let go of the pointer
    Then the column headers of the table "Demo folders" are "Name, Files, Size, Changed, Actions"

    Examples:
      | colors        |
      | light colors  |
      | forced colors |

  # A pointer's drag leaves focus where it was, so a row's control that had
  # it keeps it as its column moves (2.4.3).
  Scenario: A column dragged keeps the focus a control in its cells had
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    And I press Tab until focus is on the button "Add a file to music"
    And I drag the column "Actions" by its handle onto the top half of the column "Size" in the Columns panel
    Then the column headers of the table "Demo folders" are "Name, Actions, Size, Changed, Files"
    And focus is kept on the button "Add a file to music"

  Scenario: On a small screen with larger text, the open Columns panel doesn't make the page scroll sideways
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the "Table" demo
    And I press Tab until focus is on the button "Columns"
    And I press Enter
    Then the Columns panel of the "Table" demo lists "Size, Changed, Files, Actions", each shown
    And the page doesn't scroll sideways

  # The change mark (decision 129): a live change to a row's watched column
  # marks the row "changed, not yet seen" until the reader sees it, in bold,
  # with an edge, and in a word a screen reader hears with the status. The
  # demo's button stands in for a live update, changing the status of the
  # next row in turn: photos first, then music. Mark all as seen stands
  # beside it, in the demo's row of actions, as it does in a page's.
  Scenario: A row a live change marks stays marked until focus enters it
    When I open the "Live table" demo
    And I press Tab until focus is on the button "Change a demo status"
    And I press Enter
    Then the row "photos" of the table "Demo jobs" shows "Running" in its "Status" column
    And the row "photos" of the table "Demo jobs" is marked as changed, its "Status" in bold and an edge at its start
    And the row "photos" of the table "Demo jobs" says "Changed, not yet seen" in its "Status" column to a screen reader
    And the row "music" of the table "Demo jobs" is not marked
    And the row "music" of the table "Demo jobs" doesn't say "Changed, not yet seen" in its "Status" column to a screen reader
    And focus is kept on the button "Change a demo status"
    When I press Tab until focus is on the button "Star photos"
    Then the row "photos" of the table "Demo jobs" is not marked
    And the row "photos" of the table "Demo jobs" doesn't say "Changed, not yet seen" in its "Status" column to a screen reader
    And the page doesn't offer the button "Mark all as seen"

  Scenario: Mark all as seen clears every mark, says how many, and hands focus to the demo's heading as it goes
    When I open the "Live table" demo
    And I press Tab until focus is on the button "Change a demo status"
    And I press Enter
    And I press Enter
    Then the row "photos" of the table "Demo jobs" is marked as changed, its "Status" in bold and an edge at its start
    And the row "music" of the table "Demo jobs" is marked as changed, its "Status" in bold and an edge at its start
    When I press Tab
    Then focus is kept on the button "Mark all as seen"
    When I press Enter
    Then the polite region reads "2 changed rows marked as seen."
    And focus is on the heading "Live table"
    And the row "photos" of the table "Demo jobs" is not marked
    And the row "music" of the table "Demo jobs" is not marked
    And the row "photos" of the table "Demo jobs" doesn't say "Changed, not yet seen" in its "Status" column to a screen reader
    And the page doesn't offer the button "Mark all as seen"

  # Words a cell keeps out of sight, as a marked row's word for a screen
  # reader and the note after an outside link's words are, lie inside the
  # table's frame, which scrolls: so a row wider than a small screen, with
  # those words far past its edge, never makes the page itself scroll
  # sideways (1.4.10).
  Scenario: A marked row's word and an outside link's note stay inside the table's frame on a small screen with larger text
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the "Live table" demo
    And I press Tab until focus is on the button "Change a demo status"
    And I press Enter
    Then the row "photos" of the table "Demo jobs" says "Changed, not yet seen" in its "Status" column to a screen reader
    And the link "rustic's guide for photos (goes to another site)" shows the words "rustic's guide for photos", then an icon a screen reader skips
    And the page doesn't scroll sideways

  Scenario Outline: A marked row keeps its bold status and its edge in <colors>
    Given the browser asks for "<colors>"
    When I open the "Live table" demo
    And I press Tab until focus is on the button "Change a demo status"
    And I press Enter
    Then the row "photos" of the table "Demo jobs" is marked as changed, its "Status" in bold and an edge at its start

    Examples:
      | colors        |
      | light colors  |
      | dark colors   |
      | forced colors |
