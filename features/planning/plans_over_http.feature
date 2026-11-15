@e2e
Feature: Plans created, changed and deleted over HTTP
  The Plans page creates, changes and deletes plans through the daemon's
  plan routes, which only a signed-in browser session may use, never an API
  token (security §10.1). Each change is read as the configuration file is
  read when it loads, so a refusal names the setting at fault as the loader
  does, and is written into `config.toml` the way a person would write it:
  what changed is set where the file has it, and the file's comments, blank
  lines and order stay (decision 69). The running daemon picks the change
  up: a plan created runs on its schedule.

  Scenario: A plan created through the API runs on its schedule
    Given Ladderback is serving on loopback with the plan "demo-home"
    When the client creates the plan "every-minute" over the source of "demo-home", scheduled "* * * * *"
    Then the history comes to list a scheduled backup of the plan "every-minute" that succeeded
    And the config file holds the plan "every-minute" scheduled "* * * * *"

  Scenario: A changed plan keeps the file's comments, blank lines and order
    Given Ladderback is serving on loopback with no repositories
    And the daemon's config file is edited by hand to hold:
      """
      schema = 1

      # The NAS in the hall cupboard.
      [repositories.demo]
      repository = "/srv/demo-repo"
      password-env = "DEMO_PASSWORD"

      # Documents, every night.
      [[plans]]
      id = "home"
      repository = "demo"
      sources = ["/home/demo"]   # only the home folder
      schedule = { cron = "0 3 * * *" }

      # Photos, by hand.
      [[plans]]
      id = "photos"
      repository = "demo"
      sources = ["/home/demo/Pictures"]
      """
    When the client changes the schedule of the plan "home" to "30 2 * * *"
    Then the daemon's config file reads:
      """
      schema = 1

      # The NAS in the hall cupboard.
      [repositories.demo]
      repository = "/srv/demo-repo"
      password-env = "DEMO_PASSWORD"

      # Documents, every night.
      [[plans]]
      id = "home"
      repository = "demo"
      sources = ["/home/demo"]   # only the home folder
      schedule = { cron = "30 2 * * *" }

      # Photos, by hand.
      [[plans]]
      id = "photos"
      repository = "demo"
      sources = ["/home/demo/Pictures"]
      """

  Scenario: A deleted plan goes from the file with its comment, and the rest stays
    Given Ladderback is serving on loopback with no repositories
    And the daemon's config file is edited by hand to hold:
      """
      schema = 1

      [repositories.demo]
      repository = "/srv/demo-repo"
      password-env = "DEMO_PASSWORD"

      # Documents, every night.
      [[plans]]
      id = "home"
      repository = "demo"
      sources = ["/home/demo"]

      # Photos, by hand.
      [[plans]]
      id = "photos"
      repository = "demo"
      sources = ["/home/demo/Pictures"]
      """
    When the client deletes the plan "home"
    Then the daemon's config file reads:
      """
      schema = 1

      [repositories.demo]
      repository = "/srv/demo-repo"
      password-env = "DEMO_PASSWORD"

      # Photos, by hand.
      [[plans]]
      id = "photos"
      repository = "demo"
      sources = ["/home/demo/Pictures"]
      """

  Scenario: An invalid schedule is refused at its field, and the file is left as it was
    Given Ladderback is serving on loopback with the plan "demo-home"
    When the client creates the plan "nightly" over the source of "demo-home", scheduled "61 * * * *"
    Then the refusal has status 422 and the typed code "plan-invalid"
    And the refusal names the setting "schedule.cron"
    And the config file holds no plan "nightly"

  Scenario: Deleting a plan a notification rule names is refused, naming the rule
    Given Ladderback is serving on loopback with no repositories
    And the daemon's config file is edited by hand to hold:
      """
      schema = 1

      [repositories.demo]
      repository = "/srv/demo-repo"
      password-env = "DEMO_PASSWORD"

      [[plans]]
      id = "home"
      repository = "demo"
      sources = ["/home/demo"]

      [notifications.channels.desk]
      kind = "ntfy"
      url-env = "DEMO_NTFY_URL"

      [notifications.rules.failures]
      events = ["backup.failed"]
      channel = "desk"
      plans = ["home"]
      """
    When the client deletes the plan "home"
    Then the refusal has status 409 and the typed code "plan-in-use"
    And the refusal names the rule "failures"
