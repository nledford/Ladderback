Feature: Restarting Ladderback from Settings
  A restart applies what a running daemon reads only when it starts (an
  admin password set with `ladderback set-password`, an address to listen
  on) with no terminal: the admin asks for it in Settings. The daemon then
  makes the same orderly stop as when it is told to stop
  (`features/execution/shutdown.feature`), and exits with the status its
  service manager starts it again on (decision 94).

  The admin chooses when. Once the running operations finish is the
  default: from then on the daemon starts no new operation, neither a queued
  backup nor maintenance, a restore or a download, and it stops once none is
  running. Queued backups stay queued for the next start (decision 73). A
  download being handed over doesn't hold the restart off: the stop cuts it
  short, and the browser fails it. Until then the admin may cancel the
  restart, and the daemon starts its queued operations again. Now stops at
  once: every run is frozen, and each operation still running is recorded as
  ended by the stop.

  The daemon can only stop; its service manager brings it back. So a restart
  is offered only to a daemon its service file says a service manager runs
  (`features/cli/service.feature`). One started by hand offers none, since
  nothing would start it again. With sign-in on only the signed-in admin may
  ask, never an API token; with sign-in off anyone who may run a plan may.

  @e2e
  Scenario: Asked to restart now, the daemon stops in order and exits for its service manager to start it again
    Given Ladderback is serving on loopback under a service manager
    When the admin asks Ladderback to restart now
    Then the request is accepted
    And the daemon has exited with status 75
    And the daemon's log says "stopping (restart asked with sign-in off)"
    And the daemon's log says how long it took to stop, and how long it waited for each step

  # The after hook holds the backup running, as in shutdown.feature.
  @e2e
  Scenario: A restart once the running operations finish waits for the backup and starts no maintenance meanwhile
    Given Ladderback is serving on loopback under a service manager with the plan "home"
    And the plan's after hook waits until it is let go
    And the daemon is running a backup of the plan "home"
    When the admin asks Ladderback to restart once the running operations finish
    Then the request is accepted
    And Ladderback says a restart waits for the running operations
    And a prune of the repository is refused with the code "restart-pending"
    And a download from the repository is refused with the code "restart-pending"
    And the daemon is still serving
    When the plan's after hook is let go
    Then the daemon exits of itself with the restart status
    When Ladderback is started again on the same folders
    Then the history page shows the operation succeeded

  @e2e
  Scenario: A cancelled restart never comes, and maintenance starts again
    Given Ladderback is serving on loopback under a service manager with the plan "home"
    And the plan's after hook waits until it is let go
    And the daemon is running a backup of the plan "home"
    When the admin asks Ladderback to restart once the running operations finish
    And the admin cancels the restart
    Then Ladderback says no restart waits
    When the plan's after hook is let go
    Then the history page shows the operation succeeded
    And a prune of the repository is accepted
    And the daemon is still serving

  @e2e
  Scenario: A backup a restart now interrupts is recorded as ended by the stop
    Given Ladderback is serving on loopback under a service manager with the plan "home"
    And the plan's after hook waits until it is let go
    And the daemon is running a backup of the plan "home"
    When the admin asks Ladderback to restart now
    Then the daemon has exited with status 75
    When Ladderback is started again on the same folders
    Then the history, read once, shows the operation failed with the code "daemon-shut-down"

  @e2e
  Scenario: A daemon started by hand offers no restart
    Given Ladderback is serving on loopback, started by hand
    When the admin asks Ladderback to restart now
    Then the request is refused with the code "restart-unavailable"
    And Ladderback says no restart is offered
    And the daemon is still serving

  @ui
  Scenario: Restarting from Settings by keyboard alone says Ladderback is restarting
    Given a daemon run by a service manager
    When I open the address "/settings"
    And I skip to the content
    And I press Tab until focus is on the button "Restart Ladderback"
    And I press Enter
    Then the dialog "Restart Ladderback?" opens with focus inside it
    And the dialog "Restart Ladderback?" says "No operation is running."
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Restart"
    And I press Enter
    Then the page shows the paragraph "Ladderback is restarting. This page reloads once it answers again."
    And the polite region reads "Ladderback is restarting. This page reloads once it answers again."
    And the daemon has exited with the restart status
