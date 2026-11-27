Feature: Maintenance takes its turn under a limit of its own
  Forget, prune, check and adoption take their turn in a queue of their
  own, apart from the backups' (decision 75, point 8). The agent runs at
  most `maintenance-concurrency` of them at once across its repositories,
  one unless the configuration says more, so that many started together,
  by a schedule or by a person, don't all run at the same time, each in a
  worker with its own memory and I/O. A run beyond the limit waits: the
  history shows it queued, not running, until its turn comes, and a cancel
  ends it at once. The limit counts maintenance only, so a backup never
  waits for a prune, nor a prune for a backup.

  A repository still runs one maintenance operation at a time (I7): a run
  started while one of its repository's is running or waiting is refused,
  naming that one, as before. A run takes its turn first and its
  repository's slot when its turn comes, and a turn that finds the run can
  no longer go ahead, as when this machine stopped maintaining the
  repository meanwhile, ends it as failed with why, having run nothing.

  A restart that waits for the running operations holds the queue: no
  waiting run starts, and the restart waits only for the ones running. The
  next start ends each run still queued as failed because the daemon
  stopped, as it ends any queued maintenance (decision 73).

  Background:
    Given the repositories "nas" and "laptop" are maintained by this agent

  Scenario: With one at a time, a second repository's run waits its turn and then runs
    Given the agent runs 1 maintenance operation at a time
    When a prune of the repository "nas" is started
    And a check of the repository "laptop" is started
    Then the prune of the repository "nas" is running
    And the check of the repository "laptop" is waiting for its turn
    And the history shows the check of the repository "laptop" as queued
    When the prune of the repository "nas" finishes
    Then the check of the repository "laptop" is running
    When the check of the repository "laptop" finishes
    Then the history shows the check of the repository "laptop" as succeeded

  Scenario: With two at a time, both repositories' runs run at once
    Given the agent runs 2 maintenance operations at a time
    When a prune of the repository "nas" is started
    And a check of the repository "laptop" is started
    Then the prune of the repository "nas" is running
    And the check of the repository "laptop" is running

  Scenario: A backup takes its turn while maintenance waits for its own
    Given the agent runs 1 maintenance operation at a time
    And the agent runs 1 backup at a time
    When a prune of the repository "nas" is started
    And a check of the repository "laptop" is started
    And a backup of the plan "home" is started
    Then the backup takes its turn at once
    And the check of the repository "laptop" is waiting for its turn

  Scenario: A waiting run that is canceled ends at once and never runs
    Given the agent runs 1 maintenance operation at a time
    When a prune of the repository "nas" is started
    And a check of the repository "laptop" is started
    And the check of the repository "laptop" is canceled
    Then the history shows the check of the repository "laptop" as canceled
    When the prune of the repository "nas" finishes
    Then the engine never ran a check of the repository "laptop"

  Scenario Outline: A run started while its repository's maintenance is <going> is refused, naming it
    Given the agent runs 1 maintenance operation at a time
    When a prune of the repository "nas" is started
    And a check of the repository "laptop" is started
    And a prune of the repository "<repository>" is started
    Then the start is refused, naming the <named> of the repository "<repository>"
    And the history doesn't list the refused prune

    Examples:
      | going                | repository | named |
      | running              | nas        | prune |
      | waiting for its turn | laptop     | check |

  Scenario: A turn that finds the repository no longer maintained here ends the run as failed
    Given the agent runs 1 maintenance operation at a time
    When a prune of the repository "nas" is started
    And a check of the repository "laptop" is started
    And this agent stops maintaining the repository "laptop"
    And the prune of the repository "nas" finishes
    Then the history shows the check of the repository "laptop" as failed with the code "maintenance-not-owner"
    And the engine never ran a check of the repository "laptop"

  Scenario: A restart while a run waits starts it no more and waits only for the running one
    Given the agent runs 1 maintenance operation at a time
    When a prune of the repository "nas" is started
    And a check of the repository "laptop" is started
    And a restart holds the maintenance queue
    Then 1 maintenance operation is counted as running
    When the prune of the repository "nas" finishes
    Then the check of the repository "laptop" is waiting for its turn
    And 0 maintenance operations are counted as running
    When the daemon starts again
    Then the history shows the check of the repository "laptop" as failed with the code "daemon-stopped"
