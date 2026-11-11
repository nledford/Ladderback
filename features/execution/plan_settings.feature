@e2e
Feature: A plan's settings shape its backups
  A backup the daemon runs leaves out what the plan's source filter leaves
  out, as rustic would, and its snapshot records the snapshot host: the
  plan's own host, else the agent's hostname (technical design §6.1, §7.3).
  The snapshots are read back with rustic itself, not through Ladderback.

  Background:
    Given Ladderback is serving on loopback with the plan "home"

  Scenario: A backup leaves out what the plan's source filter excludes
    Given the plan's folder holds a "node_modules" folder
    And the plan's folder is a git repository whose ".gitignore" lists "notes.log"
    And the plan's source filter has the iglob "!node_modules" and follows ".gitignore" files
    When the daemon has finished 1 backup of the plan "home"
    Then the history page shows the operation succeeded
    And the backup's snapshot holds the plan's 3 files
    And the backup's snapshot holds nothing named "node_modules" or "notes.log"

  Scenario: A snapshot records the plan's own host, not the agent's
    Given the agent's hostname is "demo-desktop"
    And the plan's host is "demo-laptop"
    When the daemon has finished 1 backup of the plan "home"
    Then the history page shows the operation succeeded
    And the backup's snapshot records the host "demo-laptop"

  Scenario: A snapshot records the agent's hostname when the plan names no host
    Given the agent's hostname is "demo-desktop"
    When the daemon has finished 1 backup of the plan "home"
    Then the history page shows the operation succeeded
    And the backup's snapshot records the host "demo-desktop"

  Scenario: A plan's first backup builds on the history its host recorded without Ladderback
    Given the folder was backed up without Ladderback under the host "demo-laptop"
    And the plan's host is "demo-laptop"
    When the daemon has finished 1 backup of the plan "home"
    Then the history page shows the operation succeeded
    And the new snapshot's parent is the snapshot made without Ladderback
