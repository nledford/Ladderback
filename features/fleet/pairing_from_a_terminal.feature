Feature: Pairing a machine from a terminal
  On the hub, `ladderback pairing-token` makes a one-time pairing token
  while the daemon is stopped, lists the unused ones and revokes one. On
  the machine, `ladderback pair` reads the token as standard input and
  pairs with the hub, which holds the machine pending approval (technical
  design §7.1; §16, decision 121, "From a terminal"). A token for the hub's
  https address pins the certificate of its built-in TLS; a token for a
  plain http address trusts no certificate, so machines pair over it only
  on loopback.

  @e2e
  Scenario: A machine pairs with a token made at the hub's terminal
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    When a machine named "e2e-laptop" pairs with the token from its terminal
    Then the machine is paired and waits for approval
    And once the hub stops, it lists "e2e-laptop" as waiting for approval
    And the hub's terminal lists no unused token

  @e2e
  Scenario: The hub's terminal writes a new token's lapse time in the person's own time zone
    Given the hub's terminal runs in the time zone "Asia/Kolkata"
    When the person asks the hub's terminal for a pairing token
    Then the terminal says the token is usable until its lapse time in that zone, in words, with the offset "+05:30"

  @e2e
  Scenario: The hub's terminal lists an unused token's lapse time in the person's own time zone
    Given a hub with a pairing token made at its terminal
    And the hub's terminal runs in the time zone "Asia/Kolkata"
    When the person lists the unused tokens at the hub's terminal
    Then the terminal lists the token with its lapse time in that zone, in words, with the offset "+05:30"

  @e2e
  Scenario: The hub's log says that a machine paired, and what to match against the machine's screen
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    Then the hub's log says in one line that "e2e-laptop" paired and waits for approval, naming its agent ID and the key the machine showed
    And the hub's log holds nothing of the token

  @e2e
  Scenario: A machine that pairs again under another hostname is told which one the hub holds
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "first" paired with the token from its terminal
    And the machine lost the hub's reply, so it holds no pin
    When a machine named "other" pairs with the token from its terminal
    Then the machine says the hub holds it under the hostname "first", not "other"
    And the hub's log names the machine by the hostname "first" it holds and the hostname "other" it asked for
    And once the hub stops, it lists "first" as waiting for approval

  @e2e
  Scenario: An approved machine that pairs again under another hostname is told it is approved, under the hostname the hub holds
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "first" paired with the token from its terminal
    And the person approved "first" on the hub's page
    And the machine lost the hub's reply, so it holds no pin
    And the hub has stopped
    And the person made another pairing token at the hub's terminal
    When the hub serves again on the same address
    And a machine named "other" pairs with the token from its terminal
    Then the machine says the hub approved it already, under the hostname "first", not "other"
    And the hub's log names the machine by the hostname "first" it holds and the hostname "other" it asked for

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
    Then the terminal refuses to make a token while the daemon runs, and points to the Machines tab
    And the terminal shows no token

  @e2e
  Scenario: The hub's terminal lists no unused token while the daemon runs
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    When the person lists the unused tokens at the hub's terminal
    Then the terminal refuses to list the unused tokens while the daemon runs

  @e2e
  Scenario: The hub's terminal revokes no token while the daemon runs, and says the token can still pair a machine
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    When the person revokes the token at the hub's terminal
    Then the terminal refuses to revoke the token while the daemon runs, and says it can still pair a machine
    When a machine named "e2e-laptop" pairs with the token from its terminal
    Then the machine is paired and waits for approval

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
  Scenario: A machine reading a token for a plain http address says it isn't encrypted and names no certificate
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    When a machine named "e2e-laptop" pairs with the token from its terminal
    Then the machine read a token for an address that isn't encrypted, naming no certificate
    And the machine is paired and waits for approval

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

  @e2e
  Scenario: A machine pairs over the hub's built-in TLS with a token pinned to its certificate
    Given a hub with a pairing token for its https address made at its terminal
    And the hub is serving on loopback
    When a machine named "e2e-laptop" pairs with the token at the hub's https address from its terminal
    Then the machine read a token pinned to the hub's own certificate
    And the machine is paired and waits for approval

  @e2e
  Scenario: A machine refuses a hub whose certificate isn't the one its token pins
    Given a hub with a pairing token for its https address made at its terminal
    And another hub is serving on loopback
    When a machine named "e2e-laptop" pairs with the token at the other hub's https address from its terminal
    Then the machine is refused because the hub's certificate isn't the one the token names

  @e2e
  Scenario: A restarted hub serves the same certificate, so a pinned machine still connects
    Given a hub with a pairing token for its https address made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token at the hub's https address from its terminal
    When the hub restarts on the same address
    And the machine checks its link to the hub from its terminal
    Then the machine says the hub holds it waiting for approval

  @e2e
  Scenario: A hub whose TLS key is lost serves only its own machine and says how to restore the key
    Given a hub with a pairing token for its https address made at its terminal
    And the hub's TLS key file is lost
    And the hub is serving on loopback
    When a machine named "e2e-laptop" pairs with the token at the hub's https address from its terminal
    Then the machine is refused because the hub can't be reached
    And the hub refuses a machine's connection over plain http on loopback
    And the hub still serves its own machine on loopback
    And the hub's log says its TLS key is missing and to restore it from a backup

  @e2e
  Scenario: The hub's terminal makes no pinned token while its TLS key is lost, and says no machine pairs until the hub starts with it restored
    Given a hub with a pairing token for its https address made at its terminal
    And the hub's TLS key file is lost
    When the person asks the hub's terminal for a pairing token for "https://127.0.0.1:9"
    Then the terminal refuses because no machine pairs until the hub starts with its TLS key restored from a backup
    And the terminal shows no token