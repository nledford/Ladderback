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
    And the operation carries a hook warning from the before phase with detail "3"
    And the operation lists 1 warning

  # A before-hook that stopped the backup is named by its level and its place
  # in that level's run-before list, with how it ended, so a person can find
  # it in their configuration. What it wrote is not kept: it can hold a
  # secret no redactor knows (#480, #853).
  Scenario: A failing before-hook's failure says which hook it was and how it ended
    Given a before hook at the agent level that succeeds
    And a before hook at the plan level that succeeds
    And a second before hook at the plan level that exits 3 with on-failure error
    When the agent runs the operation
    Then the operation ends as failed because before-hook 2 at the plan level exited with status 3

  Scenario Outline: A failing before-hook's failure says how it ended without an exit status
    Given a before hook at the repository level that <ends>, with on-failure error
    When the agent runs the operation
    Then the operation ends as failed because before-hook 1 at the repository level <ended>

    Examples:
      | ends                  | ended                 |
      | a signal ends         | was ended by a signal |
      | runs out of time      | timed out             |
      | can't start           | could not be started  |

  # A page that is open hears a before-hook's warning as the hook ends, as
  # it hears an engine's, not only when it next reads the history; and it
  # hears it once (#369).
  Scenario: A page hears a before-hook's warning as it happens, once
    Given a page that reads the history whenever it hears an operation started or ended
    And a before hook at the plan level that exits 3 with on-failure warn
    When the agent runs the operation
    Then the page hears a hook warning from the before phase before it hears the operation started
    And the page hears 1 warning

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
    And the operation carries a hook warning from the after phase with detail "4"

  Scenario: An ignored failing after-hook attaches no warning
    Given an after hook at the plan level that exits 4 with on-failure ignore
    When the agent runs the operation
    Then the operation ends as succeeded
    And the operation carries no hook warning

  Scenario: A before-hook stop runs no failed-hooks
    Given a before hook at the plan level that exits 3 with on-failure error
    And a failed hook at the plan level that succeeds
    And a finally hook at the plan level that succeeds
    When the agent runs the operation
    Then the operation ends as failed because a hook failed with exit code 3
    And the failed hook at the plan level never ran
    And the finally hook at the plan level ran

  @e2e
  Scenario: A hook that runs past the timeout is stopped
    Given a before hook at the plan level that never ends, with on-failure error
    When the agent runs the operation
    Then the operation ends as failed because a hook timed out
    And the operation never started

  @e2e @serial
  Scenario: A hook that can't start in time is timed out
    Given a before hook at the plan level that can't start in time, with on-failure error
    When the agent runs the operation
    Then the operation ends as failed because a hook timed out
    And the operation never started

  @e2e
  Scenario: A hook sees its context but no secret of the daemon
    Given a before hook at the plan level that writes its environment to the scratch dir
    When the agent runs the operation
    Then the operation ends as succeeded
    And the hook's environment names LADDERBACK_HOOK and LADDERBACK_PLAN and no variable the worker's allow-list forbids

  # The daemon keeps the failure's code in the history, with which hook it
  # was and how it ended, and never what the hook wrote (#480).
  @e2e
  Scenario: The history keeps which before-hook stopped a backup and how it ended
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's before hook says "canary-hook-output" and exits 3, with on-failure error
    When the daemon starts a backup of the plan "home"
    Then the history page shows the operation failed
    And the history page gives the operation's failure as "hook-failed", for before-hook 1 at the plan level, which exited with status 3
    And neither the history page nor the operation's detail shows "canary-hook-output"

  @e2e
  Scenario: A connected page learns of a before-hook's warning before the backup ends
    Given Ladderback is serving on loopback with the plan "home"
    And a live-updates connection from the UI's own origin
    And the plan's before hook says "canary-hook-output" and exits 3, with on-failure warn
    When the daemon runs a backup of the plan "home"
    Then the connected page learns of a hook warning from the before phase before the operation finished
