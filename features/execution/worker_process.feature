@e2e
Feature: Operations run in a worker process
  Each operation runs in its own `steading worker` process, so a crash in
  the engine fails that one operation and never takes the agent down.

  Scenario: A worker reports warnings and a result
    Given the engine will report 2 unreadable files
    When the agent runs the operation
    Then the operation ends as succeeded with warnings
    And the operation lists 2 warnings

  Scenario: A failure reported by a worker fails the operation
    Given the engine will report the failure "repository not found"
    When the agent runs the operation
    Then the operation ends as failed because the engine reported "repository not found"

  Scenario: A worker that crashes fails the operation
    Given the engine will stop after 1 step with exit code 3
    When the agent runs the operation
    Then the operation ends as failed because the engine exited with code 3

  Scenario: A worker that can't be started fails the operation
    Given the engine can't be started
    When the agent runs the operation
    Then the operation ends as failed because the engine could not start
    And the operation never started
