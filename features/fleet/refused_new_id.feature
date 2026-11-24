Feature: A machine its hub refused as a second computer can take a new machine ID
  The hub takes one computer under each machine ID and refuses a second one
  as `identity-conflict`, signed over the connection (decision 150). A copy
  of a data folder made before the machine identifier was kept recorded its
  own computer's identifier at its first start, so the check at start can't
  tell it (decision 144): only the hub's refusal does. Once the machine
  holds that refusal, verified as signed by the hub it paired with, the
  person may take a new machine ID on it, as on a copied data folder: it
  gives up maintaining every repository first (I1), and uses the new ID
  from its next start. A refusal never takes one by itself, and one the
  paired hub didn't sign unlocks nothing. The refused computer may be the
  original, so nothing says it is a copy, and it maintains as before: its
  hub grants it no lease while it refuses it. Once the hub takes the
  computer again, after a person at the hub forgets the computer it took,
  the machine stands as before (technical design §7.1, §7.2; §16, decisions
  144 and 150; #1462).

  Background:
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository's maintenance is "local"
    And an agent paired with a hub
    And a person approved the agent on the hub
    And the agent connected to its hub
    And a copy of the agent's data folder on another computer
    And the copy recorded its own computer's machine identifier at its first start
    And the copy ran 1 backup

  Scenario: A machine its hub refused, signed, can take a new machine ID
    When the copy syncs with its hub
    Then the copy is refused because another machine holds its machine ID
    And the copy stands refused by its hub as a second computer
    And the copy maintains the repositories its configuration asks it to
    When the person takes a new machine ID on the copy
    Then the copy has a new machine ID
    And the repository's maintenance is now "none"
    And the copy's maintenance stays refused until it starts again

  Scenario: A refusal its hub didn't sign takes no new machine ID
    Given the hub's refusals reach the copy with a signature the hub didn't make
    When the copy syncs with its hub
    Then the copy's sync fails as one its hub didn't sign
    And the copy stands as usual
    When the person takes a new machine ID on the copy
    Then taking a new machine ID on the copy is refused
    And the copy's machine ID is unchanged
    And the repository's maintenance is now "local"

  Scenario: Once the hub takes the computer again, the machine stands as before
    When the copy syncs with its hub
    Then the copy stands refused by its hub as a second computer
    When a person at the hub forgets the computer it took for the agent
    And the copy syncs with its hub
    Then the hub holds the copy's 2 reports, each once, in the order they happened
    And the copy stands as usual
    When the person takes a new machine ID on the copy
    Then taking a new machine ID on the copy is refused
    And the copy's machine ID is unchanged
