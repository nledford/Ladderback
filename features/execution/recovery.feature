Feature: Crash recovery
  A daemon that stopped without finishing its work (a crash, a kill, a power
  cut, a restart) leaves operations the log still shows as queued or running.
  Nothing is running them any more, so the next start ends each of them as
  failed because the daemon stopped. It never shows one as succeeded, and a
  maintenance operation that was running no longer holds its repository (I7).

  Scenario: A backup the log shows as running is shown as failed
    Given the log shows a backup of the plan "home" as running
    When the daemon starts again
    Then the log shows the backup of the plan "home" as failed because the daemon stopped
    And the log never shows the backup of the plan "home" as succeeded

  Scenario: An operation the log shows as queued is ended the same way
    Given the log shows a backup of the plan "home" as queued
    When the daemon starts again
    Then the log shows the backup of the plan "home" as failed because the daemon stopped

  Scenario: The warnings a running backup had recorded stay on it
    Given the log shows a backup of the plan "home" as running with 2 warnings recorded
    When the daemon starts again
    Then the log shows the backup of the plan "home" as failed because the daemon stopped
    And the log still shows 2 warnings on the backup of the plan "home"

  Scenario Outline: A running maintenance operation lets go of its repository
    Given the log shows a <kind> of the repository "nas" as running
    When the daemon starts again
    Then the log shows the <kind> of the repository "nas" as failed because the daemon stopped
    And the repository "nas" is free for a prune and for a check

    Examples:
      | kind   |
      | prune  |
      | check  |
      | forget |
      | adopt  |

  Scenario Outline: An operation that ended before the daemon stopped keeps its outcome
    Given the log shows a backup of the plan "home" that ended as <ending>
    And the log shows a backup of the plan "docs" as running
    When the daemon starts again
    Then the log still shows the backup of the plan "home" as <ending>
    And the log shows the backup of the plan "docs" as failed because the daemon stopped

    Examples:
      | ending                                       |
      | succeeded                                    |
      | succeeded with warnings                      |
      | failed because the repository was unreachable |
      | cancelled                                    |

  Scenario: A log whose operations all ended has nothing to recover
    Given the log shows a backup of the plan "home" that ended as succeeded
    And the log shows a backup of the plan "docs" that ended as cancelled
    When the daemon starts again
    Then the daemon's start ended 0 operations

  Scenario: An empty log has nothing to recover
    Given the log shows no operation at all
    When the daemon starts again
    Then the daemon's start ended 0 operations

  Scenario: Starting again after recovery ends nothing more
    Given the log shows a backup of the plan "home" as running
    When the daemon starts again
    And the daemon starts again
    Then the daemon's first start ended 1 operation
    And the daemon's start ended 0 operations
    And the log shows the backup of the plan "home" as failed because the daemon stopped

  Scenario: Every unfinished operation is ended, and counted
    Given the log shows a backup of the plan "home" as queued
    And the log shows a backup of the plan "docs" as running
    And the log shows a check of the repository "nas" as running
    And the log shows a backup of the plan "photos" that ended as succeeded
    When the daemon starts again
    Then the daemon's start ended 3 operations
    And no operation the scenario put in the log is left queued or running
    And the log still shows the backup of the plan "photos" as succeeded

  Scenario: A log that cannot be written leaves the operation running
    Given the log shows a backup of the plan "home" as running
    When the daemon starts again on a log that cannot be written
    Then the daemon's start reports that the log could not be written
    And the log still shows the backup of the plan "home" as running
