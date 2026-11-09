Feature: Hooks
  A plan, a repository and the agent can each wrap an operation with hook
  commands. They run in rustic's order (agent, repository, plan before the
  operation; plan, repository, agent after it; finally-hooks either way),
  with a scrubbed environment that carries no secret (I4).

  Scenario: Before-hooks run before the backup and after-hooks after it, in order
    Given a before hook at the agent level that succeeds
    And a before hook at the repository level that succeeds
    And a before hook at the plan level that succeeds
    And an after hook at the agent level that succeeds
    And an after hook at the repository level that succeeds
    And an after hook at the plan level that succeeds
    When the agent runs the operation
    Then the operation ends as succeeded
    And the hooks ran in the order "agent-before,repository-before,plan-before,plan-after,repository-after,agent-after"

  Scenario: A failing before-hook with on-failure error stops the backup
    Given a before hook at the plan level that exits 3 with on-failure error
    And an after hook at the plan level that succeeds
    When the agent runs the operation
    Then the operation ends as failed because a hook failed with exit code 3
    And the operation never started
    And the after hook at the plan level never ran

  Scenario: A failing before-hook with on-failure warn is a warning
    Given a before hook at the plan level that exits 3 with on-failure warn
    When the agent runs the operation
    Then the operation ends as succeeded with warnings
    And the operation lists 1 warning

  Scenario: A failing before-hook with on-failure ignore is not reported
    Given a before hook at the plan level that exits 3 with on-failure ignore
    When the agent runs the operation
    Then the operation ends as succeeded
    And the operation lists 0 warnings

  Scenario: The failed-hooks run when the backup fails, and the finally-hooks either way
    Given the engine will report the failure "demo failure"
    And a failed hook at the plan level that succeeds
    And a finally hook at the plan level that succeeds
    And an after hook at the plan level that succeeds
    When the agent runs the operation
    Then the operation ends as failed because the engine reported "demo failure"
    And the failed hook at the plan level ran
    And the finally hook at the plan level ran
    And the after hook at the plan level never ran

  Scenario: The finally-hooks run after a successful backup too
    Given a finally hook at the plan level that succeeds
    When the agent runs the operation
    Then the operation ends as succeeded
    And the finally hook at the plan level ran

  Scenario: A failing after-hook does not fail a finished backup
    Given an after hook at the plan level that exits 4 with on-failure error
    When the agent runs the operation
    Then the operation ends as succeeded with warnings
    And the operation lists 1 warning

  @e2e
  Scenario: A hook that runs past the timeout is stopped
    Given a before hook at the plan level that never ends, with on-failure error
    When the agent runs the operation
    Then the operation ends as failed because a hook timed out
    And the operation never started

  @e2e
  Scenario: A hook sees its context but no secret of the daemon
    Given a before hook at the plan level that writes its environment to the scratch dir
    When the agent runs the operation
    Then the operation ends as succeeded
    And the hook's environment names LADDERBACK_HOOK and LADDERBACK_PLAN and no variable the worker's allow-list forbids
