Feature: The overlap policy and the concurrency limit
  A plan runs one backup at a time. When a plan's run comes due while its
  last run is still going, the plan's overlap policy decides what becomes of
  it: `skip`, the default, leaves it out and says so in the agent's log, and
  `queue` puts it in the persistent queue, to start once the last run ends.
  One run waiting is enough: a plan never has a second one waiting, under
  either policy. Across plans, the agent runs at most as many backups at
  once as its concurrency limit, 1 unless the configuration says otherwise;
  a run beyond it waits in the persistent queue, and as soon as a running
  backup ends the oldest waiting run that can start starts (technical
  design §5). Times here are written with their offset from UTC.

  Scenario: A run that comes due while the plan's last run is going is skipped by default
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "*/30 * * * *"
    And a run of the plan "home" takes 45 minutes
    When the agent serves until "2026-06-10 04:10+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 04:00+00:00"
    And the agent said the run of the plan "home" was skipped because its last run is still going
    And the next run of the plan "home" is planned for "2026-06-10 04:30+00:00"

  Scenario: A plan that queues its overlapping run starts it once the last run ends
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "*/30 * * * *"
    And the plan "home" queues a run that comes due while its last run is going
    And a run of the plan "home" takes 45 minutes
    When the agent serves until "2026-06-10 03:50+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:45+00:00"

  Scenario: A plan that queues keeps one run waiting, never two
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "*/30 * * * *"
    And the plan "home" queues a run that comes due while its last run is going
    And a run of the plan "home" takes 70 minutes
    When the agent serves until "2026-06-10 04:20+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 04:10+00:00"
    And the agent said the run of the plan "home" was skipped because a run of it is already waiting

  Scenario: The agent runs one backup at a time unless told otherwise
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the plan "docs" is scheduled "0 3 * * *"
    And the plan "home" is scheduled "0 3 * * *"
    And a run of the plan "docs" takes 20 minutes
    When the agent serves until "2026-06-10 03:10+00:00"
    Then the runs of the plan "docs" started at "2026-06-10 03:00+00:00"
    And the plan "home" hasn't run

  Scenario: A run waiting for the limit starts as soon as a running backup ends
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the plan "docs" is scheduled "0 3 * * *"
    And the plan "home" is scheduled "0 3 * * *"
    And a run of the plan "docs" takes 20 minutes
    When the agent serves until "2026-06-10 03:30+00:00"
    Then the runs of the plan "docs" started at "2026-06-10 03:00+00:00"
    And the runs of the plan "home" started at "2026-06-10 03:20+00:00"

  Scenario: The limit counts the running backups of every plan
    Given the agent runs at most 2 backups at once
    And the agent starts at "2026-06-10 02:30+00:00"
    And the plan "docs" is scheduled "0 3 * * *"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "photos" is scheduled "0 3 * * *"
    And a run of the plan "docs" takes 20 minutes
    And a run of the plan "home" takes 30 minutes
    When the agent serves until "2026-06-10 03:40+00:00"
    Then the runs of the plan "docs" started at "2026-06-10 03:00+00:00"
    And the runs of the plan "home" started at "2026-06-10 03:00+00:00"
    And the runs of the plan "photos" started at "2026-06-10 03:20+00:00"

  Scenario: A run that comes due while the plan's last run still waits for the limit is skipped
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "docs" is scheduled "0 3 * * *"
    And the plan "home" is scheduled "*/30 * * * *"
    And a run of the plan "docs" takes 50 minutes
    When the agent serves until "2026-06-10 03:55+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:50+00:00"
    And the agent said the run of the plan "home" was skipped because a run of it is already waiting

  Scenario: A raised limit starts a run waiting for it
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the plan "docs" is scheduled "0 3 * * *"
    And the plan "home" is scheduled "0 3 * * *"
    And a run of the plan "docs" takes 60 minutes
    When the agent serves until "2026-06-10 03:10+00:00"
    And the agent runs at most 2 backups at once
    And the agent serves until "2026-06-10 03:20+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:10:30+00:00"

  Scenario: A lowered limit holds back the runs waiting for it
    Given the agent runs at most 2 backups at once
    And the agent starts at "2026-06-10 02:30+00:00"
    And the plan "docs" is scheduled "0 3 * * *"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "photos" is scheduled "0 3 * * *"
    And a run of the plan "docs" takes 20 minutes
    And a run of the plan "home" takes 30 minutes
    When the agent serves until "2026-06-10 03:10+00:00"
    And the agent runs at most 1 backup at once
    And the agent serves until "2026-06-10 03:40+00:00"
    Then the runs of the plan "photos" started at "2026-06-10 03:30+00:00"

  Scenario: A changed overlap policy applies to the plan's next due run
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "*/30 * * * *"
    And a run of the plan "home" takes 45 minutes
    When the agent serves until "2026-06-10 03:10+00:00"
    And the plan "home" queues a run that comes due while its last run is going
    And the agent serves until "2026-06-10 03:50+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:45+00:00"
