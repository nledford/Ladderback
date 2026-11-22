Feature: The hub's keys at its first start
  A hub makes its signing key and its recovery key when it first starts,
  before any machine or person asks for one, so that a key file it can't
  read stops the start rather than the first pairing. A machine that isn't a
  hub makes none of them (technical design §16, decision 120, "The hub key").

  @e2e
  Scenario: A hub's first start makes its keys, and a machine's first start makes none
    When the hub starts for the first time
    And Ladderback starts for the first time on a machine that isn't a hub
    Then the hub's data folder holds a hub key, a recovery key and a recovery seed
    And the machine's data folder holds no hub key, no recovery key and no recovery seed

  @e2e
  Scenario: A hub whose key file holds no key doesn't start
    Given the hub's data folder holds a hub key file with no key in it
    When the hub starts
    Then the hub doesn't start, and says its hub key file holds no key it can use
    And the hub key file still holds what it held
