Feature: Forget keeps each plan's history within its policy
  Forget is the maintenance owner's to run. It applies each plan's retention
  to that plan's own snapshots and removes only what the policy lets go:
  untagged history and history another agent's plan holds survive untouched
  (I6), and a policy that would keep nothing is refused before anything is
  removed. A repository runs one maintenance operation at a time (I7), and
  the slot is taken before the engine is started.

  Scenario: Forget removes only what the plan's retention lets go
    Given a plan "home" keeping the last 2 snapshots
    And the repository holds 4 snapshots of the plan "home"
    When the owner forgets the repository
    Then the operation ends as succeeded
    And the repository keeps the 2 newest snapshots of the plan "home"
    And the log shows the forget removed 2 snapshots

  Scenario: Untagged history survives a forget
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 2 snapshots no plan owns
    When the owner forgets the repository
    Then the operation ends as succeeded
    And the repository keeps the newest snapshot of the plan "home"
    And the repository still holds the 2 snapshots no plan owns

  Scenario: History another agent's plan holds survives a forget
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 2 snapshots another agent's plan "home" holds
    When the owner forgets the repository
    Then the operation ends as succeeded
    And the repository keeps the newest snapshot of the plan "home"
    And the repository still holds the 2 snapshots the other agent's plan holds

  Scenario: Each plan keeps its own policy
    Given a plan "home" keeping the last 2 snapshots
    And a plan "photos" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 3 snapshots of the plan "photos"
    When the owner forgets the repository
    Then the operation ends as succeeded
    And the repository keeps the 2 newest snapshots of the plan "home"
    And the repository keeps the newest snapshot of the plan "photos"

  Scenario: A policy that keeps nothing is refused
    Given a plan "home" keeping nothing
    And the repository holds 3 snapshots of the plan "home"
    When the owner forgets the repository
    Then the forget is refused because the plan's policy keeps nothing
    And the repository still holds the 3 snapshots of the plan "home"

  Scenario: A policy that keeps nothing refuses only where history exists
    Given a plan "home" keeping the last 1 snapshot
    And a plan "empty" keeping nothing that has never backed up
    And the repository holds 3 snapshots of the plan "home"
    When the owner forgets the repository
    Then the operation ends as succeeded
    And the repository keeps the newest snapshot of the plan "home"

  Scenario: A prune runs as maintenance of its own
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner prunes the repository
    Then the operation ends as succeeded
    And the engine was asked to prune the repository

  Scenario: A check runs as maintenance of its own
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner checks the repository
    Then the operation ends as succeeded
    And the engine was asked to check the repository

  Scenario: A cancel made before the engine is up beats the result it already has
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner checks the repository after cancelling it
    Then the operation ends as cancelled

  Scenario: A refused forget leaves nothing in the log
    Given a plan "home" keeping nothing
    And the repository holds 3 snapshots of the plan "home"
    When the owner forgets the repository
    Then the forget is refused because the plan's policy keeps nothing
    And the log holds no operation of this forget

  Scenario: An engine that cannot be reached refuses the forget
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the engine cannot be reached
    When the owner forgets the repository
    Then the forget is refused because the engine cannot be reached
    And the engine was never asked to remove anything

  Scenario: A listing that fails refuses the forget
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository's listing fails
    When the owner forgets the repository
    Then the forget is refused because the listing failed
    And the engine was never asked to remove anything

  Scenario: A policy the retention evaluator refuses fails the forget
    Given a plan "home" keeping within 1 day
    And the repository holds 3 snapshots of the plan "home"
    When the owner forgets the repository
    Then the forget is refused because the policy is not one the engine accepts
    And the engine was never asked to remove anything

  Scenario: An empty policy keeps everything
    Given a plan "home" with no policy set
    And the repository holds 3 snapshots of the plan "home"
    When the owner forgets the repository
    Then the operation ends as succeeded
    And the repository still holds the 3 snapshots of the plan "home"

  Scenario: A forget holds the repository's maintenance slot
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner forgets the repository and a prune is asked while it runs
    Then the operation ends as succeeded
    And the prune is refused while the forget holds the repository

  Scenario: A refused forget starts no forget
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And a maintenance operation is already running on the repository
    When the owner forgets the repository
    Then the forget is refused while the other operation runs
    And the engine was never asked to remove anything

  Scenario: A forget that is only assessed holds the repository for no one
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner assesses a forget of the repository
    Then the log holds no operation of this forget
    And a prune of the repository can start
    And the assessed forget is refused when it starts

  @e2e
  Scenario: A real forget keeps the newest snapshot and the untagged one
    Given a new repository
    And a plan "home" keeping the last 1 snapshot
    And the plan has been backed up 3 times
    And the folder was backed up without Ladderback
    When the owner forgets the repository
    Then the operation ends as succeeded
    And the repository has 2 snapshots
    And the remaining snapshot of the plan is the newest of its 3
    And the snapshot made without Ladderback is still there

  Scenario: A forget of more snapshots than one engine job can name runs as several jobs
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 12000 snapshots of the plan "home"
    When the owner forgets the repository
    Then the operation ends as succeeded
    And the repository keeps the newest snapshot of the plan "home"
    And no forget job names more snapshots than an engine job takes
    And the log shows the forget removed 11999 snapshots

  Scenario: A forget that fails partway says how much it had removed
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 12000 snapshots of the plan "home"
    And the engine fails the second forget job
    When the owner forgets the repository
    Then the operation ends as failed because the engine reported "the forget failed"
    And the operation lists 1 warning
    And the engine was given 2 forget jobs

  Scenario: A forget cancelled between its jobs starts no more of them
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 12000 snapshots of the plan "home"
    And the engine holds the second forget job
    When the owner forgets the repository and cancels it while the second job runs
    Then the operation ends as cancelled
    And the operation lists 1 warning
    And the engine was given 2 forget jobs

  Scenario: A repository that never answers refuses the forget instead of holding the request
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository never answers
    When the owner forgets the repository
    Then the forget is refused because the repository did not answer in time
    And the log holds no operation of this forget
