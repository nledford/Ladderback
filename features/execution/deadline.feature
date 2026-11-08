Feature: Backup deadline
  A backup must write its snapshot before a prune elsewhere can delete data
  that the index it read still points to (invariant I3). A backup still
  running at its deadline stops without writing a snapshot and fails.

  Scenario: An operation still running at its deadline fails
    Given the engine will complete the operation in 10 steps of 1 minute each
    And the operation has 3 minutes before its deadline
    When the agent runs the operation
    Then the operation ends as failed because its deadline passed
    And the operation took 3 minutes
    And the history shows the operation as it ended

  Scenario: An operation that finishes in time succeeds
    Given the engine will complete the operation in 2 steps of 1 minute each
    And the operation has 3 minutes before its deadline
    When the agent runs the operation
    Then the operation ends as succeeded
    And the history shows the operation as it ended

  @e2e
  Scenario: A worker with no time left stops before backing up anything
    Given a new repository
    And a plan "home" backing up a folder with 3 files
    And backups have no time left before a prune
    When the agent backs up the plan
    Then the operation ends as failed because its deadline passed
    And the repository has no snapshots
    And the history shows the operation as it ended
