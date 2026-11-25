Feature: Leaving a lost or replaced hub
  A machine paired with a hub keeps the hub's key as its pin and pairs with
  no other hub while it holds it: a token from another hub is refused, in
  words that name both hubs' key fingerprints and the way out. When the hub
  is lost or replaced, the person stops the machine's daemon and runs
  `ladderback unpair` at the machine. Once they confirm, the machine drops
  every lease and epoch floor it took from that hub, then forgets the pin:
  it stands alone, maintains its own repositories again, and pairs with any
  hub's token. It never keeps a lease of the hub it left, and the hub's own
  lease expiry stays the guarantee on the hub's side (I1). It refuses with
  no terminal to ask on unless given `--yes`, and while the daemon runs
  (technical design §7.1, §7.3; §16, decision 169).

  Scenario: A machine that leaves its hub drops its leases and maintains its repositories on its own
    Given an agent paired under the hostname "laptop"
    And the agent holds a lease its hub granted
    When the agent leaves its hub
    Then the agent pins no hub
    And the agent holds no lease and no epoch floor
    And the agent maintains its repositories on its own

  Scenario: A machine that left its hub pairs with another hub
    Given an agent paired under the hostname "laptop"
    And the agent left its hub
    When the agent pairs with another hub's token
    Then the agent pins the other hub's key

  @e2e
  Scenario: A machine leaves its lost hub at its terminal and pairs with the hub that replaces it
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the hub is lost, and a new hub with a pairing token made at its terminal takes its place
    And the hub is serving on loopback
    When a machine named "e2e-laptop" pairs with the token from its terminal
    Then the machine refuses the token, since it pins the lost hub, naming both hubs' keys and "ladderback unpair"
    When the person unpairs the machine from its terminal with --yes
    Then the machine says it left the hub and maintains its own repositories again
    When a machine named "e2e-laptop" pairs with the token from its terminal
    Then the machine is paired and waits for approval

  @e2e
  Scenario: The machine's terminal leaves no hub with no terminal to ask on
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    When the person unpairs the machine with no terminal to ask on
    Then the terminal left the hub nothing and says to run again with --yes
    And the machine still pins the hub

  @e2e
  Scenario: The machine's terminal leaves no hub while the machine's daemon runs
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    When the machine's daemon starts
    And the person unpairs the machine from its terminal with --yes
    Then the terminal refuses to unpair while the machine's daemon runs
    And the machine still pins the hub

  @e2e
  Scenario: The machine's terminal says a machine that never paired has no hub to leave
    When the person unpairs the machine from its terminal with --yes
    Then the terminal says the machine isn't paired with a hub
