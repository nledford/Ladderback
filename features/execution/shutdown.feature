Feature: Stopping the daemon
  Told to stop (Ctrl-C, or the SIGTERM systemd and launchd send), the daemon
  stops serving, stops its workers the way a cancel does, so a backup that
  archives winds down, closes the operation log once the writes in flight
  have ended, and only then lets go of the data folder (§9). It leaves the
  log in its one file, as tidy as it found it, and exits with status 0.

  A stop has a limit: each step is waited for within a bound of its own,
  and the bounds add up to less than the service manager waits before it
  kills the daemon. A step that never ends is given up at its bound, so the
  daemon still stops cleanly, and the log says it gave up (#543).

  An operation an orderly stop interrupts is recorded, not announced. At the
  signal every run is frozen where it stands: it starts no worker, runs no
  further hook, records no end and announces none. Once the workers have
  stopped, the operation log ends each operation still running as failed
  with the code that the daemon was shut down, and nothing is sent, since
  whoever stopped the daemon knows why. A backup in its after hooks counts
  as interrupted, and keeps on its record the snapshot it wrote. A stop that
  never gets that far (the daemon killed while it stops, a power cut) leaves
  them running in the log, and the next start ends and announces each as
  after a crash (`features/execution/restart.feature`). A queued operation
  stays queued, for the next start to run (decision 73).

  # A plan's after hook that waits until it is let go holds its backup
  # running, as in restart.feature.
  @e2e
  Scenario: A backup an orderly stop interrupts is recorded as ended by the stop
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's after hook waits until it is let go
    And the daemon is running a backup of the plan "home"
    When the daemon is told to stop
    Then the daemon has exited with status 0
    When Ladderback is started again on the same folders
    Then the history, read once, shows the operation failed with the code "daemon-shut-down"

  # The after hooks run once the backup has written its snapshot.
  @e2e
  Scenario: A backup an orderly stop interrupts in its after hooks keeps the snapshot it wrote
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's after hook waits until it is let go
    And the daemon is running a backup of the plan "home"
    And the plan's after hook is running
    When the daemon is told to stop
    Then the daemon has exited with status 0
    When Ladderback is started again on the same folders
    Then the history, read once, shows the operation failed with the code "daemon-shut-down"
    And the history, read once, shows the snapshot the backup wrote

  # A run goes no further once the daemon is told to stop, even one whose
  # worker has ended: its hooks, its end and its announcement belong to a
  # daemon that is going. The request held open keeps the daemon stopping
  # for 5 s, long enough for a run that went on to reach its finally hook.
  @e2e
  Scenario: A backup's run goes no further once the daemon is told to stop
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's after hook waits until it is let go
    And the plan runs a finally hook that notes it ran
    And the daemon is running a backup of the plan "home"
    And a client has begun a request it never finishes
    When the daemon is told to stop, and the plan's after hook is let go once it is stopping
    Then the daemon has exited with status 0
    And the plan's finally hook has not run
    When Ladderback is started again on the same folders
    Then the history, read once, shows the operation failed with the code "daemon-shut-down"

  # Each backup a start ends is announced within seconds of that start, so
  # one the first start announced would reach the webhook long before the
  # second backup is killed.
  @e2e
  Scenario: Of a backup an orderly stop interrupts and one a kill interrupts, only the killed one is announced
    Given a webhook listening on loopback that the rule "failures" sends "backup.failed" to
    And Ladderback is serving on loopback with the plan "home"
    And the plan's after hook waits until it is let go
    And the daemon is running a backup of the plan "home"
    When the daemon is told to stop
    And Ladderback is started again on the same folders
    Then the history, read once, shows the operation failed with the code "daemon-shut-down"
    When the daemon is running a backup of the plan "home"
    And the daemon is killed
    And Ladderback is started again on the same folders
    Then the webhook is told "backup.failed" for the plan "home" with the code "daemon-stopped"
    And the history, read once, shows the operation failed with the code "daemon-stopped"

  @e2e
  Scenario: A daemon told to stop closes the operation log before it exits
    Given Ladderback is serving on loopback with the plan "home"
    When the daemon has finished 1 backup of the plan "home"
    And the daemon is told to stop
    Then the daemon has exited with status 0
    And the operation log is closed, with no "-wal" or "-shm" file of it left in the data folder

  @e2e
  Scenario: A daemon told to stop says when it began stopping and when it had stopped
    The log shows a stop as it happens: the signal that began it, and once
    the daemon has stopped, how long that took and how long each step was
    waited for, so a slow stop, or one the service manager cut short, can be
    told from a crash.

    Given Ladderback is serving on loopback with the plan "home"
    When the daemon is told to stop
    Then the daemon has exited with status 0
    And the daemon's log says "stopping (SIGTERM)"
    And the daemon's log says how long it took to stop, and how long it waited for each step

  # The signal is sent the moment the line is read, before the daemon has
  # answered anything, and the start is repeated: a signal that lands before
  # the daemon listens for it kills it, so the scenario looks for that
  # moment more than once (#628).
  @e2e
  Scenario: A daemon told to stop the moment it says it listens stops in order
    A service manager may stop a daemon it has just started, as a quick
    restart does. From the moment the daemon says it listens, a stop signal
    takes the orderly stop, never the default that kills it outright.

    Given Ladderback is serving on loopback with the plan "home"
    When the daemon is told to stop
    And Ladderback is started again on the same folders, and told to stop the moment it says it listens
    Then the daemon has exited with status 0
    And the daemon's log says how long it took to stop, and how long it waited for each step
    When Ladderback is started again on the same folders, and told to stop the moment it says it listens
    Then the daemon has exited with status 0
    And the daemon's log says how long it took to stop, and how long it waited for each step
    When Ladderback is started again on the same folders, and told to stop the moment it says it listens
    Then the daemon has exited with status 0
    And the daemon's log says how long it took to stop, and how long it waited for each step

  @e2e
  Scenario: A request that never ends doesn't keep the daemon from stopping
    The daemon waits 5 s for the requests still open to end, then stops
    without them.

    Given Ladderback is serving on loopback with the plan "home"
    And a client has begun a request it never finishes
    When the daemon is told to stop
    Then the daemon has exited with status 0
    And the daemon stopped within 10 seconds
    And the daemon's log says it stopped without waiting longer for the open requests
