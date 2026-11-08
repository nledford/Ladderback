Feature: Cancelling an operation
  Cancelling stops the engine. An operation the user cancelled is recorded
  as cancelled, never as failed, and a cancelled backup writes no snapshot.

  Scenario: A cancelled operation is recorded as cancelled
    Given the engine will complete the operation in 10 steps of 1 minute each
    When the agent runs the operation and the user cancels it after 3 minutes
    Then the operation ends as cancelled
    And the operation took 3 minutes
    And the history shows the operation as it ended

  @e2e
  Scenario: Cancelling a backup stops its worker and writes no snapshot
    Given a new repository
    And a plan "home" backing up a folder with a 4 GiB file
    When the agent backs up the plan and the user cancels it once it has started
    Then the operation ends as cancelled
    And the repository has no snapshots
    And the history shows the operation as it ended
