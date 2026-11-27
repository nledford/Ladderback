@e2e
Feature: A person on the hub's page runs a backup on a connected machine
  A person signed in on the hub's page asks a connected machine to run one
  of its plans, and the machine's own daemon runs the backup as Back up now
  does and records that the hub started it for that person's session. The
  machine answers once the backup is queued and reads the repository's
  password after that, so a password it can't read fails that backup on
  the machine. A page on another site can't ask in the person's name: the
  hub refuses its request before anything reaches the machine (technical
  design §7.2, §10.3; §16, decision 176).

  Background:
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the machine has the plan "documents" over a real repository
    And the hub has stopped
    And the person approved "e2e-laptop" at the hub's terminal
    And the hub's admin password is "demo-admin-pass"
    And the hub serves again on the same address
    And the person signed in on the hub's page with "demo-admin-pass"
    And the machine's daemon starts
    And the hub soon shows "e2e-laptop" online

  Scenario: A person signed in on the hub's page runs a plan on the machine
    When the person runs "documents" on "e2e-laptop" from the hub's page
    Then the hub answers that the machine started a backup
    And the machine's backup soon succeeds
    And once the machine's daemon stops, its store says the hub started that backup for the person's session

  Scenario: A backup whose password the machine can't read is started, and fails on the machine
    Given the machine reads the repository's password from a variable that isn't set
    When the person runs "documents" on "e2e-laptop" from the hub's page
    Then the hub answers that the machine started a backup
    And the machine's backup soon fails, its password missing

  Scenario: A page on another site can't have the hub run a plan in the person's name
    When a page on another site asks the hub to run "documents" on "e2e-laptop" with the person's session
    Then the hub refuses with the code "origin-not-allowed"
    And once the machine's daemon stops, its store holds no backup
