Feature: A paired agent connecting to its hub
  Every connection after pairing starts with both sides proving who they
  are: the hub first, with the key the agent pinned at pairing, then the
  agent, with the agent key the hub registered. Only then does the hub say
  whether it holds the agent pending or has approved it (technical design
  §7.1, §7.2; §16, decision 120, "The handshake" and "Signed messages after
  the handshake").

  Scenario: A paired agent connects and is told it waits for approval
    Given an agent paired with a hub
    When the agent connects to its hub
    Then the hub accepts the agent's proof
    And the agent is told it waits for approval

  Scenario: An approved agent connects and is told the hub approved it
    Given an agent paired with a hub
    And a person approved the agent on the hub
    When the agent connects to its hub
    Then the agent is told the hub approved it

  Scenario: The hub keeps the counter of the agent's latest connection
    Given an agent paired with a hub
    When the agent connects to its hub twice
    Then the hub holds the counter of the agent's second connection

  Scenario: An agent refuses a hub whose key isn't the one it trusts
    Given an agent paired with a hub
    When another hub that has another key answers its connection
    Then the agent stops because the hub's key isn't the one it trusts
    And the agent sent no proof

  Scenario: A hub refuses the proof of an agent it never registered
    Given an agent paired with a hub
    When a machine that holds the same pin but never paired connects to the hub
    Then the hub refuses the machine's proof
    And the hub lists only the paired agent

  Scenario: A hub refuses a proof made with another key under a registered ID
    Given an agent paired with a hub
    When a machine with another key connects under the agent's ID
    Then the hub refuses the machine's proof
    And the hub still holds the agent's key
