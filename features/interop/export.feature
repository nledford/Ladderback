Feature: Export a plan as a rustic profile
  A person who leaves Ladderback, or wants to check a plan with the stock
  `rustic` CLI, exports a plan as a profile (technical design §6.2). The
  profile holds only what rustic reads, scopes forget to this plan's snapshots
  on this agent, and never holds a secret as text. Where Ladderback can't say
  something in rustic's words, the export says so, or refuses.

  Rule: An export is made from one plan of the configuration

    Scenario: The profile is made from the plan, its repository and the agent's own settings
      Given the configuration holds the repositories "nas" and "cloud"
      And the configuration holds the plan "home" for the repository "cloud"
      And the configuration holds the plan "work" for the repository "nas"
      And the agent's hostname is "demo-laptop"
      And the agent's hooks run "agent-hook" before everything
      When the owner exports the plan "home"
      Then the export succeeds
      And the profile is made for the plan "home" of this agent
      And the profile is made from the repository "cloud"
      And the profile carries the agent's hostname "demo-laptop"
      And the profile carries the agent's hooks

    Scenario: A plan the configuration does not have is refused
      Given the configuration holds the repository "nas"
      And the configuration holds the plan "home" for the repository "nas"
      When the owner exports the plan "ghost"
      Then the export is refused because the plan "ghost" is not configured
      And no profile was made

    Scenario: What the profile cannot say is refused, and no profile is made
      Given the configuration holds the repository "nas"
      And the configuration holds the plan "home" for the repository "nas"
      And the profile writer cannot put the key "repository" into a profile
      When the owner exports the plan "home"
      Then the export is refused because the profile cannot hold the key "repository"
      And no profile was made

    Scenario: What the profile left out comes back with it
      Given the configuration holds the repository "nas"
      And the configuration holds the plan "home" for the repository "nas"
      And the profile writer leaves out the option "token" for the variable "RUSTIC_REPO_OPT_TOKEN"
      When the owner exports the plan "home"
      Then the export succeeds
      And the export notes that the option "token" was left out for the variable "RUSTIC_REPO_OPT_TOKEN"

  @e2e
  Rule: The profile holds only what rustic reads, scoped to the plan

    Scenario: The plan's retention is applied to the plan's own snapshots only
      Given a config file holding:
        """
        schema = 1

        [agent]
        hostname = "demo-laptop"

        [repositories.nas]
        repository = "/srv/demo-repo"
        password-file = "/srv/demo-password"
        maintenance = "local"

        [[plans]]
        id = "home"
        repository = "nas"
        label = "demo home"
        sources = ["/home/demo", "/srv/demo data"]
        tags = ["laptop"]
        schedule = { cron = "0 * * * *" }
        retry = { attempts = 3, backoff = "10m" }
        git-ignore = true
        exclude-if-present = [".nobackup"]

        [plans.retention]
        keep-last = 3
        keep-daily = 7

        [[plans]]
        id = "work"
        repository = "nas"
        sources = ["/home/demo/work"]
        """
      When the owner exports the plan "home"
      Then the export succeeds
      And the exported profile has:
        | key                                   | value                                                          |
        | repository.repository                 | "/srv/demo-repo"                                               |
        | repository.password-file              | "/srv/demo-password"                                           |
        | backup.snapshots.0.name               | "home"                                                         |
        | backup.snapshots.0.sources            | ["/home/demo", "/srv/demo data"]                               |
        | backup.snapshots.0.label              | "demo home"                                                    |
        | backup.snapshots.0.host               | "demo-laptop"                                                  |
        | backup.snapshots.0.git-ignore         | true                                                           |
        | backup.snapshots.0.exclude-if-present | [".nobackup"]                                                  |
        | backup.snapshots.0.tags               | ["laptop", "ladderback-plan:home", "ladderback-agent:{agent}"] |
        | forget.keep-last                      | 3                                                              |
        | forget.keep-daily                     | 7                                                              |
        | forget.group-by                       | ""                                                             |
        | snapshot-filter.filter-tags           | ["ladderback-plan:home,ladderback-agent:{agent}"]              |
      And the exported profile doesn't set "forget.prune"
      And the exported profile doesn't hold "schedule"
      And the exported profile doesn't hold "retry"
      And the exported profile doesn't hold "maintenance"
      And the exported profile doesn't hold "/home/demo/work"

    Scenario: Hooks go back where rustic reads them
      Given a config file holding:
        """
        schema = 1

        [hooks]
        run-before = ["echo agent"]

        [repositories.nas]
        repository = "/srv/demo-repo"
        password-file = "/srv/demo-password"
        hooks = { run-before = ["echo repository"] }

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        hooks = { run-before = ["echo plan-before"], run-after = ["echo plan-after"] }
        """
      When the owner exports the plan "home"
      Then the export succeeds
      And the exported profile has:
        | key                                      | value                |
        | global.hooks.run-before                  | ["echo agent"]       |
        | repository.hooks.run-before              | ["echo repository"]  |
        | backup.snapshots.0.hooks.run-before      | ["echo plan-before"] |
        | backup.snapshots.0.hooks.run-after       | ["echo plan-after"]  |

    Scenario: Exporting a plan that is not configured is refused at the command line
      Given a config file holding:
        """
        schema = 1
        """
      When the owner exports the plan "ghost"
      Then the export is refused naming "ghost"

  @e2e
  Rule: A password is written as a way to read it, never as text

    Scenario Outline: A password the stock rustic can read the same way
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        <source>

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the owner exports the plan "home"
      Then the export succeeds
      And the exported profile has:
        | key                   | value   |
        | repository.<key>      | <value> |

      Examples:
        | source                                  | key              | value                    |
        | password-file = "/srv/demo-password"    | password-file    | "/srv/demo-password"     |
        | password-command = "pass show demo"     | password-command | "pass show demo"         |
        | password-env = "DEMO_PASSWORD"          | password-command | "printenv DEMO_PASSWORD" |

    Scenario: A password in the keyring is read with the platform's keyring command
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        password-keyring = "ladderback/demo"

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the owner exports the plan "home"
      Then the export succeeds
      And the exported profile reads the password from the keyring entry "ladderback/demo"

    Scenario: A password written in the config is refused, and never shown
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        password = "CANARY-inline-password"

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the owner exports the plan "home"
      Then the export is refused naming "password"
      And the export made no profile
      And the export never shows "CANARY-inline-password"

  @e2e
  Rule: A reference rustic cannot resolve is left out or refused, never copied

    Scenario: An option that is a reference is left out, with the variable to set instead
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        password-file = "/srv/demo-password"
        options = { region = "eu-demo-1", token = "${DEMO_TOKEN}" }

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the owner exports the plan "home"
      Then the export succeeds
      And the exported profile has:
        | key                       | value        |
        | repository.options.region | "eu-demo-1"  |
      And the exported profile doesn't hold "DEMO_TOKEN"
      And the export notes that "RUSTIC_REPO_OPT_TOKEN" is to be set instead

    Scenario: A repository that is a reference is refused, naming the key
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "${DEMO_REPOSITORY}"
        password-file = "/srv/demo-password"

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the owner exports the plan "home"
      Then the export is refused naming "repository"
      And the export made no profile

  @e2e
  Rule: What rustic would refuse, split or print is said or refused

    Scenario: A plan that keeps everything is exported, with a notice that rustic's forget will refuse the profile
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        password-file = "/srv/demo-password"

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the owner exports the plan "home"
      Then the export succeeds
      And the export warns that rustic's forget needs a keep option
      And the exported profile has:
        | key                         | value                                             |
        | forget.group-by             | ""                                                |
        | snapshot-filter.filter-tags | ["ladderback-plan:home,ladderback-agent:{agent}"] |
      And the exported profile doesn't set "forget.keep-last"

    Scenario: A repository address with a password in it is refused, naming the key and never the password
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "rest:https://demo:CANARY-url-password@nas.demo:8000/repo"
        password-file = "/srv/demo-password"

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the owner exports the plan "home"
      Then the export is refused naming "repository"
      And the export made no profile
      And the export never shows "CANARY-url-password"

    Scenario: An rclone connection string with a secret in it is refused, naming the key and never the secret
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "rclone::s3,provider=AWS,secret_access_key=CANARY-rclone-secret:bucket/repo"
        password-file = "/srv/demo-password"

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the owner exports the plan "home"
      Then the export is refused naming "repository"
      And the export made no profile
      And the export never shows "CANARY-rclone-secret"

    Scenario: An option with a user or a password in an address is left out like a secret
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        password-file = "/srv/demo-password"
        options = { region = "eu-demo-1", proxy = "demo:CANARY-proxy-password@proxy.demo:3128" }

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        """
      When the owner exports the plan "home"
      Then the export succeeds
      And the exported profile has:
        | key                       | value        |
        | repository.options.region | "eu-demo-1"  |
      And the exported profile doesn't hold "CANARY-proxy-password"
      And the export notes that "RUSTIC_REPO_OPT_PROXY" is to be set instead

    Scenario: A tag with a comma is refused, naming the tag
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        password-file = "/srv/demo-password"

        [[plans]]
        id = "home"
        repository = "nas"
        sources = ["/home/demo"]
        tags = ["weekly,laptop"]
        """
      When the owner exports the plan "home"
      Then the export is refused naming "weekly,laptop"
      And the export made no profile

  @e2e
  Rule: What is exported can be imported again

    Scenario: An exported plan comes back as the same plan, without Ladderback's own tags
      Given a config file holding:
        """
        schema = 1

        [repositories.nas]
        repository = "/srv/demo-repo"
        password-file = "/srv/demo-password"

        [[plans]]
        id = "home"
        repository = "nas"
        label = "demo home"
        sources = ["/home/demo"]
        tags = ["laptop"]

        [plans.retention]
        keep-last = 3
        """
      When the owner exports the plan "home"
      And the export is kept as the profile "nas.toml"
      And the config file is removed
      And the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import notes that the tag "ladderback-plan:home" was dropped
      And the saved config has:
        | key                            | value                |
        | plans.home.sources             | ["/home/demo"]       |
        | plans.home.label               | "demo home"          |
        | plans.home.tags                | ["laptop"]           |
        | plans.home.retention.keep-last | 3                    |
        | repositories.nas.password-file | "/srv/demo-password" |

  @e2e
  Rule: The stock rustic forgets exactly what Ladderback forgets

    Scenario: A dry run of rustic's forget removes the snapshots Ladderback's forget removes
      Given a new repository
      And a plan "home" backing up a folder with 3 files
      And the plan has been backed up 2 times
      And the plan's folder has moved
      And the plan has been backed up 2 times
      And the folder was backed up without Ladderback
      And the repository holds 2 snapshots of another agent's plan "home"
      And the repository's password is in the file "{scratch}/password"
      And a config file holding:
        """
        schema = 1

        [repositories.demo]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[plans]]
        id = "home"
        repository = "demo"
        sources = ["{scratch}/moved-source"]

        [plans.retention]
        keep-last = 2
        """
      When the owner exports the plan "home"
      And rustic is asked which snapshots its forget would remove, from the exported profile
      And the owner forgets the repository
      Then the operation ends as succeeded
      And rustic would have removed 2 snapshots
      And Ladderback removed exactly the snapshots rustic would have removed
