@e2e @maintenance
Feature: The Snapshots page asks the daemon for a repository's history and for a preview of each maintenance run
  A repository's history is shown grouped by who owns it: the plans of this
  agent, history another agent's or a forgotten plan's snapshots hold
  (orphaned), and history made without Ladderback (unmanaged). Before a
  forget, an adoption, a prune or a check starts, the owner previews it: the
  preview names what the run would do and changes nothing, takes no
  maintenance slot and leaves nothing in the operation log. A forget or an
  adoption that is confirmed names the selection its preview showed, and a
  repository that has changed since is refused instead of acted on (I1, I6).

  Background:
    Given Ladderback is serving on loopback with the plan "home"

  Scenario: In the running daemon, the groups list a plan's snapshots newest first
    Given the daemon holds 2 snapshots of the plan "home"
    When the owner asks for the repository's groups
    Then the groups list the plan "home" with 2 snapshots newest first

  Scenario: In the running daemon, a plan with no snapshots is listed with none
    When the owner asks for the repository's groups
    Then the groups list the plan "home" with no snapshots
    And the groups name no orphaned and no unmanaged history

  Scenario: In the running daemon, another agent's history is orphaned and adoptable into the plan
    Given the plan has been backed up
    When the owner asks for the repository's groups
    Then the groups name one orphaned group of 1 snapshot, from the agent that made them and the plan "home", adoptable into the plan "home"

  Scenario: In the running daemon, untagged history is unmanaged and names the plan that would adopt it
    Given the folder was backed up without Ladderback
    When the owner asks for the repository's groups
    Then the groups name one unmanaged group of 1 snapshot, adoptable into the plan "home"

  Scenario: In the running daemon, the groups of a repository that is not configured are refused
    When the owner asks for the groups of the repository "nowhere"
    Then the refusal has status 404 and the typed code "repository-unknown"

  Scenario: In the running daemon, a forget preview names what goes and removes nothing
    Given the daemon holds 3 snapshots of the plan "home"
    And the daemon's plan keeps the last 1 snapshot
    When the owner previews forgetting
    Then the forget preview names 2 of the 3 snapshots of the plan "home" for removal, the oldest ones
    And the preview names a selection of 16 lower-case hex digits
    And the repository has 3 snapshots

  Scenario: In the running daemon, a forget preview names the history it leaves alone
    Given the plan has been backed up
    And the folder was backed up without Ladderback
    When the owner previews forgetting
    Then the forget preview leaves alone 1 orphaned group of 1 snapshot and 1 unmanaged group of 1 snapshot
    And the repository has 2 snapshots

  Scenario: In the running daemon, a forget preview refuses a policy that keeps nothing
    Given the daemon holds 1 snapshot of the plan "home"
    And the daemon's plan keeps nothing
    When the owner previews forgetting
    Then the refusal has status 422 and the typed code "retention-keeps-nothing"
    And the repository has 1 snapshot

  Scenario: In the running daemon, an adoption preview names the snapshots it would rewrite and rewrites none
    Given the plan has been backed up
    When the owner previews adopting orphaned history into the plan "home"
    Then the adoption preview names 1 snapshot to rewrite into the plan "home"
    And the preview names a selection of 16 lower-case hex digits
    And every snapshot in the repository still carries the tag of the agent that made it

  Scenario: In the running daemon, an adoption preview of unmanaged history refuses when no history matches
    When the owner previews adopting unmanaged history into the plan "home"
    Then the refusal has status 422 and the typed code "adoption-no-match"

  Scenario: In the running daemon, an adoption preview into a plan that is not configured is refused
    Given the folder was backed up without Ladderback
    When the owner previews adopting unmanaged history into the plan "nowhere"
    Then the refusal has status 404 and the typed code "plan-unknown"

  Scenario: In the running daemon, a prune preview reports what it would do and prunes nothing
    Given the daemon holds 2 snapshots of the plan "home"
    And the daemon has forgotten all but the last 1 snapshot
    And the names of the files in the repository's index and data folders are noted
    When the owner previews pruning
    Then the prune preview marks at least 1 pack
    And the prune preview waits 144 hours before anything marked is deleted
    And the repository's index and data files are as they were noted

  Scenario: In the running daemon, a check preview says it reads the repository's structure only and starts nothing
    When the owner previews a check
    Then the check preview reads the repository's structure only
    And the operation log holds no maintenance operation

  Scenario: In the running daemon, a forget confirmed with the preview's selection removes exactly what the preview listed
    Given the daemon holds 3 snapshots of the plan "home"
    And the daemon's plan keeps the last 1 snapshot
    When the owner previews forgetting
    And the owner confirms the forget with the preview's selection
    Then the confirmed forget runs to a success
    And the forget removed exactly the snapshots the preview listed and kept the newest

  Scenario: In the running daemon, a forget started with a stale selection is refused and removes nothing
    Given the daemon holds 3 snapshots of the plan "home"
    And the daemon's plan keeps the last 1 snapshot
    When the owner previews forgetting
    And the daemon makes another snapshot of the plan "home"
    And the owner confirms the forget with the preview's selection
    Then the refusal has status 409 and the typed code "maintenance-selection-changed"
    And the repository has 4 snapshots

  Scenario Outline: In the running daemon, a preview request it cannot take is refused: <what>
    When the owner previews maintenance of the repository "<alias>" with the request <body>
    Then the refusal has status <status> and the typed code "<code>"

    Examples:
      | what                        | alias   | body              | status | code                     |
      | a body that is not JSON     | demo    | not json          | 400    | invalid-body             |
      | a kind that is not run      | demo    | {"kind":"wipe"}   | 404    | maintenance-kind-unknown |
      | a repository not configured | nowhere | {"kind":"forget"} | 404    | repository-unknown       |

  Scenario Outline: In the running daemon, a start that names a selection it cannot hold is refused: <what>
    When the owner starts maintenance of the repository "demo" with the request <body>
    Then the refusal has status 400 and the typed code "invalid-body"

    Examples:
      | what                              | body                                                  |
      | a selection that is not a token   | {"kind":"forget","selection":"not-a-token"}           |
      | a selection on a prune            | {"kind":"prune","selection":"0123456789abcdef"}       |
