Feature: Pairing a machine from a terminal
  On the hub, `ladderback pairing-token` makes a one-time pairing token
  while the daemon is stopped, lists the unused ones and revokes one. On
  the machine, `ladderback pair` reads the token as standard input and
  pairs with the hub, which holds the machine pending approval (technical
  design §7.1; §16, decision 121, "From a terminal"). The hub doesn't serve
  its built-in TLS yet, so these machines pair over plain http on
  loopback, the one plain address a machine pairs over.

  @e2e
  Scenario: A machine pairs with a token made at the hub's terminal
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    When a machine named "e2e-laptop" pairs with the token from its terminal
    Then the machine is paired and waits for approval
    And once the hub stops, it lists "e2e-laptop" as waiting for approval
    And the hub's terminal lists no unused token

  @e2e
  Scenario: A token revoked at the hub's terminal can't pair a machine
    Given a hub with a pairing token made at its terminal
    And the person revoked the token at the hub's terminal
    And the hub is serving on loopback
    When a machine named "e2e-laptop" pairs with the token from its terminal
    Then the machine is refused because the token was revoked

  @e2e
  Scenario: The hub's terminal makes no token while the daemon runs
    Given the hub is serving on loopback
    When the person asks the hub's terminal for a pairing token
    Then the terminal refuses because the daemon holds the data folder
    And the terminal shows no token

  @e2e
  Scenario: A machine refuses a plain http address on another machine
    Given a hub with a pairing token made at its terminal
    When a machine pairs with the token at the address "http://hub.invalid:9898" from its terminal
    Then the machine is refused because the address would show the token to the network
