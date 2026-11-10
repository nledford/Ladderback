Feature: Restarting after a crash
  A crash, a kill or a power cut leaves operations in the log that no
  process is running. The next start ends each of them as failed, with the
  reason that the daemon stopped, before it accepts a request, and a
  maintenance operation it ended stops holding its repository. One daemon at
  a time serves a data folder: a second one is refused and touches nothing.

  @e2e
  Scenario: An operation a killed daemon left running is failed by the next start
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's source holds many more files
    And the daemon is running a backup of the plan "home"
    When the daemon is killed
    And Ladderback is started again on the same folders
    Then the history, read once, shows the operation failed with the code "daemon-stopped"

  @e2e
  Scenario: A maintenance operation a killed daemon left running no longer holds its repository
    Given Ladderback is serving on loopback with the plan "home"
    And the daemon is killed
    And the log, while the daemon is down, shows a running prune of the repository
    When Ladderback is started again on the same folders
    Then the history, read once, shows the operation failed with the code "daemon-stopped"
    And a prune of the repository is accepted

  @e2e
  Scenario: A second daemon on a folder that is in use is refused and disturbs nothing
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's source holds many more files
    And the daemon is running a backup of the plan "home"
    When a second Ladderback is started on the same folders
    Then the second Ladderback is refused, naming the data folder and the process that holds it
    And the history page shows the operation running
    And the history page shows the operation succeeded
