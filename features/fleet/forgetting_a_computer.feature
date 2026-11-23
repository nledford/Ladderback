Feature: Forgetting at the hub's terminal the computer the hub took
  The hub takes one computer under each machine ID and refuses a second
  one that proves itself with the same keys, as a copy of a data folder on
  another computer would (technical design §7.1, §7.2; §16, decision 150).
  When the refused computer is the one that should keep the machine ID, the
  person stops the other computer or gives it a new machine ID, stops the
  hub's daemon and runs `ladderback pairing-token --forget-computer` with
  the machine's hostname: the next computer to connect as that machine is
  the one the hub takes from then on. It asks for no confirmation, since it
  runs only on the hub's own computer with the daemon stopped (decision 99).

  @e2e
  Scenario: Once the hub's terminal forgets the computer it took, the next computer to connect is taken
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the hub has stopped
    And the hub took another computer as "e2e-laptop" first, as a copy of the machine's data folder would make it
    When the hub serves again on the same address
    And the machine checks its link to the hub from its terminal
    Then the machine is refused because the hub took another computer as it first
    When the hub stops
    And the person forgets the computer the hub took as "e2e-laptop" at the hub's terminal
    Then the terminal says the hub forgot the computer it took as "e2e-laptop"
    When the hub serves again on the same address
    And the machine checks its link to the hub from its terminal
    Then the machine says the hub holds it waiting for approval
    And once the hub stops, it holds no second computer under "e2e-laptop"

  @e2e
  Scenario: The hub's terminal forgets no computer while the daemon runs
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    When the person forgets the computer the hub took as "e2e-laptop" at the hub's terminal
    Then the terminal refuses to forget a computer while the daemon runs

  @e2e
  Scenario: The hub's terminal forgets nothing for a machine that never paired
    Given a hub with a pairing token made at its terminal
    When the person forgets the computer the hub took as "e2e-desktop" at the hub's terminal
    Then the terminal refuses because no machine named "e2e-desktop" paired with the hub
