Feature: Approving a machine from the hub's terminal
  A machine that pairs is held pending until a person approves it on the
  hub. At the hub's terminal, while the daemon is stopped as for making a
  token, `ladderback pairing-token --pending` lists the machines waiting
  for approval, and `ladderback pairing-token --approve` names one machine's
  hostname, agent ID and full key fingerprint, asks whether to approve it,
  and approves it. The machine is welcomed approved the next time it
  connects (technical design §7.1; §16, decisions 120 and 121, "From a
  terminal").

  @e2e
  Scenario: A machine paired while nobody waited is approved at the hub's terminal
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the hub has stopped
    When the person approves "e2e-laptop" at the hub's terminal with --yes
    Then the terminal named "e2e-laptop" with the key the machine showed, and approved it
    When the hub serves again on the same address
    And the machine checks its link to the hub from its terminal
    Then the machine says the hub approved it

  @e2e
  Scenario: The hub's terminal lists the machines waiting for approval
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the hub has stopped
    When the person lists the machines waiting at the hub's terminal
    Then the terminal lists "e2e-laptop" with the key the machine showed

  @e2e
  Scenario: The hub's terminal writes when each machine paired in the person's own time zone
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the hub has stopped
    And the hub's terminal runs in the time zone "Asia/Kolkata"
    When the person lists the machines waiting at the hub's terminal
    Then the terminal lists "e2e-laptop" as waiting since it paired, in that zone, in words, with the offset "+05:30"

  @e2e
  Scenario: The hub's terminal says when no machine waits for approval
    Given a hub with a pairing token made at its terminal
    When the person lists the machines waiting at the hub's terminal
    Then the terminal says no machine waits for approval

  @e2e
  Scenario: With no terminal to ask on, the hub approves nothing without --yes
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the hub has stopped
    When the person approves "e2e-laptop" at the hub's terminal with no terminal to ask on
    Then the terminal approved nothing and says to compare the key and run again with --yes
    And once the hub stops, it lists "e2e-laptop" as waiting for approval

  @e2e
  Scenario: A machine is approved once
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the hub has stopped
    And the person approved "e2e-laptop" at the hub's terminal
    When the person approves "e2e-laptop" at the hub's terminal with --yes
    Then the terminal refuses because "e2e-laptop" is approved already

  @e2e
  Scenario: The hub's terminal approves no machine that never paired
    Given a hub with a pairing token made at its terminal
    When the person approves "e2e-desktop" at the hub's terminal with --yes
    Then the terminal refuses because no machine named "e2e-desktop" paired with the hub

  @e2e
  Scenario: The hub's terminal approves nothing while the daemon runs
    Given the hub is serving on loopback
    When the person approves "e2e-laptop" at the hub's terminal with --yes
    Then the terminal refuses to approve a machine while the daemon runs, and points to the Machines tab

  @e2e
  Scenario: The hub's terminal lists no machine waiting while the daemon runs
    Given the hub is serving on loopback
    When the person lists the machines waiting at the hub's terminal
    Then the terminal refuses to list the machines waiting while the daemon runs, and points to the Machines tab
