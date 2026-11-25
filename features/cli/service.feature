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
      And standard output says how to keep the service running while logged out

    Scenario: A user who lingers isn't told how to linger
      On Linux a user's services stop with the user's last session unless
      the user lingers (operations §12): `install-service` says how to
      linger only when `loginctl` doesn't say the user does already, or
      can't say (#546). A launchd agent on macOS runs without lingering, and
      nothing about it is said there, so the check is Linux's alone.

      Given the user lingers
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service runs
      And standard output never says how to linger

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

    Scenario: On a machine in hub mode, the service runs the hub with no flag of its own
      Hub mode is a setting of config.toml that `serve` reads at start, so
      the service file is the same on a hub as on any other machine, and
      turning hub mode on or off needs no new service file (decisions 136
      and 167).

      Given config.toml turns hub mode on, listening on a free port on loopback
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service file runs this program's "serve --service-managed"
      And the program the service file runs starts as the hub

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

    Scenario: The service manager gives the agent time to stop before it kills it
      A stop lets a backup that archives wind down, for up to 35 s, and the
      agent's whole stop takes 65 s at most. The service manager waits longer
      than that before it sends SIGKILL, so it never cuts a stop short
      (decision 50, #321).

      When the owner runs "install-service"
      Then the command exits with status 0
      And the service manager waits 75 seconds for the service to stop before it kills it

    Scenario: The service manager starts the agent again when it exits to restart
      A restart asked for in Settings is an orderly stop and an exit with
      status 75, which the service manager answers by starting the agent
      again. The service file tells the agent a manager runs it, so Settings
      offers the restart only then
      (`features/execution/restart_from_settings.feature`).

      When the owner runs "install-service"
      Then the command exits with status 0
      And the service file runs this program's "serve --service-managed"
      And the service manager starts the service again when it exits with status 75

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

  Rule: The service manager starts the agent again after any exit it didn't ask for
    Only the manager's own stop keeps the agent stopped: `remove-service`,
    and `systemctl --user stop` or `launchctl bootout` until the next login
    (operations §12, #1035). After any other exit, a crash, a refusal to
    start, a restart asked for in Settings or a stop another program asked
    for, the manager starts the agent again, systemd 10 s later and launchd
    within 10 s, so backups don't stop with it.

    Scenario: The agent stopped by another program's SIGTERM is started again
      Any program running as the owner can send the agent SIGTERM: a `kill`,
      or a `pkill -f` whose pattern matches the agent's command line. The
      agent stops in order and exits with status 0, as it does when its
      manager stops it, and the manager, which didn't ask for that stop,
      starts it again.

      Given the service is installed
      And Ladderback is serving on loopback under a service manager
      When a program other than the service manager sends the agent SIGTERM
      Then the daemon has exited with status 0
      And the service manager starts the service again

  Rule: On Linux it says when a repository's password can't be read before login
    A `password-keyring` password is read from the Secret Service, which
    starts with the user's desktop session. A service that lingers starts at
    boot, before anyone logs in, so a backup that runs before the first login
    can't read the password (operations §12, #479). On Linux `install-service`
    says so whenever a repository's password is in the keyring, and suggests
    a password file that only the user can read. A launchd agent on macOS
    runs only while its user is logged in, so nothing about it is said there.

    Scenario: A repository whose password is in the keyring is told of a password file
      Given a repository reads its password from the keyring
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service runs
      And standard output says, on Linux alone, that a password in the keyring can't be read before login

    Scenario: A repository whose password is in a file is told nothing of the keyring
      Given a repository reads its password from a file
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service runs
      And standard output never says that a password in the keyring can't be read before login

  Rule: It says when another Ladderback holds the data folder the service needs
    One process at a time holds a data folder (technical design §9, "One
    process per data dir"). A daemon started by hand with `ladderback serve`
    and still running holds it, so the service is refused at each start,
    which its manager tries again every 10 s, until that daemon stops (#459).
    `install-service` still installs and starts the service, and ends with
    status 0, but names the process that holds the folder and says the
    service takes over once it stops. The installed service holding the
    folder is the one being replaced, and nothing is said of it. The service
    runs on the platform's data folder: its file carries none of the shell's
    environment, so a daemon on the folder `LADDERBACK_DATA_DIR` names in the
    shell isn't in its way, and nothing is said of that one either.

    Scenario: Another Ladderback that holds the data folder is named
      Given another Ladderback holds the data folder
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service runs
      And standard output names the process that holds the data folder and says to stop it for the service to take over

    Scenario: The installed service that holds the data folder is not named
      Given the service is installed
      And the service's own process holds the data folder
      When the owner runs "install-service"
      Then the command exits with status 0
      And standard output says "replaced the service"
      And standard output never says that another Ladderback holds the data folder

    Scenario: A Ladderback on a data folder the service doesn't use is not named
      Given another Ladderback holds the data folder the shell names
      When the owner runs "install-service"
      Then the command exits with status 0
      And the service runs
      And standard output never says that another Ladderback holds the data folder

  Rule: Removing has the manager stop the service and removes its file

    Scenario: Removing the service
      Given the service is installed
      When the owner runs "remove-service"
      Then the command exits with status 0
      And the service does not run
      And the service manager does not start it again once its program has exited
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
        | install-service | Install Ladderback as a service |
        | remove-service  | Stop the service                |
