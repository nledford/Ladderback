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

  @e2e
  Scenario: A machine reads a token pasted across two lines
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    When a machine named "e2e-laptop" pairs with the token pasted across two lines from its terminal
    Then the machine is paired and waits for approval

  @e2e
  Scenario: The hub's terminal makes no token for an address a machine would refuse
    When the person asks the hub's terminal for a pairing token for "https://hub.example/ladderback"
    Then the terminal refuses because the address isn't a hub address
    And the terminal shows no token

  @e2e
  Scenario: The hub's terminal refuses a token life past what it counts
    When the person asks the hub's terminal for a pairing token lasting 153722867280912931 minutes
    Then the terminal refuses because a token lives 5 to 1440 minutes
    And the terminal shows no token
