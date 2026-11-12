@e2e
Feature: Install the agent as a service
  A person who wants backups to run without a terminal open installs
  Ladderback as a service of their own user (operations §12): a systemd user
  unit on Linux, a launchd agent on macOS. `ladderback install-service`
  writes the service file into the user's folders and has the service
  manager start it, now and at each login; `ladderback remove-service` has
  the manager stop it and removes the file. The service runs this program's
  `serve`, and its file holds no secret: the agent reads its secrets from
  their sources when it runs, as it always does (I4).

  In the examples the home folder is a scratch folder, and the service
  manager is a stand-in that keeps a list of what it was asked and which
  services it runs: no real service is ever started.

  Background:
    Given the service manager is a stand-in

  Rule: Installing writes the service file and has the manager start it

    Scenario: Installing the service
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service file runs this program's "serve"
      And the service runs
      And standard output says "installed the service"

    Scenario: A program run by a package manager's link is named by the link
      A package manager such as Homebrew installs the program in a folder of
      its version and puts a link to it on the PATH; an upgrade removes the
      old version's folder and points the link at the new one. The service
      file names the link the owner ran, so the service still runs after an
      upgrade (decision 64).

      Given the program is run by a package manager's link on the PATH
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service file runs the program by the link

    Scenario: Installing over an installed service replaces its file and starts it again
      Given the service is installed
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service runs
      And the service manager stopped the service before it started it again
      And standard output says "replaced the service"

    Scenario: The service file and the manager never see a secret of the environment
      Given the environment sets "RESTIC_PASSWORD" to "CANARY-service-password"
      And the environment sets "AWS_SECRET_ACCESS_KEY" to "CANARY-service-key"
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service file never shows "CANARY-service"
      And the service manager never saw "CANARY-service"

    Scenario: The service keeps rustic's cache in a folder only its user reads
      On Linux the service manager makes the cache folder, private to the
      user, and the agent is pointed at it (decision 59); on macOS the agent
      makes its own cache folder private, as it always does.

      When the owner runs "install-service"
      Then the command exits with status 0
      And the service keeps rustic's cache in a private folder

    Scenario: A service manager that refuses is named, with its words
      Given the service manager refuses with "Failed to connect to bus: No medium found"
      When the owner runs "install-service"
      Then the command exits with status 1
      And standard error says "Failed to connect to bus: No medium found"
      And the service does not run

    Scenario: Installing with no home folder
      Given there is no home folder and no configuration folder is named
      When the owner runs "install-service"
      Then the command exits with status 1
      And standard error says "home folder"
      And the service manager was asked nothing

  Rule: Removing has the manager stop the service and removes its file

    Scenario: Removing the service
      Given the service is installed
      When the owner runs "remove-service"
      Then the command exits with status 0
      And the service does not run
      And there is no service file
      And standard output says "removed the service"

    Scenario: Removing a service that is not installed changes nothing
      When the owner runs "remove-service"
      Then the command exits with status 0
      And the service manager was asked nothing
      And standard output says "no service to remove"

  Rule: A file at the service file's place that Ladderback didn't write is left as it is

    Scenario Outline: A service file written by hand is neither replaced nor removed
      A unit or an agent the owner wrote under the same name, or systemd's
      mask, a link to /dev/null, is theirs: neither command changes it, and
      the service manager is asked nothing.

      Given a service file that Ladderback did not write
      When the owner runs "<command>"
      Then the command exits with status 1
      And standard error says "is not a service file Ladderback wrote"
      And the service file is still the one Ladderback did not write
      And the service manager was asked nothing

      Examples:
        | command         |
        | install-service |
        | remove-service  |

  Rule: The commands describe themselves in the reader's language

    Scenario Outline: The help of a service command
      When the owner runs "<command> --help"
      Then the command exits with status 0
      And standard output says "<words>"
      And the service manager was asked nothing

      Examples:
        | command         | words                           |
        | install-service | Install this agent as a service |
        | remove-service  | Stop the service                |
