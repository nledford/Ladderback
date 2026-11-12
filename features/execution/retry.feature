Feature: Retries of a failed backup
  A plan may retry a failed backup the schedule started: `retry = { attempts,
  backoff }` retries it at most `attempts` times, the first after `backoff`
  and each after twice the wait before it, at most an hour. Retries stop at
  the plan's next scheduled run, as it was planned when the failed run ended,
  and a success or a cancel ends them. A backup a person started is theirs:
  its failure starts no retries and leaves the plan's own as they were. A
  retry waits its turn in the persistent queue, as any backup does, under the
  agent's concurrency limit; it is skipped when a run of its plan is still
  going, whatever the plan's overlap policy, since that run takes its place.
  The agent announces each retry it plans, and a retry it planned is kept
  across a restart (technical design §5). Times here are written with their
  offset from UTC.

  Scenario: A failed backup is retried after the back-off
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And the runs of the plan "home" end "failed"
    When the agent serves until "2026-06-10 03:30+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:10+00:00"
    And the retries of the plan "home" started at "2026-06-10 03:10+00:00"
    And the agent announced retry 1 of 3 of the plan "home" for "2026-06-10 03:10+00:00"

  Scenario: The back-off doubles after each failed retry, to at most an hour
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 5 times, 20 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 07:30+00:00"
    Then the retries of the plan "home" started at "2026-06-10 03:20+00:00, 2026-06-10 04:00+00:00, 2026-06-10 05:00+00:00, 2026-06-10 06:00+00:00, 2026-06-10 07:00+00:00"
    And the agent announced retry 3 of 5 of the plan "home" for "2026-06-10 05:00+00:00"

  Scenario: Retries stop once their attempts are spent
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 2 times, 10 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 06:00+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:10+00:00, 2026-06-10 03:30+00:00"
    And the agent said the retries of the plan "home" ran out
    And the plan "home" has no pending retry

  Scenario: Retries stop at the next scheduled run
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 * * * *"
    And the plan "home" retries a failed backup 5 times, 20 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 04:10+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:20+00:00, 2026-06-10 04:00+00:00"
    And the retries of the plan "home" started at "2026-06-10 03:20+00:00"
    And the agent said the retries of the plan "home" stopped at its next scheduled run

  Scenario: A success ends the retries
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And the runs of the plan "home" end "failed, succeeded, failed"
    When the agent serves until "2026-06-10 05:00+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:10+00:00"
    And the plan "home" has no pending retry

  Scenario: A cancelled retry is not retried again
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And the runs of the plan "home" end "failed, cancelled, failed"
    When the agent serves until "2026-06-10 05:00+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:10+00:00"
    And the plan "home" has no pending retry

  Scenario: A retry waiting its turn when a person cancels a run of the plan is not retried again
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "docs" is scheduled "5 3 * * *"
    And the plan "home" is scheduled "0 3 * * *"
    And a run of the plan "docs" takes 30 minutes
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 03:15+00:00"
    And a person starts a backup of the plan "home"
    And the person cancels that backup while it waits for its turn
    And the agent serves until "2026-06-10 04:30+00:00"
    Then the retries of the plan "home" started at "2026-06-10 03:35+00:00"
    And the agent announced 1 retry of the plan "home"
    And the plan "home" has no pending retry

  Scenario: A backup started by hand that fails is not retried
    Given the agent starts at "2026-06-10 03:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 03:30+00:00"
    And a person starts a backup of the plan "home"
    And the agent serves until "2026-06-10 04:30+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:30+00:00"
    And the plan "home" has no pending retry

  Scenario: A backup started by hand that fails leaves the retry the schedule's failed run planned
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 03:05+00:00"
    And a person starts a backup of the plan "home"
    And the agent serves until "2026-06-10 03:20+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:05+00:00, 2026-06-10 03:10+00:00"
    And the retries of the plan "home" started at "2026-06-10 03:10+00:00"
    And the agent announced retry 2 of 3 of the plan "home" for "2026-06-10 03:30+00:00"

  Scenario: A backup started by hand that succeeds ends the retries
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And the runs of the plan "home" end "failed, succeeded"
    When the agent serves until "2026-06-10 03:05+00:00"
    And a person starts a backup of the plan "home"
    And the agent serves until "2026-06-10 04:00+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:05+00:00"
    And the plan "home" has no pending retry

  Scenario: A retry that comes due while a backup started by hand is going is skipped
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And a run of the plan "home" takes 30 minutes
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 03:35+00:00"
    And a person starts a backup of the plan "home"
    And the agent serves until "2026-06-10 05:00+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:35+00:00"
    And the agent said the retries of the plan "home" stopped because a run of it is going
    And the plan "home" has no pending retry

  Scenario: A retry waits its turn under the concurrency limit
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "docs" is scheduled "5 3 * * *"
    And the plan "home" is scheduled "0 3 * * *"
    And a run of the plan "docs" takes 30 minutes
    And the plan "home" retries a failed backup 1 time, 10 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 04:00+00:00"
    Then the retries of the plan "home" started at "2026-06-10 03:35+00:00"

  Scenario: A failed backup isn't retried while the plan's next run is waiting
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "*/30 * * * *"
    And the plan "home" queues a run that comes due while its last run is going
    And a run of the plan "home" takes 45 minutes
    And the plan "home" retries a failed backup 3 times, 1 minute apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 03:50+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00, 2026-06-10 03:45+00:00"
    And the agent said the retries of the plan "home" stopped because a run of it is going

  Scenario: A retry pending when the machine slept past the next scheduled run is dropped
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 * * * *"
    And the plan "home" doesn't make up for runs it missed
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 03:05+00:00"
    And the machine sleeps until "2026-06-10 04:30+00:00"
    And the agent serves until "2026-06-10 04:40+00:00"
    Then the runs of the plan "home" started at "2026-06-10 03:00+00:00"
    And the agent said the retries of the plan "home" stopped at its next scheduled run
    And the plan "home" has no pending retry

  Scenario: A retry pending when the agent stopped runs at its time after a restart
    Given the agent starts at "2026-06-10 03:15:10+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And every run of the plan "home" fails
    And the next run of the plan "home" was planned for "2026-06-11 03:00+00:00"
    And retry 2 of the plan "home" was pending for "2026-06-10 03:20+00:00", before the run planned for "2026-06-11 03:00+00:00"
    When the agent serves until "2026-06-10 03:30+00:00"
    Then the retries of the plan "home" started at "2026-06-10 03:20+00:00"
    And the agent announced retry 3 of 3 of the plan "home" for "2026-06-10 04:00+00:00"

  Scenario: A retry pending past attempts lowered since it was planned doesn't run
    Given the agent starts at "2026-06-10 02:50+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And every run of the plan "home" fails
    When the agent serves until "2026-06-10 03:15+00:00"
    And the plan "home" retries a failed backup 1 time, 10 minutes apart at first
    And the agent serves until "2026-06-10 04:30+00:00"
    Then the retries of the plan "home" started at "2026-06-10 03:10+00:00"
    And the agent said the retries of the plan "home" ran out
    And the plan "home" has no pending retry

  Scenario: A retry pending when the agent stopped gives way to the scheduled run that comes first
    Given the agent starts at "2026-06-10 04:30+00:00"
    And the plan "home" is scheduled "0 * * * *"
    And the plan "home" retries a failed backup 3 times, 10 minutes apart at first
    And the next run of the plan "home" was planned for "2026-06-10 04:00+00:00"
    And retry 2 of the plan "home" was pending for "2026-06-10 03:20+00:00", before the run planned for "2026-06-10 05:00+00:00"
    When the agent serves until "2026-06-10 04:40+00:00"
    Then the plan "home" ran once, to catch up, at "2026-06-10 04:30+00:00"
    And the agent said the retries of the plan "home" stopped at its next scheduled run
    And the plan "home" has no pending retry
