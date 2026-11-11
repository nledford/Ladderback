Feature: Stopping the daemon
  Told to stop (Ctrl-C, or the SIGTERM systemd and launchd send), the daemon
  stops serving, closes the operation log once the writes in flight have
  ended, and only then lets go of the data folder (§9). It leaves the log in
  its one file, as tidy as it found it, and exits with status 0.

  @e2e
  Scenario: A daemon told to stop closes the operation log before it exits
    Given Ladderback is serving on loopback with the plan "home"
    When the daemon has finished 1 backup of the plan "home"
    And the daemon is told to stop
    Then the daemon has exited with status 0
    And the operation log is closed, with no "-wal" or "-shm" file of it left in the data folder
