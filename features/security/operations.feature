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

  Scenario: A started backup appears in the history page, first running, then succeeded
    Given the plan's source holds many more files
    When the daemon starts a backup of the plan "home"
    Then the history page shows the operation running
    And the history page shows the operation succeeded

  Scenario: A page that arrives while a backup runs still sees it
    Given the plan's source holds many more files
    When the daemon starts a backup of the plan "home"
    Then the history page shows the operation running

  Scenario: The history page is newest first and honours its cursor and the plan filter
    When the daemon has finished 3 backups of the plan "home"
    Then the history page lists them newest first
    And the history page after the newest operation's cursor omits it
    And the history page filtered to the plan "home" lists all three
    And a history page asked for 1000 operations answers, not refuses

  Scenario: Cancelling a running backup stops it
    Given the plan's source holds many more files
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

  Scenario: The events WebSocket answers a ping
    Given a page has a live-updates connection
    When the page pings the daemon
    Then the page receives a pong

  Scenario: Two backups of the same plan both run
    Given the plan's source holds many more files
    When the daemon starts two backups of the plan "home"
    Then the history page shows both operations succeeded
