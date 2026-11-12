@e2e
Feature: A configuration file with a mistake, read over HTTP
  A person edits the configuration file by hand, and the daemon reads it on
  every request (scheduling-and-config §6.1). When the file has a mistake in
  it, every request that reads it says where the mistake is, as a save does:
  the file, the line and the setting, and never a value the file holds (I4).

  Background:
    Given Ladderback is serving on loopback with no repositories
    And the daemon's config file is edited by hand to hold:
      """
      schema = 1

      [repositories.demo]
      repository = "/srv/demo-repo"
      password = "CANARY-inline-password"
      colour = "CANARY-inline-colour"
      """

  Scenario Outline: Asking to <request> names the line and the setting of a mistake
    When the client asks to <request>
    Then the refusal has status 503 and the typed code "config-invalid"
    And the refusal names the daemon's config file, line 6 and the setting "repositories.demo.colour"
    And no answer shows "CANARY-inline-password"
    And no answer shows "CANARY-inline-colour"

    Examples:
      | request                                     |
      | list the plans                              |
      | list the repositories                       |
      | start a backup of the plan "nightly"        |
      | list the snapshots of "demo"                |
      | list the snapshots of "demo" by owner       |
      | preview a check of "demo"                   |
      | start a check of "demo"                     |
