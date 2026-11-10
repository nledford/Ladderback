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

  Scenario: An adoption of more snapshots than one engine job can name runs as several jobs
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 7000 snapshots another agent's plan "home" holds
    When the owner adopts the other agent's "home" history into the plan "home"
    Then the operation ends as succeeded
    And the 7000 snapshots are rewritten with the tags of the plan "home"

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

  Scenario: Adopting into a plan this agent doesn't have is refused
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 2 snapshots another agent's plan "home" holds
    When the owner adopts the other agent's "home" history into the plan "ghost"
    Then the adoption is refused because the plan "ghost" is not configured
    And the repository still holds the 2 snapshots the other agent's plan holds
    And the engine was never asked to rewrite anything

  Scenario: An unmanaged group no plan matches is refused
    Given a plan "photos" keeping the last 1 snapshot
    And the repository holds 2 snapshots no plan owns of another folder
    When the owner adopts the unmanaged history into the plan "photos"
    Then the adoption is refused because no plan matches that history
    And the repository still holds the 2 snapshots no plan owns
    And the engine was never asked to rewrite anything

  Scenario: Two plans that both match one history are refused
    Given a plan "home" keeping the last 1 snapshot
    And a plan "mirror" keeping the last 1 snapshot that backs up the same sources
    And the repository holds 2 snapshots no plan owns that match the plan "home"
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
    And the repository holds 2 snapshots no plan owns of another folder
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

  @e2e
  Scenario: A real adoption into a plan with history counts only what it rewrote
    Given a new repository
    And a plan "home" keeping the last 1 snapshot
    And the plan "home" already holds a snapshot of the folder
    And the folder was backed up without Ladderback with the tag "weekly"
    When the owner adopts the unmanaged history that matches the plan "home"
    Then the operation ends as succeeded
    And the adoption reports 1 snapshot rewritten
    And the adopted snapshot still carries the tag "weekly"

  @e2e
  Scenario: An adoption cut short after its copy is finished without a second copy
    Given a new repository
    And a plan "home" keeping the last 1 snapshot
    And the folder was backed up without Ladderback
    And an earlier adoption into the plan "home" saved its copy but left the original
    When the owner adopts the unmanaged history that matches the plan "home"
    Then the operation ends as succeeded
    And the adoption reports 1 snapshot rewritten
    And every snapshot of the folder carries this agent's and the plan's tags
    And no snapshot made without Ladderback is left
