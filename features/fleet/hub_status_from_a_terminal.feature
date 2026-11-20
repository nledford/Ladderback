Feature: Checking a machine's link to its hub from a terminal
  On a paired machine, `ladderback hub-status` connects to the hub the
  machine pinned at pairing, proves who it is, and says whether the hub
  holds it pending or has approved it (technical design §7.1; §16,
  decisions 120 and 121, "From a terminal"). The hub doesn't serve its
  built-in TLS yet, so these machines connect over plain http on loopback.

  @e2e
  Scenario: A paired machine is told its hub holds it waiting for approval
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    When the machine checks its link to the hub from its terminal
    Then the machine says the hub holds it waiting for approval

  @e2e
  Scenario: A machine that never paired has no hub to check
    When the machine checks its link to the hub from its terminal
    Then the machine says it isn't paired with a hub
    And the machine's data folder holds nothing
