Feature: The hub alerts when a paired machine stays offline
  The hub shows a machine offline once its connection ends, 90 seconds
  after the machine last sent anything at most (decision 159). It watches
  its machines every 30 seconds, and once a machine has been offline for 15
  minutes it tells its notification rules, once, with `machine.offline`,
  naming the machine and since when it is offline. When the machine comes
  back, the hub tells them `machine.online`, but only when it alerted that
  the machine was offline. A laptop asleep for 15 minutes is alerted like
  any other machine. Removing a machine cancels its alert. The hub keeps
  what it alerted in memory, so after it restarts a machine still offline
  is alerted 15 minutes after the restart (technical design §7.2, §11; §16,
  decision 168).

  Background:
    Given an agent paired with a hub
    And a person approved the agent on the hub
    And the hub has a rule that sends "machine.offline, machine.online" to the webhook channel "ops"
    And the agent keeps syncing with its hub while it runs
    And the agent is connected to its hub

  Scenario: A machine offline for 15 minutes is alerted once
    When the agent stops sending anything
    And 90 seconds pass on the hub's clock
    Then the hub shows the agent offline
    When the hub watches its machines for 14 minutes
    Then "ops" was sent nothing
    When the hub watches its machines for 1 more minute
    Then "ops" was sent "machine.offline" for "laptop", offline since 15 minutes ago
    When the hub watches its machines for 60 more minutes
    Then "ops" was sent only "machine.offline"

  Scenario: A laptop asleep for 15 minutes is alerted like any other machine
    When the agent's computer goes to sleep
    And 90 seconds pass on the hub's clock
    And the hub watches its machines for 15 minutes
    Then "ops" was sent "machine.offline" for "laptop", offline since 15 minutes ago

  Scenario: A machine that comes back after its alert is told back online
    When the agent stops sending anything
    And 90 seconds pass on the hub's clock
    And the hub watches its machines for 15 minutes
    And the agent starts again and connects to its hub
    And the hub watches its machines
    Then "ops" was sent "machine.offline", then "machine.online" for "laptop", back online now

  Scenario: A machine back within 15 minutes is not alerted, and its return is not told
    When the agent stops sending anything
    And 90 seconds pass on the hub's clock
    And the hub watches its machines for 14 minutes
    And the agent starts again and connects to its hub
    And the hub watches its machines
    Then "ops" was sent nothing

  Scenario: An alert no rule sent goes to the first rule that sends it, while the machine stays offline
    Given the hub's rule for "ops" is changed to send only "backup.failed"
    When the agent stops sending anything
    And 90 seconds pass on the hub's clock
    And the hub watches its machines for 15 minutes
    Then "ops" was sent nothing
    When the hub's rule for "ops" is changed to send only "machine.offline, machine.online"
    And the hub watches its machines
    Then "ops" was sent "machine.offline" for "laptop", offline since 15 minutes ago

  Scenario: Removing an offline machine cancels its alert
    When the agent stops sending anything
    And 90 seconds pass on the hub's clock
    And the hub watches its machines for 10 minutes
    And the person removes "laptop" on the hub's page
    And the hub watches its machines for 15 more minutes
    Then "ops" was sent nothing

  Scenario: A hub that restarts alerts a machine still offline 15 minutes after its start
    When the agent stops sending anything
    And 90 seconds pass on the hub's clock
    And the hub watches its machines for 10 minutes
    And the hub's daemon restarts
    And the hub watches its machines for 14 more minutes
    Then "ops" was sent nothing
    When the hub watches its machines for 1 more minute
    Then "ops" was sent "machine.offline" for "laptop", offline since 15 minutes ago
