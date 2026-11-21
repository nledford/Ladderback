Feature: A paired machine following its hub to a new address
  A hub's address changes: a new IP address, a new name. The person gives
  each machine the new address with `ladderback pair <address>`, and the
  machine keeps working without pairing again. It keeps the address only
  once the hub there has proved the key it pinned at pairing, and still
  tries the token's address when the new one doesn't answer (technical
  design §7.1; §16, decision 120, "Addresses and the transport statement",
  and decision 121, "From a terminal").

  Scenario: A machine follows its hub to a new address without pairing again
    Given an agent paired with a hub
    And the hub answers only at a new address
    When the person gives the agent the hub's new address
    Then the agent keeps the hub's new address
    When the agent connects to its hub
    Then the hub accepts the agent's proof
    And the agent tried the hub's new address first

  Scenario: A machine keeps no address where another hub answers
    Given an agent paired with a hub
    When the person gives the agent an address where another hub that has another key answers
    Then the agent stops because the hub's key isn't the one it trusts
    And the agent sent no proof
    And the agent keeps only the address in its token

  Scenario: A machine falls back to its token's address when the new one doesn't answer
    Given an agent paired with a hub
    And the person gave the agent the hub's new address
    And the hub answers only at the address in its token
    When the agent connects to its hub
    Then the hub accepts the agent's proof
    And the agent tried the hub's new address, then the address in its token

  @e2e
  Scenario: A machine follows its hub to a new address from its terminal
    Given a hub with a pairing token for its https address made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token at the hub's https address from its terminal
    When the hub moves to another address
    And the person gives the machine the hub's new address from its terminal
    Then the machine says it keeps the hub's new address
    When the machine checks its link to the hub from its terminal
    Then the machine says the hub holds it waiting for approval

  @e2e
  Scenario: A machine keeps no address where another hub answers, from its terminal
    Given a hub with a pairing token for its https address made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token at the hub's https address from its terminal
    And another hub is serving on loopback
    When the person gives the machine the other hub's address from its terminal
    Then the machine is refused because the hub's certificate isn't the one the token names
    And the machine says it kept the hub addresses it had
    When the machine checks its link to the hub from its terminal
    Then the machine says the hub holds it waiting for approval
