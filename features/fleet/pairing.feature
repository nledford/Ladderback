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

  @e2e
  Scenario: A plan's own iglob file can't bring a key file back into a backup
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's source is Ladderback's data folder, which holds the hub's and the agent's key files
    And the plan's iglob file takes in every file
    When the daemon has finished 1 backup of the plan "home"
    Then the history page shows the operation succeeded
    And the backup's snapshot leaves out the hub key, the hub's TLS key, the recovery seed and the agent key
    And the backup's snapshot holds the rest of the data folder

  Scenario: An agent pairs with a hub and waits for approval
    Given a hub with a pairing token
    When an agent pairs with the token
    Then the hub lists the agent under its hostname as waiting for approval
    And the token can't be used again
    And the agent pins the hub's key, its certificate and its recovery key

  Scenario: A token that has lapsed is refused
    Given a hub with a pairing token that has lapsed
    When an agent pairs with the token
    Then the pairing is refused as expired
    And the hub lists no agent

  Scenario: A token that was revoked is refused
    Given a hub with a pairing token
    When the person revokes the token
    And an agent pairs with the token
    Then the pairing is refused as revoked

  Scenario: A new hub key voids every unused token
    Given a hub with a pairing token
    When the hub starts again without its key files and makes new keys
    Then the hub lists no unused token
    When a client that doesn't check the hub's key pairs with the token
    Then the pairing is refused because the hub's key isn't the token's
    And the hub lists no agent

  Scenario: A pairing whose answer was lost can be repeated
    Given an agent that paired but never received the hub's answer
    When it pairs again with the same token
    Then the hub answers it as before and lists one agent

  Scenario: An agent that pairs again under another hostname is told which one the hub holds
    Given an agent paired under the hostname "laptop"
    And a pairing token
    When the same agent pairs with the token under the hostname "desktop"
    Then the agent is told the hub holds it under "laptop"
    And the hub lists one agent, under the hostname "laptop"

  Scenario: A hostname that is taken is refused and the token stays good
    Given an agent paired under the hostname "laptop"
    And a pairing token
    When a second agent named "laptop" pairs with the token
    Then the pairing is refused because the hostname is taken
    And the refusal names the agent that holds it
    When the second agent is renamed "desktop" and pairs with the same token
    Then the hub lists both agents

  Scenario: A name that differs only by normalization is taken
    Given an agent paired under the hostname "café" in one Unicode form
    And a pairing token
    When a second agent named "café" in the other Unicode form pairs with the token
    Then the pairing is refused because the hostname is taken

  Scenario: A lookalike hostname is flagged for the person
    Given an agent paired under the hostname "laptop"
    When another agent pairs as "lаptop" with a Cyrillic letter
    Then the hub lists it as waiting for approval
    And the hub says the name looks like "laptop"

  Scenario: A lookalike that differs by case is flagged
    Given an agent paired under the hostname "Laptop"
    When another agent pairs as "lаptop" with a Cyrillic letter
    Then the hub says the name looks like "Laptop"

  Scenario: A name in another script that looks like no other isn't flagged
    Given an agent paired under the hostname "laptop"
    When another agent pairs as "ノートパソコン"
    Then the hub lists it as waiting for approval
    And the hub says the name looks like no other

  Scenario: A registered agent can't be taken over with another key
    Given an agent paired under the hostname "laptop"
    And a pairing token
    When another machine pairs with the token under the first agent's ID
    Then the pairing is refused because the ID belongs to another key

  Scenario: An agent sends nothing of the token to a hub that isn't the token's
    Given a hub with a pairing token
    When another hub answers at the token's address and the agent pairs with the token
    Then the agent stops because the hub's key isn't the token's
    And the agent sent nothing signed with the token
    And the agent pins no hub

  Scenario: A twenty-first agent waiting for approval is refused and the token stays good
    Given a hub with 20 agents waiting for approval
    And a pairing token
    When a second agent named "one-more" pairs with the token
    Then the pairing is refused because too many agents wait for approval
    And the token is still unused

  Scenario: An agent pairs over its connection with a token pasted between quotes
    Given a hub with a pairing token
    When an agent pairs over its connection with the token pasted between quotes
    Then the hub lists the agent under its hostname as waiting for approval
    And the agent pins the hub's key, its certificate and its recovery key

  Scenario: A token for a plain http address on another machine is refused before connecting
    Given a hub with a pairing token for the address "http://hub.example:9898"
    When an agent pairs over its connection with the token
    Then the agent stops because the address would show the token to the network
    And the agent connected to nothing
    And the hub lists no agent

  Scenario: An agent whose hub doesn't answer pins nothing
    Given a hub with a pairing token
    When an agent pairs over its connection with the token and nothing answers
    Then the agent stops because the hub can't be reached
    And the agent pins no hub
    And the token is still unused
