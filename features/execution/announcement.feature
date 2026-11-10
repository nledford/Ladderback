Feature: Announcing how an operation ended
  A page is told how an operation ended (technical design §4.4) only once
  the history can say it too: a page that reads the history on hearing the
  news finds the end there. A log that can't take the end doesn't hold the
  news back. The page hears it all the same, and the history shows what the
  log last had.

  Background:
    Given a page that reads the history whenever it hears an operation ended

  Scenario: A page that hears an operation succeeded finds it succeeded in the history
    Given the engine will complete the operation in 3 steps of 1 minute each
    When the agent runs the operation
    Then the page hears the operation succeeded
    And the history the page read holds the operation as it ended

  Scenario: A page that hears an operation failed finds it failed in the history
    Given the engine will report the failure "repository not found"
    When the agent runs the operation
    Then the page hears the operation failed with the code "operation-failed"
    And the history the page read holds the operation as it ended

  Scenario: A page that hears an engine stopped without a result finds the failure in the history
    Given the engine will stop after 1 step with exit code 3
    When the agent runs the operation
    Then the page hears the operation failed with the code "engine-exited"
    And the history the page read holds the operation as it ended

  Scenario: A page hears nothing of a cancelled operation
    Given the engine will complete the operation in 10 steps of 1 minute each
    When the agent runs the operation after the user has cancelled it
    Then the operation ends as cancelled
    And the page has heard nothing

  Scenario Outline: A log that can't take the end doesn't keep the page from hearing it
    Given the log <fault> the write of the operation's end
    And the engine will complete the operation in 3 steps of 1 minute each
    When the agent runs the operation
    Then the page hears the operation succeeded
    And the history the page read holds the operation as running
    And the run reports a write the log did not take

    Examples:
      | fault         |
      | refuses       |
      | never answers |

  Scenario: A page that hears a forget succeeded finds it succeeded in the history
    Given a plan "home" keeping the last 2 snapshots
    And the repository holds 4 snapshots of the plan "home"
    When the owner forgets the repository
    Then the page hears the operation succeeded
    And the history the page read holds the operation as it ended

  Scenario: A forget that runs as several jobs is heard to end once, when all of them have
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 12000 snapshots of the plan "home"
    When the owner forgets the repository
    Then the page hears the operation succeeded
    And the history the page read holds the operation as it ended
