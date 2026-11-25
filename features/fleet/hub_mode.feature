@e2e
Feature: Hub mode in config.toml
  A machine becomes the hub in place: the `[serve]` table of its
  config.toml turns hub mode on, with the address the other machines reach
  it at and the host names they reach it by. `ladderback serve` reads them
  at start, so the service `install-service` writes runs the hub with no
  flag of its own, and a command-line flag overrides each of them. Hub mode
  turned off stays on while a machine is paired with the hub, and the hub
  names each such machine: it would lose its hub, and a hub that refused to
  start would stop this machine's backups too (technical design §16,
  decisions 136 and 167).

  Scenario: A machine whose config.toml turns hub mode on starts as the hub
    Given the hub's config.toml turns hub mode on
    When the hub starts with no command-line flag
    Then the hub's data folder holds a hub key, a recovery key and a recovery seed

  Scenario: The address and the allowed host names come from config.toml
    Given the hub's config.toml listens on a free port on loopback and allows the host name "files.example"
    When the hub starts with no command-line flag
    Then the hub listens on the address its config.toml gives
    And the hub answers a request for the host name "files.example"
    And the hub refuses a request for the host name "other.example"

  Scenario: A command-line flag overrides each setting of config.toml
    Given the hub's config.toml turns hub mode on
    And the hub's config.toml listens on a free port on loopback and allows the host name "files.example"
    When the hub starts with "--hub=false --listen 127.0.0.1:0 --allowed-host other.example"
    Then the hub listens on another address than its config.toml gives
    And the hub answers a request for the host name "other.example"
    And the hub refuses a request for the host name "files.example"
    And the hub's data folder holds no hub key

  Scenario: Hub mode turned off stays on while a machine is paired, naming it
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    When the hub restarts with hub mode off in its config.toml
    Then the hub still starts as the hub, and says hub mode stays on while "e2e-laptop" is paired

  Scenario: --hub=false keeps hub mode on as well while a machine is paired
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    When the hub restarts with "--hub=false" over hub mode on in its config.toml
    Then the hub still starts as the hub, and says hub mode stays on while "e2e-laptop" is paired

  Scenario: A host name in config.toml that no browser could send stops the start, naming the file
    Given the hub's config.toml allows the host name "files.example:9898"
    When the hub tries to start with no command-line flag
    Then the hub doesn't start, and says "allowed-hosts" in config.toml is the cause

  Scenario: An address beyond loopback in config.toml with no allowed host name stops the start
    Given the hub's config.toml listens on "0.0.0.0:0"
    When the hub tries to start with no command-line flag
    Then the hub doesn't start, and says "allowed-hosts" in config.toml is the cause

  Scenario: Hub mode turns off while no machine is paired
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    When the hub restarts with hub mode off in its config.toml
    Then the hub starts as a machine that isn't a hub
