Feature: Full Disk Access on macOS
  On macOS, folders such as Mail, Messages and Safari, and other programs'
  containers, open only to a program the person has granted Full Disk Access
  in System Settings. Nothing can ask for the grant, so the agent checks for
  it: the Full Disk Access probe lists a folder only that grant opens. A
  refusal there means the grant is missing; any other answer, such as a
  missing folder, leaves it unknown, which is never reported as missing. The
  probe runs when the agent starts, before each backup, and when a person
  asks to check again (decision 115). A backup that a source item refuses,
  after a probe that found the grant missing, says so with a warning of its
  own, so its notification says why.

  Background:
    Given a plan "home" backs up "/Users/demo"

  Scenario: A granted agent says nothing of Full Disk Access
    Given the Full Disk Access probe finds the grant
    When the agent starts
    Then the log says nothing of Full Disk Access
    And the agent reports Full Disk Access as granted

  Scenario: A missing grant is logged once at start
    Given the Full Disk Access probe finds the grant missing
    When the agent starts
    Then the log says once that Full Disk Access is missing
    And the agent reports Full Disk Access as missing

  Scenario: An unknown grant is never reported as missing
    Given the Full Disk Access probe can't tell
    When the agent starts
    Then the log says nothing of Full Disk Access
    And the agent reports Full Disk Access as unknown

  Scenario: A refused item, with the grant missing, gets the Full Disk Access warning
    Given the Full Disk Access probe finds the grant missing
    And the engine is refused "/Users/demo/Library/Mail" with "Operation not permitted (os error 1)"
    When the plan "home" backs up
    Then the operation ends as succeeded with warnings
    And the operation has a "full-disk-access-missing" warning for "/Users/demo/Library/Mail"
    And the operation has no "source-unreadable" warning

  Scenario: A refused item, with the grant found, stays a source that couldn't be read
    Given the Full Disk Access probe finds the grant
    And the engine is refused "/Users/demo/Library/Mail" with "Operation not permitted (os error 1)"
    When the plan "home" backs up
    Then the operation has a "source-unreadable" warning for "/Users/demo/Library/Mail"
    And the operation has no "full-disk-access-missing" warning

  Scenario: A refused item, with the grant unknown, stays a source that couldn't be read
    Given the Full Disk Access probe can't tell
    And the engine is refused "/Users/demo/Library/Mail" with "Operation not permitted (os error 1)"
    When the plan "home" backs up
    Then the operation has a "source-unreadable" warning for "/Users/demo/Library/Mail"
    And the operation has no "full-disk-access-missing" warning

  Scenario: An item that can't be read for another reason stays a source that couldn't be read
    Given the Full Disk Access probe finds the grant missing
    And the engine is refused "/Users/demo/locked" with "Permission denied (os error 13)"
    When the plan "home" backs up
    Then the operation has a "source-unreadable" warning for "/Users/demo/locked"
    And the operation has no "full-disk-access-missing" warning

  Scenario: The probe runs again before each backup
    Given the Full Disk Access probe finds the grant missing
    And the agent has started
    And the Full Disk Access probe now finds the grant
    And the engine is refused "/Users/demo/Library/Mail" with "Operation not permitted (os error 1)"
    When the plan "home" backs up
    Then the operation has a "source-unreadable" warning for "/Users/demo/Library/Mail"
    And the agent reports Full Disk Access as granted

  Scenario: Checking again finds a grant given since
    Given the Full Disk Access probe finds the grant missing
    And the agent has started
    And the Full Disk Access probe now finds the grant
    When a person asks the agent to check Full Disk Access again
    Then the agent reports Full Disk Access as granted

  Scenario: The Full Disk Access warning reaches notifications
    Given a webhook channel "phone"
    And a rule "warnings" that sends "backup.warning" to "phone"
    And the Full Disk Access probe finds the grant missing
    And the engine is refused "/Users/demo/Library/Mail" with "Operation not permitted (os error 1)"
    When the plan "home" backs up
    Then "phone" is sent "backup.warning" for the plan "home" with the code "full-disk-access-missing"
