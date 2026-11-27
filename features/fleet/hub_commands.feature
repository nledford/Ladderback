Feature: A machine answers its hub's commands
  A machine connected to its hub runs a plan's backup when the hub asks, as
  Back up now does, and cancels a backup it runs. It acts on a command only
  when its hub signed it on this connection, asked it within 15 minutes of
  the hub's time and never sent it before, and the hub starts at most one
  backup of a plan every 5 minutes and 12 an hour. The machine answers each
  command with what it did, or why it did nothing, and the hub tells the
  person who asked; a machine that isn't connected, or doesn't take the
  command, is sent nothing, and nothing waits to be sent later (technical
  design §7.2, §10.3; §16, decisions 120 and 176).

  Background:
    Given an agent paired with a hub
    And a person approved the agent on the hub
    And the machine has the plan "photos"

  Scenario: The hub's command runs the plan's backup
    Given the machine is connected to its hub
    Then the machine told its hub it takes its commands
    When the hub asks the machine to run the plan "photos"
    Then the machine started a backup of "photos" for the hub's session
    And the machine answered that it started that backup

  Scenario: A plan whose backup is running answers with that backup
    Given the machine is connected to its hub
    And a backup of "photos" is running on the machine
    When the hub asks the machine to run the plan "photos"
    Then the machine started no backup
    And the machine answered that it started the backup already running

  Scenario: A plan the machine doesn't have is refused
    Given the machine is connected to its hub
    When the hub asks the machine to run the plan "music"
    Then the machine started no backup
    And the machine refused the command with "plan-unknown"

  Scenario: A machine that turned its hub's commands off takes none
    Given the machine's configuration turns its hub's commands off
    And the machine is connected to its hub
    Then the machine told its hub it takes no commands
    When the hub asks the machine to run the plan "photos"
    Then the machine started no backup
    And the machine refused the command with "machine-commands-off"

  Scenario: A command asked more than 15 minutes ago is refused
    Given the machine is connected to its hub
    When the hub asks the machine to run the plan "photos", asked 16 minutes ago
    Then the machine started no backup
    And the machine refused the command with "command-expired"

  Scenario: A command sent again after the machine restarted is refused
    Given the machine is connected to its hub
    And the hub asked the machine to run the plan "photos"
    And that backup ended
    When the machine restarts and connects to its hub again
    And the hub sends the machine that command again
    Then the machine started 1 backup
    And the machine refused the command with "command-replayed"

  Scenario: A command the hub didn't sign is acted on in no way
    Given the machine is connected to its hub
    When a command to run the plan "photos" reaches the machine with a signature its hub didn't make
    Then the machine started no backup
    And the machine answered nothing and dropped the connection

  Scenario: The hub starts at most one backup of a plan every 5 minutes
    Given the machine is connected to its hub
    And the hub asked the machine to run the plan "photos"
    And that backup ended
    When 2 minutes pass on the machine and its hub
    And the hub asks the machine to run the plan "photos"
    Then the machine started 1 backup
    And the machine refused the command with "command-rate-limited", to be asked again 3 minutes from now

  Scenario: The hub cancels a backup midway
    Given the machine is connected to its hub
    And the hub asked the machine to run the plan "photos"
    When the hub asks the machine to cancel that backup
    Then that backup was canceled
    And the machine answered that it canceled that backup

  Scenario: The hub cancels only backups
    Given the machine is connected to its hub
    And a check is running on the machine
    When the hub asks the machine to cancel that check
    Then the check runs on
    And the machine refused the command with "command-backups-only"

  Scenario: A machine not connected is named with its plan's next run
    Given the machine told its hub the plan "photos" runs next in an hour
    And the machine's connection to its hub ended
    When a person on the hub asks the machine to run the plan "photos"
    Then the hub told the person "machine-offline-next-run", with the plan's next run in an hour
    And the machine started no backup
