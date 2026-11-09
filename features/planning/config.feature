Feature: An agent's configuration
  An agent is configured with the repositories it backs up into and the plans
  that say what to back up, when, and how much history to keep. The
  configuration is kept as one whole: it is loaded exactly as it was saved,
  and it is accepted only when its parts agree with each other. A person edits
  the file it is kept in, so a value that makes no sense is refused by what it
  was meant to be, never guessed at, and a refusal never repeats a command,
  which may hold a secret (invariant I4).

  Rule: A store holds one configuration, and the last one saved is it

    Scenario: A new agent has an empty configuration
      When the configuration is loaded
      Then the loaded configuration has 0 repositories
      And the loaded configuration has 0 plans

    Scenario: A saved configuration is loaded next
      Given a repository "nas"
      And a plan "home" backing up "/home/demo" into "nas"
      When the configuration is saved
      And the configuration is loaded
      Then the loaded configuration has 1 repository
      And the loaded configuration has the plan "home" backing up "/home/demo" into "nas"

    Scenario: The last save wins
      Given a repository "nas"
      And a plan "home" backing up "/home/demo" into "nas"
      And the configuration is saved
      And the plan "home" is dropped
      And a plan "docs" backing up "/home/demo/docs" into "nas"
      When the configuration is saved
      And the configuration is loaded
      Then the loaded configuration has 1 plan
      And the loaded configuration has the plan "docs" backing up "/home/demo/docs" into "nas"

  Rule: A configuration is accepted only when its plans agree with it

    Scenario: A plan must back up into a repository the configuration has
      Given a repository "nas"
      And a plan "home" backing up "/home/demo" into "nas"
      And a plan "work" backing up "/home/demo/work" into "cloud"
      When the configuration is built
      Then the configuration is refused because plan number 2, "work", names the repository "cloud", which it doesn't have

    Scenario: Plan IDs are unique
      Given a repository "nas"
      And a plan "home" backing up "/home/demo" into "nas"
      And a plan "work" backing up "/home/demo/work" into "nas"
      And a plan "home" backing up "/home/demo/other" into "nas"
      When the configuration is built
      Then the configuration is refused because plan number 3, "home", repeats the ID of an earlier plan

    Scenario: A plan needs something to back up
      Given a repository "nas"
      And a plan "home" backing up nothing into "nas"
      When the configuration is built
      Then the configuration is refused because plan number 1, "home", backs up nothing

    Scenario: The first plan to break a rule is the one reported
      Given a repository "nas"
      And a plan "home" backing up nothing into "nas"
      And a plan "work" backing up "/home/demo/work" into "cloud"
      When the configuration is built
      Then the configuration is refused because plan number 1, "home", backs up nothing

    Scenario: A configuration whose plans agree is accepted
      Given a repository "nas"
      And a repository "cloud"
      And a plan "home" backing up "/home/demo" into "nas"
      And a plan "work" backing up "/home/demo/work" into "cloud"
      When the configuration is built
      Then the configuration is accepted

  Rule: What a configuration doesn't say gets a safe default

    Scenario: A schedule says only when, and keeps the other defaults
      Given a repository "nas"
      And a plan "home" backing up "/home/demo" into "nas"
      And the plan "home" follows the cron expression "0 * * * *"
      When the configuration is built
      Then the plan "home" runs on the cron expression "0 * * * *"
      And the plan "home" makes up for a run it missed
      And the plan "home" runs on any power source
      And the plan "home" adds no jitter

    Scenario: A plan without a schedule runs only when asked
      Given a repository "nas"
      And a plan "home" backing up "/home/demo" into "nas"
      When the configuration is built
      Then the plan "home" has no schedule

    Scenario: A repository nobody said anything about is not maintained from here
      Given a repository "nas"
      When the configuration is built
      Then the repository "nas" has the maintenance "none"
      And the repository "nas" groups snapshots by "host,label,paths"

    Scenario: An agent runs one operation at a time unless told otherwise
      When the configuration is built
      Then the agent runs 1 operation at a time

  Rule: A value that makes no sense is refused by what it was meant to be

    Scenario Outline: A value the model can't use is refused
      When the <kind> "<value>" is read
      Then the value is refused as <kind>

      Examples:
        | kind             | value                  |
        | hostname         |                        |
        | source           | home/demo              |
        | locale           | en_US                  |
        | repository-alias | Not A Slug             |
        | concurrency      | 0                      |
        | nice             | 20                     |
        | cron             | every day              |
        | cron             | 0 0 31 2 *             |
        | duration         | 5 parsecs              |
        | attempts         | 0                      |
        | attempts         | 101                    |
        | keep-count       | -2                     |
        | keep-within      | -1d                    |
        | maintenance      | sometimes              |
        | group-by         | colour                 |
        | on-failure       | explode                |
        | hook-command     | echo 'never closed     |

    Scenario Outline: A value at the edge of what is allowed is accepted
      When the <kind> "<value>" is read
      Then the value is accepted as <kind>

      Examples:
        | kind             | value                  |
        | hostname         | demo-host              |
        | source           | /home/demo             |
        | locale           | en-US                  |
        | repository-alias | nas                    |
        | concurrency      | 64                     |
        | nice             | 19                     |
        | cron             | @daily                 |
        | duration         | 366 days               |
        | attempts         | 100                    |
        | attempts         | 1                      |
        | keep-count       | -1                     |
        | keep-within      | 2 years                |
        | maintenance      | hub                    |
        | group-by         |                        |
        | on-failure       | warn                   |
        | hook-command     | notify-send 'all done' |

  Rule: A refused hook command is never repeated

    Scenario: A command written as one string is split into words
      When the hook-command "notify-send 'backup done' --urgency=low" is read
      Then the value is accepted as hook-command
      And the hook runs the program "notify-send"
      And the hook's arguments are "backup done" and "--urgency=low"

    Scenario: A refused command is not repeated
      When the hook-command "login --password hunter2 'never closed" is read
      Then the value is refused as hook-command
      And the refusal doesn't mention "hunter2"

  @e2e
  Rule: The configuration is kept in a file that a person edits

    Scenario: A valid file is loaded as it was written
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        password-env = "DEMO_PASSWORD"

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the agent loads the config file
      Then the agent has the plan "home" backing up "/home/demo" into "nas"
      And the config file is exactly as it was written

    Scenario: A key nobody knows is refused where it is written, and no secret is repeated
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        password = "CANARY-inline-password"
        colour = "red"
        """
      When the agent loads the config file
      Then the load is refused because the key "repositories.nas.colour" is unknown
      And the refusal points at line 6 of the config file
      And the refusal never shows "CANARY-inline-password"

    Scenario: A file that doesn't say which version it is in is refused
      Given a config file holding:
        """
        [agent]
        concurrency = 2
        """
      When the agent loads the config file
      Then the load is refused because the file names no schema version
      And the config file is exactly as it was written

    Scenario: A file from a newer version is refused and left as it is
      Given a config file holding:
        """
        schema = 99

        [hub]
        url = "https://hub.example.lan:9899"
        """
      When the agent loads the config file
      Then the load is refused because the file is in version 99 and the agent reads version 1
      And the config file is exactly as it was written
      And no earlier version of the config file is kept

    Scenario: Secrets are kept as references when the configuration is saved again
      Given the file "{scratch}/s3-key" holds "CANARY-file-secret"
      And a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "s3:s3.example.lan/demo-bucket"
        password-command = "security find-generic-password -s demo-nas -w"
        options = { key = "${file:{scratch}/s3-key}" }
        """
      When the agent loads the config file
      And the agent saves the configuration it loaded
      Then the saved config file gives the repository "nas" the "password-command" "security find-generic-password -s demo-nas -w"
      And the saved config file gives the repository "nas" the option "key" as "${file:{scratch}/s3-key}"
      And the saved config file doesn't hold "CANARY-file-secret"

    Scenario: The old file is kept before a migration replaces it
      Given a config file holding:
        """
        schema = 1

        [agent]
        name = "demo-host"
        """
      And the agent reads version 2 of the format, where an agent's name became its hostname
      When the agent loads the config file
      Then the agent's hostname is "demo-host"
      And the history of the config file holds the old file exactly as it was written
      And the config file now says it is in version 2
