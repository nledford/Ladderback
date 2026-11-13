@e2e
Feature: The operation lifecycle
  A person starts, watches, cancels and reads back the operations of their
  plans (technical design §4.2). The history page is the record: newest
  first, one page at a time, and it holds running operations too, so a page
  that arrives late still sees them before it subscribes. Cancelling is
  polite: the operation stops, its finally-hooks run, and the log shows it
  cancelled.

  Background:
    Given Ladderback is serving on loopback with the plan "home"

  # A plan's after hook that waits until it is let go holds its backup
  # running: an operation has not ended until its ending hooks have, so the
  # history page finds it running however soon the engine finishes its part.
  Scenario: A started backup appears in the history page, first running, then succeeded
    Given the plan's after hook waits until it is let go
    When the daemon starts a backup of the plan "home"
    Then the history page shows the operation running
    When the plan's after hook is let go
    Then the history page shows the operation succeeded

  Scenario: A page that arrives while a backup runs still sees it
    Given the plan's after hook waits until it is let go
    When the daemon starts a backup of the plan "home"
    Then the history page shows the operation running

  Scenario: The history page is newest first and honours its cursor and the plan filter
    When the daemon has finished 3 backups of the plan "home"
    Then the history page lists them newest first
    And the history page after the newest operation's cursor omits it
    And the history page filtered to the plan "home" lists all three
    And a history page asked for 1000 operations answers, not refuses

  # A file in the plan's source that the backup waits on holds the backup
  # itself running: its worker reads that file before it saves anything,
  # and the read waits until the file is let go, which this scenario never
  # does. The cancel finds the backup running however fast the machine. A
  # worker already waiting there can't wind down, so the daemon ends it once
  # its 10 s grace is over; cancel.feature covers the polite wind-down.
  Scenario: Cancelling a running backup stops it
    Given the plan's source holds a file the backup waits on until it is let go
    And the plan runs a finally hook that notes it ran
    When the daemon starts a backup of the plan "home"
    And the operation is cancelled
    Then the history page shows the operation cancelled
    And the plan's finally hook has run

  Scenario: Cancelling a finished operation is refused
    When the daemon has finished 1 backup of the plan "home"
    And the operation is cancelled
    Then the refusal is typed "operation-not-running"

  Scenario: Cancelling an operation the log has none of is refused
    When the operation "018f0000-0000-7000-8000-00000000dead" is cancelled
    Then the refusal is typed "operation-unknown"

  Scenario: A history page whose cursor isn't an operation is refused
    When a history page is asked for with the cursor "not-an-operation"
    Then the refusal is typed "invalid-cursor"

  # The engine says why the backup failed in its own words, which it masks of
  # every secret of the job first (I4); the history carries them with the
  # failure's code, so a page can say why without the daemon's log (#474).
  Scenario: A backup that fails says why in the history, without its password
    Given the plan's repository password in the configuration is "canary-not-a-secret", which isn't the repository's
    When the daemon starts a backup of the plan "home"
    Then the history page shows the operation failed
    And the history page gives the operation's failure as "operation-failed", in the engine's own words
    And neither the history page nor the operation's detail shows "canary-not-a-secret"

  Scenario: Starting a backup whose password command fails is refused with that secret's problem
    Given the plan's repository reads its password from a command that prints "CANARY-start-password" and then fails saying "no such entry"
    When the client starts a backup of the plan "home"
    Then the refusal has status 422 and the typed code "secret-command-failed"
    And the refusal names the setting "password" and the command "/bin/sh"
    And the refusal says the command said "no such entry"
    And no answer shows "CANARY-start-password"
    And the history page lists no operation

  Scenario: The events WebSocket answers a ping
    Given a page has a live-updates connection
    When the page pings the daemon
    Then the page receives a pong

  Scenario: Two backups of the same plan both run
    Given the plan's source holds many more files
    When the daemon starts two backups of the plan "home"
    Then the history page shows both operations succeeded
