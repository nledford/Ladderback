@ui
Feature: Browsing a snapshot's tree by keyboard
  A snapshot listed on the Snapshots page opens on a page of its own, which
  shows its folders and files in a tree: one stop for Tab, whose arrow keys
  move from row to row and open and close folders, each folder read from the
  repository as it is first opened. The journeys use the keyboard alone,
  against a daemon this run started with a demo plan and a scratch repository
  the plan has backed up to, so every folder they open is read from a real
  snapshot. Restoring and downloading a selected entry have journeys of
  their own, in features/restore/.

  Scenario: A snapshot opens from its plan's history, and its tree is one stop for Tab
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the link to the newest snapshot of the plan "home"
    And I press Enter
    Then focus is on the main heading
    And the main heading names the newest snapshot of the plan "home" in "demo"
    When I press Tab until focus is on a tree item
    Then the tree "Folders and files" has one tab stop
    And the page has no accessibility violations

  Scenario: Opening folder after folder by keyboard reaches the files the plan backed up
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    Then the tree item "demo-1.txt" is described as a file modified at some time
    And the tree item "demo-3.txt" is item 3 of 3
    And the page has no accessibility violations

  Scenario: A snapshot that's gone says so in place of its tree
    Given a demo plan "home" with a tiny scratch repository
    When I open the address "/snapshots/demo/0000000000000000000000000000000000000000000000000000000000000000"
    Then the page alerts "The repository has no snapshot 0000000000000000000000000000000000000000000000000000000000000000. It may have been forgotten; open a snapshot the list still shows."
    And the page shows no tree
    And the page has no accessibility violations
