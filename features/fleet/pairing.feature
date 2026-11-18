Feature: Pairing an agent with its hub
  An agent and its hub keep their keys in key files in the data folder,
  never in the database. Anyone holding the secret ones can pose as the hub
  or the agent, so Ladderback's own backups leave them out: a plan over the
  data folder backs up the rest of it and never puts a key in a repository
  another machine can read (technical design §7.1; §16, decisions 120 and
  122). The snapshot is read back with rustic itself, not through
  Ladderback.

  @e2e
  Scenario: A hub's key files stay out of a backup by default
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's source is Ladderback's data folder, which holds the hub's and the agent's key files
    When the daemon has finished 1 backup of the plan "home"
    Then the history page shows the operation succeeded
    And the backup's snapshot leaves out the hub key, the hub's TLS key, the recovery seed and the agent key
    And the backup's snapshot holds the rest of the data folder
