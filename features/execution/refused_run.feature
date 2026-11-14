Feature: A scheduled run refused before it starts
  A backup the schedule starts needs its repository's settings resolved
  first: the password, and every reference in the repository and its
  options. When one can't be resolved (a keyring entry or a password file
  that isn't there, a command that fails), nothing runs, and the run is kept
  as a failed backup of its plan, queued and failed in the same instant and
  never started. Its failure is the refusal's own code, naming where the
  secret was looked for and never a secret, and the plan's retries follow it
  as they follow any failed scheduled run. A backup left queued when the
  daemon stopped, and refused the same way when the next start takes it up
  again, ends failed with that refusal too, not because the daemon stopped.
  A run a person asks for is refused to them instead, and nothing is kept
  (technical design §5).

  Scenario: A scheduled run whose password isn't in the keyring is kept as a failed backup
    Given the repository's password is in the keyring under "ladderback/demo"
    When the schedule starts a backup of the plan "home" on the repository
    Then the operation ends as failed because the password's keyring entry "ladderback/demo" is missing
    And the operation never started
    And the history shows the operation as it ended
    And the run ends as a failed scheduled run of the plan "home"

  @e2e
  Scenario: A missed scheduled run whose password file is gone is kept as a failed backup after a restart
    Given Ladderback is serving on loopback with no repositories
    And the daemon is killed
    And the daemon's config file is edited by hand to hold:
      """
      schema = 1

      [repositories.demo]
      repository = "/srv/demo-repo"
      password-file = "/nonexistent/ladderback-demo/password"

      [[plans]]
      id = "home"
      repository = "demo"
      sources = ["/home/demo"]
      schedule = { every = "6h" }
      """
    And the schedule, while the daemon was down, kept the plan "home" due an hour ago
    When Ladderback is started again on the same folders
    Then the history comes to list one backup of the plan "home", failed with the code "secret-missing" and never started

  @e2e
  Scenario: A backup left queued whose password file is gone ends with its refusal after a restart
    Given Ladderback is serving on loopback with no repositories
    And the daemon is killed
    And the daemon's config file is edited by hand to hold:
      """
      schema = 1

      [repositories.demo]
      repository = "/srv/demo-repo"
      password-file = "/nonexistent/ladderback-demo/password"

      [[plans]]
      id = "home"
      repository = "demo"
      sources = ["/home/demo"]
      """
    And the log, while the daemon is down, shows a queued backup of the plan "home"
    When Ladderback is started again on the same folders
    Then the history comes to list one backup of the plan "home", failed with the code "secret-missing" and never started
