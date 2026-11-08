Feature: Operation outcome
  An operation's outcome reflects what the engine reported, so a run with
  unreadable files is never mistaken for a clean one or a failed one.

  Scenario: An operation without warnings succeeds
    Given the engine will complete the operation in 3 steps of 1 minute each
    When the agent runs the operation
    Then the operation ends as succeeded
    And the operation took 3 minutes
    And the history shows the operation as it ended

  Scenario: Unreadable files don't fail an operation
    Given the engine will report 2 unreadable files
    When the agent runs the operation
    Then the operation ends as succeeded with warnings
    And the operation lists 2 warnings
    And the history shows the operation as it ended

  Scenario: A failure reported by the engine fails the operation
    Given the engine will report the failure "repository not found"
    When the agent runs the operation
    Then the operation ends as failed because the engine reported "repository not found"
    And the history shows the operation as it ended

  Scenario: An engine that stops without a result fails the operation
    Given the engine will stop after 1 step with exit code 3
    When the agent runs the operation
    Then the operation ends as failed because the engine exited with code 3
    And the history shows the operation as it ended

  Scenario: An engine that can't start fails the queued operation
    Given the engine can't be started
    When the agent runs the operation
    Then the operation ends as failed because the engine could not start
    And the operation never started
    And the history shows the operation as it ended

  Scenario: Warnings past the cap are counted but not kept
    Given the engine will report 10005 unreadable files
    When the agent runs the operation
    Then the operation ends as succeeded with warnings
    And the history lists 10000 warnings for the operation
    And the history counts 10005 warnings for the operation
