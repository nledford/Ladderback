@e2e @needs-node
Feature: Update the service from a checkout without interrupting an operation
  `just install-restart` (scripts/install-restart.ts) builds Ladderback from
  a checkout into a staging folder beside the installed program, checks the
  new program there, waits until the daemon has no operation running or
  queued, then moves the new program over the installed one and restarts
  the service on it at once. It reads what is running and queued from the
  daemon's operation log, read-only, so it works whether sign-in is on or
  off, and it doesn't restart a daemon when it can't tell whether that
  daemon is busy. It confirms the restart by the service running a new
  process, the new program naming the checkout's build and the daemon
  answering; with sign-in off the daemon names that build too.

  In the examples the build only copies a stand-in for the new program
  where it is told to install it, the service manager is a stand-in, the new
  program is the real one except that it names the build the scenario chose,
  and the daemon's address is a stand-in that says only whether sign-in is
  on and which build it runs. The operation log is a real one, in the data
  folder the service's process holds.

  Background:
    Given the service manager is a stand-in
    And the service is installed
    And the service's daemon keeps its operation log, with sign-in on

  Rule: It restarts an idle daemon, and confirms the restart

    Scenario: With sign-in on, the new process and the installed program confirm the build
      When the owner runs install-restart
      Then the command exits with status 0
      And the service runs a new process
      And standard output says "the installed program says it is ladderback"
      And standard output says "with sign-in on, it names its build only to a session"

    Scenario: With sign-in off, the daemon names the build it runs
      Given the daemon at the service's address has sign-in off
      When the owner runs install-restart
      Then the command exits with status 0
      And the service runs a new process
      And standard output says "says it runs ladderback"

    Scenario: A restart that leaves the same process running isn't confirmed
      Given the service manager keeps the service's process when it restarts it
      When the owner runs install-restart
      Then the command exits with status 1
      And standard error says "the service manager still names process"

    Scenario: A new program that isn't the checkout's build is neither installed nor started
      Given the new program says it is "ladderback 0.0.1"
      When the owner runs install-restart
      Then the command exits with status 1
      And the service runs the same process
      And the installed program is still the one from before
      And no staging folder is left
      And standard error says "the new program says ladderback 0.0.1, not ladderback"

  Rule: It replaces the installed program only once the daemon is idle

    Until the restart, the program on disk stays the one the running daemon
    runs: macOS's keychain refuses a read from a process whose program on
    disk has changed under it, so a scheduled backup that starts during the
    wait would fail (#1107). The new program is built into a staging folder
    in the installed program's folder, so on the same file system, and is
    checked there. Once the daemon is idle, one rename puts it in place, and
    the service restarts at once.

    Scenario: The new program is checked in a staging folder, then put in place for the restart
      When the owner runs install-restart
      Then the command exits with status 0
      And the new program named its build and read the keyring entries before it was put in place
      And the installed program is the new one
      And the service was restarted by the new program in place
      And no staging folder is left

    Scenario: A busy daemon's program stays in place while install-restart waits
      Given the operation log shows a backup running
      When the owner runs install-restart with "--wait 0"
      Then the command exits with status 1
      And the installed program is still the one from before
      And no staging folder is left
      And standard error says "the installed program is untouched"

    Scenario: A failed build leaves the installed program in place
      Given the new program's build fails
      When the owner runs install-restart
      Then the command exits with status 1
      And the service runs the same process
      And the installed program is still the one from before
      And no staging folder is left
      And standard error says "nothing was installed"

    Scenario: Stopping install-restart while it waits leaves the installed program in place
      Given the operation log shows a backup running
      When the owner stops install-restart while it waits
      Then the command exits with status 130
      And the service runs the same process
      And the installed program is still the one from before
      And no staging folder is left

    Scenario: Stopping install-restart while it builds leaves the installed program in place
      Given install-restart is told to stop while it builds
      When the owner runs install-restart with "--now"
      Then the command exits with status 143
      And the service runs the same process
      And the installed program is still the one from before
      And no staging folder is left

    Scenario Outline: An install root with no folder is refused before anything is built
      When the owner runs install-restart with "<args>"
      Then the command exits with status 2
      And the installed program is still the one from before
      And no staging folder is left
      And standard error says "--root needs a folder"

      Examples:
        | args           |
        | --root         |
        | --root=        |
        | --root --force |

  Rule: It waits while an operation runs or is queued

    Scenario Outline: A daemon with an operation <status> isn't restarted while it lasts
      Given the operation log shows a backup <status>
      When the owner runs install-restart with "--wait 0"
      Then the command exits with status 1
      And the service runs the same process
      And standard error says "<said>"

      Examples:
        | status  | said                                           |
        | running | still busy after 0 min (1 running, 0 queued)   |
        | queued  | still busy after 0 min (0 running, 1 queued)   |

    Scenario: --now restarts a busy daemon without waiting
      Given the operation log shows a backup running
      When the owner runs install-restart with "--now"
      Then the command exits with status 0
      And the service runs a new process

  Rule: It doesn't restart a daemon when it can't tell whether it is busy

    Scenario Outline: <surprise>: not restarted
      Given <surprise>
      When the owner runs install-restart
      Then the command exits with status 1
      And the service runs the same process
      And standard error says "can't tell whether the daemon is busy"
      And standard error says "<why>"
      And standard error says "Not installing or restarting; --now installs and restarts without asking."

      Examples:
        | surprise                                                                      | why                           |
        | there is no operation log                                                     | there is no operation log at  |
        | the operation log isn't a database                                            | file is not a database        |
        | the operation log comes from a version that keeps operations elsewhere        | no such table: operations     |
        | the operation log comes from a version that keeps no status for an operation  | no such column: status        |
        | the operation log holds an operation whose status is "paused"                 | whose status is paused, which |
        | another process holds the operation log locked                                | database is locked            |
        | another process holds the data folder                                         | not the service's process     |

    Scenario: A service that isn't running isn't restarted
      Given the service's process has ended
      When the owner runs install-restart
      Then the command exits with status 1
      And standard error says "can't tell whether the daemon is busy: the service isn't running"

  Rule: It reads the keyring entries with the new program before it restarts

    On macOS the keychain asks once for each new build before it lets the
    build read a keyring entry. The new program reads each entry once
    (`ladderback check-keyring`), in the terminal, before the wait and the
    restart, so the keychain asks while someone is there and not at the
    next scheduled backup or notification.

    Scenario: The new program reads the keyring entries before the service restarts
      When the owner runs install-restart
      Then the command exits with status 0
      And the new program read the keyring entries before the service was restarted
      And standard output says "no repository or notification channel takes a secret from the keyring"

    Scenario: The keyring entries are read before the wait for a busy daemon
      Given the operation log shows a backup running
      When the owner runs install-restart with "--wait 0"
      Then the command exits with status 1
      And the new program read the keyring entries
      And the new program never restarted the service

    Scenario: A keyring entry the new program can't read stops it before the restart
      Given the new program can't read a keyring entry
      When the owner runs install-restart
      Then the command exits with status 1
      And the service runs the same process
      And the new program never restarted the service
      And the installed program is still the one from before
      And standard error says "could not read the keyring entry ladderback/nas"
      And standard error says "the new program couldn't read every keyring entry"
      And standard error says "--ignore-keyring-failure restarts all the same"

    Scenario: --ignore-keyring-failure restarts despite a keyring entry the new program can't read
      Given the new program can't read a keyring entry
      When the owner runs install-restart with "--ignore-keyring-failure"
      Then the command exits with status 0
      And the service runs a new process
      And standard error says "the new program couldn't read every keyring entry"
      And standard output says "--ignore-keyring-failure: restarting all the same"
