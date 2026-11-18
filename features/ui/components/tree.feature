@ui
Feature: The tree
  A snapshot's tree is one stop for Tab, moved within by the arrow keys in the
  page's direction, its folders read as they are opened.

  Scenario: The tree is one stop for Tab, and the arrow keys, Home and End move within it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    Then the tree "Demo snapshot" has one tab stop
    And the tree item "documents" is item 1 of 10 at level 1
    When I press ArrowDown
    Then focus is on the tree item "empty"
    When I press End
    Then focus is on the tree item "readme.txt"
    And the tree item "readme.txt" is described by "File, 1.2 KiB"
    When I press Home
    Then focus is on the tree item "documents"
    When I press ArrowDown
    And I press Tab
    Then focus has left the tree "Demo snapshot"
    When I press Shift+Tab
    Then focus is on the tree item "empty"

  Scenario: Right opens a folder, reading it as it opens, and Left closes it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    Then the tree item "documents" is collapsed
    When I press ArrowRight
    Then the tree item "documents" is expanded
    And the tree item "notes.txt" is item 1 of 2 at level 2
    And focus is on the tree item "documents"
    When I press ArrowRight
    Then focus is on the tree item "notes.txt"
    When I press ArrowLeft
    Then focus is on the tree item "documents"
    When I press ArrowLeft
    Then the tree item "documents" is collapsed
    And the tree shows no tree item "notes.txt"
    When I press Enter
    Then the tree item "documents" is expanded

  Scenario: A folder holding nothing says it is empty
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I press ArrowDown
    And I press ArrowRight
    Then the tree item "empty" is expanded
    And the tree item "empty" is described by "Folder, empty"

  Scenario: A folder read in pages ends in a row that reads the next page, and focus moves to what it read
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I type "m" in the tree
    And I press ArrowRight
    Then the tree item "alpha.txt" is item 1 of 5 at level 2
    When I press ArrowDown until focus is on the tree item "Show more: 2 of 5 shown"
    And I press Enter
    Then focus is on the tree item "charlie.txt"
    And the tree item "charlie.txt" is item 3 of 5 at level 2
    And the tree has the tree item "Show more: 4 of 5 shown"

  Scenario: A folder still being read is marked busy
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I type "s" in the tree
    And I press ArrowRight
    Then the tree item "slow" is busy
    And the tree item "slow" is described by "Folder, loading"

  Scenario: A folder that can't be read says why and stays closed
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I type "u" in the tree
    And I press ArrowRight
    Then the page alerts "The demo folder could not be read."
    And the tree item "unreadable" is collapsed
    And focus is on the tree item "unreadable"
    And the "Tree" demo has no accessibility violations

  Scenario: Typing the first letters of a name moves focus to it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I type "re" in the tree
    Then focus is on the tree item "readme.txt"
    When I type "l" in the tree
    Then focus is on the tree item "latest"
    And the tree item "latest" is described by "Link"

  Scenario: A row is named by the name it shows, not by the key that opens it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    Then the tree has the tree item "abc[U+202E]txt.exe"

  Scenario: A name written right to left keeps its direction in a left-to-right page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    Then the tree item "הערות.txt" shows its name right to left

  Scenario: In a page that runs right to left, Left opens a folder and Right closes it
    When I open the component gallery asking for the locale "ar-XB"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowLeft
    Then the focused tree item is expanded
    When I press ArrowRight
    Then the focused tree item is collapsed
