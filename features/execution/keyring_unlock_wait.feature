Feature: A run whose keyring is locked waits for it to be unlocked
  A run nobody waits on whose repository password is in the keyring isn't
  refused at once when the keyring service answers that its keyring is
  locked: the run waits for the keyring to be unlocked, asking only whether
  it is locked — never reading the entry, so waiting can raise no unlock
  prompt — and reads the entry again the moment it is. It gives up when the
  plan's next run is due, never waiting past its successor, and is refused
  once as locked. A keyring with no service to ask still waits only while
  the daemon starts (decision 116), and one whose unlock prompt was
  dismissed still fails at once. Stopping the daemon ends the wait, and a
  person who asked is answered at once (#1221).

  Background:
    Given the daemon started a moment ago
    And the keyring holds "CANARY-keyring-password" under "ladderback/demo"
    And the repository's password is in the keyring under "ladderback/demo"
    And the repository is "/srv/demo-repo"
    And the plan's next run is due 600 seconds after the daemon started

  Scenario: A run that fires while the keyring is locked waits for it to be unlocked
    Given the keyring unlocks 55 seconds after the daemon started
    When the schedule starts a backup 30 seconds after the daemon started
    Then the run's secrets are resolved 60 seconds after the daemon started
    And the keyring entry "ladderback/demo" was read 2 times
    And the daemon asked whether the keyring is locked 4 times

  Scenario: A keyring that never unlocks gives the run up when the plan's next run is due
    Given the keyring is locked
    And the plan's next run is due 3600 seconds after the daemon started
    When the schedule starts a backup 30 seconds after the daemon started
    Then the run fails 3600 seconds after the daemon started because the keyring is locked
    And the run is refused with "secret-keyring-locked" for the keyring entry "ladderback/demo"
    And the keyring entry "ladderback/demo" was read 1 time

  Scenario: An entry still locked once the keyring says it is unlocked is read again only every 10 seconds
    Given the keyring says it is unlocked, but its entries stay locked
    And the plan's next run is due 120 seconds after the daemon started
    When the schedule starts a backup 30 seconds after the daemon started
    Then the run fails 120 seconds after the daemon started because the keyring is locked
    And the keyring entry "ladderback/demo" was read 10 times

  Scenario: A run that starts once its plan's next run is due is refused without asking whether the keyring is locked
    Given the keyring is locked
    And the plan's next run is due 20 seconds after the daemon started
    When the schedule starts a backup 30 seconds after the daemon started
    Then the run fails 30 seconds after the daemon started because the keyring is locked
    And the daemon asked whether the keyring is locked 0 times

  Scenario: The daemon logs the unlock wait once, naming the keyring entry and no secret
    Given the keyring unlocks 45 seconds after the daemon started
    When the schedule starts a backup 30 seconds after the daemon started
    Then the daemon logged once that the run waits for the keyring entry "ladderback/demo"
    And what the daemon logged of the wait holds no "CANARY-keyring-password"

  Scenario: Stopping the daemon while a run waits for the keyring ends the wait
    Given the keyring is locked
    When the schedule starts a backup 30 seconds after the daemon started and the daemon stops once the run waits
    Then the wait ended 30 seconds after the daemon started, and the run never started

  Scenario: A keyring service that goes away after the keyring unlocked is waited for only until the plan's next run is due
    Given the keyring unlocks 20 seconds after the daemon started
    And the keyring service goes away 26 seconds after the daemon started
    And the daemon takes 5 seconds to say whether the keyring is locked
    And the plan's next run is due 40 seconds after the daemon started
    When the schedule starts a backup 10 seconds after the daemon started
    Then the run fails 40 seconds after the daemon started because there is no keyring service to ask
    And the keyring entry "ladderback/demo" was read 3 times

  Scenario: An unlocked answer that comes after the plan's next run is due is not acted on
    Given the keyring unlocks 15 seconds after the daemon started
    And the daemon takes 6 seconds to say whether the keyring is locked
    And the plan's next run is due 30 seconds after the daemon started
    When the schedule starts a backup 10 seconds after the daemon started
    Then the run fails 32 seconds after the daemon started because the keyring is locked
    And the keyring entry "ladderback/demo" was read 1 time
    And the daemon asked whether the keyring is locked 2 times
