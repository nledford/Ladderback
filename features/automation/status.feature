Feature: The status summary an AI agent reads
  "Is everything backed up?" is one question. The status summary answers it
  in one reading: for each plan, its last successful backup and how long ago,
  and whether it is overdue for its schedule; and for the agent, the backups
  running and waiting their turn and the repositories that couldn't be read
  at their last attempt. It names plans and repositories by their names,
  never a secret (I4). Each scenario's plan runs every hour on the
  repository "nas", and the plan "work" runs on the repository "office".

  Background:
    Given the plans "home" on "nas" and "work" on "office", each run every hour

  Scenario: A healthy setup is on schedule with nothing waiting
    Given the plan "home" last backed up successfully 20 minutes ago
    And the plan "work" last backed up successfully 50 minutes ago
    When an AI agent reads the status summary
    Then the plan "home" last backed up successfully 20 minutes ago
    And the plan "work" last backed up successfully 50 minutes ago
    And no plan is overdue
    And no repository is said to be unreadable
    And no backup is running or waiting

  Scenario: A plan whose last success is a whole run behind is overdue
    Given the plan "home" last backed up successfully 3 hours ago
    And the plan "home" last failed 10 minutes ago
    And the plan "work" last backed up successfully 50 minutes ago
    When an AI agent reads the status summary
    Then the plan "home" is overdue
    And the plan "home" last backed up successfully 3 hours ago
    And the plan "work" is not overdue

  Scenario: A plan that has never backed up successfully is overdue
    Given the plan "work" last backed up successfully 50 minutes ago
    When an AI agent reads the status summary
    Then the plan "home" is overdue
    And the plan "home" has no successful backup

  Scenario: A repository that couldn't be read at its last attempt is named
    Given the plan "home" last backed up successfully 2 hours ago
    And the plan "home" last failed 10 minutes ago because no repository's config file was found
    And the plan "work" last backed up successfully 50 minutes ago
    When an AI agent reads the status summary
    Then the repository "nas" is said to be unreadable with "repository-config-not-found"
    And the repository "office" is not said to be unreadable

  Scenario: A repository read since its failure is no longer named
    Given the plan "home" last failed 2 hours ago because no repository's config file was found
    And the plan "home" last backed up successfully 20 minutes ago
    When an AI agent reads the status summary
    Then no repository is said to be unreadable

  Scenario: A queue that is waiting shows what runs and what waits
    Given the plan "home" last backed up successfully 50 minutes ago
    And a backup of the plan "home" is running
    And a backup of the plan "work" is waiting its turn
    When an AI agent reads the status summary
    Then the backup of the plan "home" is running
    And the backup of the plan "work" is waiting
