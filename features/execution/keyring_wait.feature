Feature: A scheduled run waits for the session's keyring service while the daemon starts
  At login the daemon can start before the session's Secret Service does,
  and a run the schedule starts then finds no keyring service to ask. Such a
  run, fired while the daemon has been up for less than 2 minutes, waits for
  the service: it reads again only the keyring entry it couldn't read, every
  10 seconds, until the service answers or the daemon has been up for 2
  minutes. The daemon logs the wait once, naming the entry and never a
  secret. A keyring that is locked, or whose unlock prompt was dismissed, is
  not asked again, and a locked one is refused as locked, never as a missing
  keyring service; a person who asked for something, such as a test of the
  repository, is answered at once; and after those 2 minutes no run waits
  (technical design §5).

  Background:
    Given the daemon started a moment ago
    And the keyring holds "CANARY-keyring-password" under "ladderback/demo"
    And the repository's password is in the keyring under "ladderback/demo"
    And the repository is "/srv/demo-repo"

  Scenario: A scheduled run fired before the session's keyring service is up waits for it
    Given the session's keyring service starts 55 seconds after the daemon
    When the schedule starts a backup 30 seconds after the daemon started
    Then the run's secrets are resolved 60 seconds after the daemon started
    And the keyring entry "ladderback/demo" was read 4 times

  Scenario: Only the keyring entry that couldn't be read is read again
    Given the session's keyring service starts 50 seconds after the daemon
    And the file "/home/demo/token" holds "demo-token"
    And the option "token" is "${file:/home/demo/token}"
    When the schedule starts a backup 30 seconds after the daemon started
    Then the run's secrets are resolved 50 seconds after the daemon started
    And the keyring entry "ladderback/demo" was read 3 times
    And the file "/home/demo/token" was read once

  Scenario: The daemon logs the wait once, naming the keyring entry and no secret
    Given the session's keyring service starts 45 seconds after the daemon
    When the schedule starts a backup 30 seconds after the daemon started
    Then the daemon logged once that the run waits for the keyring entry "ladderback/demo"
    And what the daemon logged of the wait holds no "CANARY-keyring-password"

  Scenario: A locked keyring fails the run at once and isn't asked again
    Given the keyring is locked
    When the schedule starts a backup 30 seconds after the daemon started
    Then the run fails 30 seconds after the daemon started because the keyring is locked
    And the keyring entry "ladderback/demo" was read 1 time
    And the daemon never logged that the run waits

  Scenario: A locked keyring is refused as locked, not as a missing keyring service
    Given the keyring is locked
    When the schedule starts a backup 30 seconds after the daemon started
    Then the run is refused with "secret-keyring-locked" for the keyring entry "ladderback/demo"

  Scenario: A dismissed unlock prompt fails the run at once and isn't asked again
    Given the keyring's unlock prompt was dismissed
    When the schedule starts a backup 30 seconds after the daemon started
    Then the run fails 30 seconds after the daemon started because the keyring refused
    And the keyring entry "ladderback/demo" was read 1 time
    And the daemon never logged that the run waits
    And the run is refused with "secret-keyring-unavailable" for the keyring entry "ladderback/demo"

  Scenario: A person who tests the repository is answered at once
    Given the session's keyring service starts 55 seconds after the daemon
    When a person tests the repository 30 seconds after the daemon started
    Then the run fails 30 seconds after the daemon started because there is no keyring service to ask
    And the keyring entry "ladderback/demo" was read 1 time
    And the daemon never logged that the run waits

  Scenario: After 2 minutes of uptime a run with no keyring service fails at once
    Given the machine has no keyring service
    When the schedule starts a backup 120 seconds after the daemon started
    Then the run fails 120 seconds after the daemon started because there is no keyring service to ask
    And the keyring entry "ladderback/demo" was read 1 time
    And the daemon never logged that the run waits

  Scenario: A keyring service that never comes fails the run at the bound
    Given the machine has no keyring service
    When the schedule starts a backup 35 seconds after the daemon started
    Then the run fails 120 seconds after the daemon started because there is no keyring service to ask
    And the keyring entry "ladderback/demo" was read 10 times

  Scenario: A clock set back while a run waits doesn't make the wait longer
    Given the machine has no keyring service
    When the schedule starts a backup 35 seconds after the daemon started, and the clock is set back an hour once the run waits
    Then the run fails once the daemon has been up 120 seconds, because there is no keyring service to ask
    And the keyring entry "ladderback/demo" was read 10 times

  Scenario: A daemon stopped while a run waits for the keyring service ends the wait
    Given the machine has no keyring service
    When the schedule starts a backup 30 seconds after the daemon started and the daemon stops once the run waits
    Then the wait ended 30 seconds after the daemon started, and the run never started
    And the keyring entry "ladderback/demo" was read 1 time
