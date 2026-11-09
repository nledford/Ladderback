Feature: Adopt gives history this agent doesn't own to a plan of its own
  Adoption is the maintenance owner's to run. It rewrites snapshots the
  configuration doesn't own — another agent's tagged history, or untagged
  history that matches a plan's host, label and sources — with that plan's
  tags, as new snapshot files over the same trees (§7.3). Nothing is
  removed but the snapshots it rewrote, nothing a plan of this agent holds
  is ever rewritten, and a group no plan matches is refused before
  anything is written. A repository runs one maintenance operation at a
  time (I7), and the slot is taken before the engine is started.

  Scenario: An orphaned group is adopted into a plan of this agent
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots another agent's plan "home" holds
    When the owner adopts the other agent's "home" history into the plan "home"
    Then the operation ends as succeeded
    And the 3 snapshots are rewritten with the tags of the plan "home"
    And the snapshots the other agent held are gone

  Scenario: Adopting one group leaves every other snapshot alone
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 2 snapshots another agent's plan "home" holds
    And the repository holds 2 snapshots no plan owns
    When the owner adopts the other agent's "home" history into the plan "home"
    Then the operation ends as succeeded
    And the 2 snapshots are rewritten with the tags of the plan "home"
    And the repository still holds the 3 snapshots of the plan "home"
    And the repository still holds the 2 snapshots no plan owns

  Scenario: An unmanaged group that matches a plan is adopted
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 2 snapshots no plan owns that match the plan "home"
    When the owner adopts the unmanaged history that matches the plan "home"
    Then the operation ends as succeeded
    And the 2 snapshots are rewritten with the tags of the plan "home"
    And the snapshots no plan owned are gone

  Scenario: An unmanaged group no plan matches is refused
    Given a plan "photos" keeping the last 1 snapshot
    And the repository holds 2 snapshots no plan owns
    When the owner adopts the unmanaged history into the plan "photos"
    Then the adoption is refused because no plan matches that history
    And the repository still holds the 2 snapshots no plan owns
    And the engine was never asked to rewrite anything

  Scenario: Two plans that both match one history are refused
    Given a plan "home" keeping the last 1 snapshot
    And a plan "mirror" keeping the last 1 snapshot that backs up the same sources
    And the repository holds 2 snapshots no plan owns
    When the owner adopts the unmanaged history into the plan "home"
    Then the adoption is refused because two plans match that history
    And the repository still holds the 2 snapshots no plan owns
    And the engine was never asked to rewrite anything

  Scenario: An adoption holds the repository's maintenance slot
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 2 snapshots another agent's plan "home" holds
    When the owner adopts the other agent's "home" history into the plan "home" and a forget is asked while it runs
    Then the operation ends as succeeded
    And the forget is refused while the adoption holds the repository

  Scenario: A refused adoption leaves nothing in the log
    Given a plan "photos" keeping the last 1 snapshot
    And the repository holds 2 snapshots no plan owns
    When the owner adopts the unmanaged history into the plan "photos"
    Then the adoption is refused because no plan matches that history
    And the log holds no operation of this adoption

  @e2e
  Scenario: A real adoption makes new snapshots with the same trees
    Given a new repository
    And a plan "home" keeping the last 1 snapshot
    And the folder was backed up without Ladderback
    When the owner adopts the unmanaged history that matches the plan "home"
    Then the operation ends as succeeded
    And every snapshot of the folder carries this agent's and the plan's tags
    And no snapshot made without Ladderback is left
