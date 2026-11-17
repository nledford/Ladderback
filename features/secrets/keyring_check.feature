Feature: Reading each keyring entry once, while someone is there
  On macOS the keychain asks once for each new build of the program before
  it lets that build read a `password-keyring` entry, so after a rebuild
  the first read waits for an answer, and at a scheduled backup nobody may
  be there to give it (#892). `ladderback check-keyring` reads, with the
  program that runs it, each keyring entry the configuration's repositories
  take their password from, once, so that the keychain asks while someone
  is at the terminal. For each repository it says the entry and whether it
  was read, or the refusal's code; never what the entry holds. A password
  kept anywhere else is left alone: no other source asks, and a command is
  never run. It fails when any entry couldn't be read (technical design
  §10.2).

  Rule: Each keyring entry is read once, and only keyring entries are read

    Background:
      Given the repository "nas" takes its password from the keyring entry "ladderback/nas"

    Scenario: An entry that can be read is reported read, and nothing it holds is reported
      Given the keyring holds "CANARY-nas-password" under "ladderback/nas"
      When the keyring entries are checked
      Then the check reports the repository "nas" and its keyring entry "ladderback/nas" read
      And the keyring entry "ladderback/nas" was read 1 time
      And the check's report holds no "CANARY-nas-password"
      And the check passes

    Scenario Outline: An entry that can't be read is reported with the refusal's code: <why>
      Given <why>
      When the keyring entries are checked
      Then the check reports the repository "nas" refused with "<code>"
      And the check fails

      Examples:
        | why                                       | code                       |
        | the keyring has no entry "ladderback/nas" | secret-missing             |
        | the keyring is locked                     | secret-keyring-unavailable |
        | the machine has no keyring service        | secret-keyring-unavailable |

    Scenario: Repositories that share an entry have it read once
      Given the keyring holds "CANARY-nas-password" under "ladderback/nas"
      And the repository "usb" takes its password from the keyring entry "ladderback/nas"
      When the keyring entries are checked
      Then the check reports the repository "nas" and its keyring entry "ladderback/nas" read
      And the check reports the repository "usb" and its keyring entry "ladderback/nas" read
      And the keyring entry "ladderback/nas" was read 1 time

    Scenario: One entry that can't be read fails the check, and the others are still read
      Given the keyring holds "CANARY-usb-password" under "ladderback/usb"
      And the repository "usb" takes its password from the keyring entry "ladderback/usb"
      When the keyring entries are checked
      Then the check reports the repository "nas" refused with "secret-missing"
      And the check reports the repository "usb" and its keyring entry "ladderback/usb" read
      And the check fails

    Scenario: Passwords kept elsewhere are left alone, and no command is run
      Given the keyring holds "CANARY-nas-password" under "ladderback/nas"
      And the repository "usb" takes its password from the command "/usr/local/bin/demo-password"
      And the repository "cloud" takes its password from the file "/home/demo/cloud.password"
      When the keyring entries are checked
      Then the check reports the repositories "nas" alone
      And nothing but the keyring entry "ladderback/nas" was read

  Rule: The command reads the configuration the daemon reads

    The examples name no keyring entry: one would ask the real keyring of
    the computer the tests run on.

    @e2e
    Scenario: A configuration with no keyring entry has nothing to read, and runs no command
      Given a repository reads its password from a command that leaves a mark
      When the owner runs "check-keyring"
      Then the command exits with status 0
      And standard output says "no repository reads its password from the keyring"
      And the repository's password command never ran

    @e2e
    Scenario: A configuration it can't read is refused
      Given the configuration file isn't valid TOML
      When the owner runs "check-keyring"
      Then the command exits with status 1
      And standard error says "config.toml"
