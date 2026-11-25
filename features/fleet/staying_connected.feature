Feature: A paired machine stays connected to its hub
  An approved machine holds one connection to its hub open while its daemon
  runs, and sends a heartbeat on it as it connects and every 30 seconds
  after, which the hub answers. Its reports, plans and progress go over
  that same connection as they come. A machine whose connection dropped
  finds it closed at its next heartbeat at the latest, and connects again
  after its backoff, from 1 second, doubling to 5 minutes. The hub shows a
  machine online from that connection's first message until it ends, so a
  check from the machine's terminal leaves the daemon's connection in
  place; it closes a connection that sent nothing for 90 seconds, and tells
  its browsers when a machine comes online or goes offline (technical
  design §7.2; §16, decision 159).

  Scenario: An idle machine holds one connection and sends a heartbeat every 30 seconds
    Given an agent paired with a hub
    And a person approved the agent on the hub
    And the agent keeps syncing with its hub while it runs
    When an hour passes with nothing to report
    Then the agent connected to its hub once
    And the agent sent its hub 121 heartbeats, the first as it connected
    And the hub shows the agent online

  Scenario: A check of the hub from the machine's terminal leaves the daemon's connection in place
    Given an agent paired with a hub
    And a person approved the agent on the hub
    And the agent keeps syncing with its hub while it runs
    And the agent is connected to its hub
    When the machine checks its hub from its terminal
    And a minute passes on the agent's clock
    Then the agent connected to its hub once
    And the hub shows the agent online
    And the hub's browsers heard the agent come online and nothing more

  Scenario: A dropped connection is found at the next heartbeat and made again after the backoff
    Given an agent paired with a hub
    And a person approved the agent on the hub
    And the agent keeps syncing with its hub while it runs
    And the agent is connected to its hub
    When the hub drops the agent's connection
    Then the hub shows the agent offline
    And the agent connects to its hub again within 31 seconds, 1 second after its next heartbeat found the connection closed
    And the hub shows the agent online

  Scenario: The hub closes the connection of a machine that sent nothing for 90 seconds
    Given an agent paired with a hub
    And a person approved the agent on the hub
    And the agent keeps syncing with its hub while it runs
    And the agent is connected to its hub
    When the agent stops sending anything
    And 89 seconds pass on the hub's clock
    Then the hub shows the agent online
    When 1 more second passes on the hub's clock
    Then the hub holds no connection of the agent
    And the hub shows the agent offline
    And the hub's browsers heard the agent come online, then go offline

  Scenario: Reports cross the one connection in order, and a lost acknowledgement loses none
    Given an agent paired with a hub
    And a person approved the agent on the hub
    And the agent keeps syncing with its hub while it runs
    And the agent is connected to its hub
    And the hub's next acknowledgement is lost with its connection
    When the agent runs 3 backups
    Then the hub soon holds the agent's 6 reports, each once, in the order they happened
    And the agent connected to its hub twice
    And the hub shows the agent online

  @e2e
  Scenario: A machine's daemon stays connected to its hub, through a restart of the hub, until it stops
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the hub has stopped
    And the person approved "e2e-laptop" at the hub's terminal
    When the hub serves again on the same address
    And the machine's daemon starts
    Then the hub soon shows "e2e-laptop" online
    When the hub restarts on the same address
    Then the hub soon shows "e2e-laptop" online
    When the machine's daemon stops
    Then the hub soon shows "e2e-laptop" offline
