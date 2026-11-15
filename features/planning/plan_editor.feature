Feature: Creating, changing and deleting a plan
  A person creates a plan, changes any of its settings and deletes it on the
  Plans page, and Ladderback writes the change into the configuration as one
  configuration change, under the lock every write of the configuration
  takes (scheduling-and-config §6.1). The configuration's rules hold as they
  do when it loads: a plan's ID is its own, it names a repository the
  configuration has, and it backs up at least one source. The scheduler
  reads the configuration again as it serves, so a plan created or changed
  runs on its new schedule, and a deleted plan's snapshots stay in the
  repository: forget removes only snapshots a plan owns (I6), so nothing
  forgets them on its own any more.

  Scenario: A plan created while the agent serves runs on its schedule
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the agent has its repository and no plans
    When the agent serves until "2026-06-10 02:40+00:00"
    And a person creates the plan "home" scheduled "0 3 * * *"
    And the agent serves until "2026-06-10 03:30+00:00"
    Then the plan "home" ran once, on schedule, at "2026-06-10 03:00+00:00"
    And the next run of the plan "home" is planned for "2026-06-11 03:00+00:00"

  Scenario: A plan whose schedule a person changes runs at its new time
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    When the agent serves until "2026-06-10 02:40+00:00"
    And a person changes the schedule of the plan "home" to "0 4 * * *"
    And the agent serves until "2026-06-10 04:30+00:00"
    Then the plan "home" ran once, on schedule, at "2026-06-10 04:00+00:00"

  Scenario: A plan a person deletes doesn't run again
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    When the agent serves until "2026-06-10 02:40+00:00"
    And a person deletes the plan "home" while the agent serves
    And the agent serves until "2026-06-10 03:30+00:00"
    Then the plan "home" hasn't run
    And the plan "home" has no planned run

  Scenario: A plan whose ID another plan has is refused at its ID, and nothing is written
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    When a person creates the plan "home" scheduled "0 4 * * *"
    Then the change is refused at the setting "id" because another plan has the ID
    And the configuration still holds only the plan "home" scheduled "0 3 * * *"

  Scenario: A plan that names a repository the configuration doesn't have is refused at its repository
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the agent has its repository and no plans
    When a person creates the plan "home" into the repository "elsewhere"
    Then the change is refused at the setting "repository" because the configuration has no such repository
    And the configuration holds no plans

  Scenario: A change to a plan the configuration no longer has is refused
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the agent has its repository and no plans
    When a person changes the schedule of the plan "home" to "0 4 * * *"
    Then the change is refused because the configuration has no plan "home"
    And the configuration holds no plans

  Scenario: A deleted plan's snapshots stay, and forget no longer removes any of them
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When a person deletes the plan "home"
    And the owner forgets the repository
    Then the operation ends as succeeded
    And the repository still holds the 3 snapshots of the plan "home"
