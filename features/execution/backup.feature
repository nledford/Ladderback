@e2e
Feature: Backups
  A backup records which plan and agent made it, builds on that plan's last
  snapshot, and reports files it couldn't read without failing.

  Background:
    Given a new repository
    And a plan "home" backing up a folder with 3 files

  Scenario: A backup creates a snapshot tagged with its plan and agent
    When the agent backs up the plan
    Then the operation ends as succeeded
    And the history shows the operation as it ended
    And the history shows the backup's snapshot
    And the repository has 1 snapshot
    And the snapshot is tagged with the plan and the agent

  @needs-file-permissions
  Scenario: Unreadable files are warnings, not failures
    Given 1 of the files can't be read
    When the agent backs up the plan
    Then the operation ends as succeeded with warnings
    And the operation lists 1 warning
    And the history shows the operation as it ended
    And the repository has 1 snapshot

  Scenario: A plan's history stays attached when its folder moves
    Given the plan has been backed up
    And the plan's folder has moved
    When the agent backs up the plan
    Then the new snapshot's parent is the plan's previous snapshot

  Scenario: The first backup of a plan builds on rustic's own choice of parent
    Given the folder was backed up without Ladderback
    When the agent backs up the plan
    Then the new snapshot's parent is the snapshot made without Ladderback

  Scenario: A backup refuses a location that now holds another repository
    Given the repository's ID is known
    And another repository has replaced it at its location
    When the agent backs up the plan
    Then the operation ends as failed because its location holds another repository
    And the repository has no snapshots

  Scenario: A backup to a server that refuses the credentials fails, saying so
    Given the repository is on a REST server that refuses the credentials the agent sends it
    When the agent backs up the plan
    Then the operation ends as failed because the server refused the credentials

  Scenario: A backup with the wrong password fails, saying the password was not accepted
    Given the agent has the wrong password for the repository
    When the agent backs up the plan
    Then the operation ends as failed because the repository's password was not accepted
    And the repository has no snapshots
