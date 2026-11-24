Feature: A machine its hub removed or rejected can take a new machine ID and ask again
  A person removes an approved machine, or rejects a waiting one, on the
  hub's page, and the hub keeps a tombstone of its agent ID and key: it
  answers the machine's next connection, signed, that it removed it, and
  never takes that ID or that key again (decision 154). Once the machine
  holds that refusal, verified as signed by the hub it paired with, the
  person may take a new machine ID on it, as on a machine its hub refused
  as a second computer: it gives up maintaining every repository first
  (I1), makes a new agent key, and uses the new ID from its next start. It
  then pairs again with a new token and waits for approval under the new ID
  and key, so the hub's owner decides again and the removal holds until
  they approve. A refusal the hub didn't sign unlocks nothing, and the old
  ID and key stay refused (technical design §7.1; §16, decisions 150 and
  154; #1489).

  Background:
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository's maintenance is "local"
    And an agent paired with a hub
    And the agent started on the computer its data folder was made on

  Scenario: A machine its hub removed takes a new machine ID and key, asks again and is approved
    Given a person approved the agent on the hub
    And the agent connected to its hub
    And the agent ran 1 backup
    And the person removed "laptop" on the hub's page
    When the agent tries to sync with its hub
    Then the agent's sync is refused because its hub removed it
    And the agent stands removed by its hub
    And the agent maintains only under a lease from its hub, and holds none
    When the person takes a new machine ID
    Then the agent has a new machine ID
    And the agent holds a new agent key
    And the repository's maintenance is now "none"
    And the agent's maintenance stays refused until it starts again
    When the agent starts again under its new machine ID
    And the agent pairs again with a new token
    Then the hub lists the agent under its hostname as waiting for approval
    And the hub holds the agent under a key other than the one it removed
    When a person approves the agent on the hub
    And the agent syncs with its hub
    Then the hub holds the agent's 2 reports, each once, in the order they happened
    And the agent stands as usual
    And the agent maintains the repositories its configuration asks it to
    When the agent's old machine ID connects to its hub
    Then the hub tells the agent it removed it

  Scenario: A machine its hub rejected while it waited takes a new machine ID and asks again
    Given the person rejected "laptop" on the hub's page
    When the agent tries to sync with its hub
    Then the agent's sync is refused because its hub removed it
    And the agent stands removed by its hub
    When the person takes a new machine ID
    Then the agent has a new machine ID
    And the agent holds a new agent key
    When the agent starts again under its new machine ID
    And the agent pairs again with a new token
    Then the hub lists the agent under its hostname as waiting for approval
    When a person approves the agent on the hub
    And the agent syncs with its hub
    Then the agent stands as usual
    When the agent's old machine ID connects to its hub
    Then the hub tells the agent it removed it

  Scenario: A removed machine's key is refused under a new machine ID
    Given a person approved the agent on the hub
    And the person removed "laptop" on the hub's page
    When a machine holding the removed agent's key pairs under a new machine ID with a new token
    Then the pairing is refused because the hub removed the agent's key
    And the hub lists no agent
