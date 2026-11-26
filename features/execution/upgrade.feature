Feature: Upgrading from an earlier version by installing this one
  A machine running Ladderback 0.5.0, or a build made before this version,
  upgrades by installing this version and starting it (§16, decision 170).
  The first start moves the machine ID and key from the names an earlier
  build gave them (`agent-id`, `agent-key`) to their own (`machine-id`,
  `machine-key`), keeping the ID, so the machine's snapshots stay its own.
  It copies an operation log the migrations before the baseline schema left
  into the baseline once, keeping the old log beside it as
  `ladderback.db.before-baseline`: the history and the schedule's state read
  back as before, and the plans in the configuration file are untouched. A
  start that stops partway leaves the old log as it was, and the next start
  copies it again. Only a process that holds the data folder upgrades it: a
  command run beside the daemon, such as `hub-status`, upgrades a folder
  nobody holds itself, and changes nothing while another Ladderback holds
  it. This upgrade is removed at 1.0.

  # Migration 16 is the schema Ladderback 0.5.0 shipped; 28 and 29 are the
  # last two before the baseline, which builds after 0.5.0 left.
  @e2e
  Scenario Outline: A data folder an earlier version left at migration <migration> upgrades on start with its machine ID, its plans and its history
    Given Ladderback is serving on loopback with the plan "home"
    And the daemon is killed
    And the data folder is as an earlier Ladderback left it: its operation log at migration <migration>, its machine ID and key under their earlier names
    When Ladderback is started again on the same folders
    Then the machine keeps the machine ID and key it had under their earlier names
    And the history shows the operations the earlier version logged
    And the earlier operation log is kept beside the new one
    And a backup of the plan "home" the daemon runs now is tagged with that machine ID

    Examples:
      | migration |
      | 16        |
      | 28        |
      | 29        |

  # 0.5.0's daemon still running on the folder, not yet stopped.
  @e2e
  Scenario: A command beside a Ladderback that holds an earlier version's data folder changes nothing
    Given Ladderback is serving on loopback with the plan "home"
    And the daemon is killed
    And the data folder is as an earlier Ladderback left it: its operation log at migration 16, its machine ID and key under their earlier names
    And another Ladderback still holds that data folder
    When the owner runs "hub-status --hostname hub.example.test" on that data folder
    Then it is refused, saying Ladderback is running on the folder and must be stopped first
    And every file in the data folder is as it was
