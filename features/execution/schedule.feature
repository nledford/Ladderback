Feature: When a plan runs next
  A plan's schedule is a cron expression, read on the wall clock of its time
  zone, or an interval, counted in real time from the start of the plan's
  last run (technical design §5). The agent plans each run when the one
  before it starts, and keeps the planned run while it is off or asleep.
  A planned run that passed while the agent was away is made up for once,
  as soon as the agent is back, however many of the schedule's moments
  passed meanwhile: a catch-up. Jitter spreads a run over a stretch after
  its moment, so that machines on one schedule don't all start at once,
  and never as far as the schedule's following moment. When the clocks go
  forward, the moments they skip run once, at the time the clocks jump to;
  when they go back, each moment of the repeated hour runs once, the first
  time round. Times here are written with their offset from UTC.

  Scenario: A plan on a cron schedule runs at the schedule's next moment
    Given a plan scheduled "0 3 * * *" in the time zone "Europe/Berlin"
    When the plan's run starts at "2026-06-10 03:00+02:00"
    Then the plan's next run is at "2026-06-11 03:00+02:00"

  Scenario: A plan on an interval runs that long after its last run started
    Given a plan that runs every "6h"
    When the plan's run starts at "2026-06-10 03:17:20+00:00"
    Then the plan's next run is at "2026-06-10 09:17:20+00:00"

  Scenario: An interval counts real time when the clocks change
    Given a plan that runs every "1h" in the time zone "Europe/Berlin"
    When the plan's run starts at "2026-03-29 01:30+01:00"
    Then the plan's next run is at "2026-03-29 03:30+02:00"

  Scenario: Runs missed while the agent was off are made up for once
    Given a plan scheduled "0 * * * *" in the time zone "UTC"
    And the plan's next run was planned for "2026-06-10 10:00+00:00"
    When the agent comes back at "2026-06-10 15:20+00:00"
    Then the plan runs at once to catch up
    When the plan's run starts at "2026-06-10 15:20+00:00"
    Then the plan's next run is at "2026-06-10 16:00+00:00"

  Scenario: An interval shorter than the time away is made up for once
    Given a plan that runs every "1h"
    And the plan's next run was planned for "2026-06-10 10:00+00:00"
    When the agent comes back at "2026-06-10 15:20+00:00"
    Then the plan runs at once to catch up
    When the plan's run starts at "2026-06-10 15:20+00:00"
    Then the plan's next run is at "2026-06-10 16:20+00:00"

  Scenario: A plan that doesn't catch up skips the runs it missed
    Given a plan scheduled "0 * * * *" in the time zone "UTC"
    And the plan doesn't make up for runs it missed
    And the plan's next run was planned for "2026-06-10 10:00+00:00"
    When the agent comes back at "2026-06-10 15:20+00:00"
    Then the plan's next run is at "2026-06-10 16:00+00:00"

  Scenario: A run still to come when the agent comes back stays as planned
    Given a plan scheduled "0 * * * *" in the time zone "UTC"
    And the plan's runs are spread over "5m"
    And the plan's next run was planned for "2026-06-10 11:03+00:00"
    When the agent comes back at "2026-06-10 10:30+00:00"
    Then the plan's next run is at "2026-06-10 11:03+00:00"

  Scenario: A run planned before the clock was set back is planned again from the clock's time
    Given a plan scheduled "0 3 * * *" in the time zone "Europe/Berlin"
    And the plan's next run was planned for "2027-06-11 03:00+02:00"
    When the agent comes back at "2026-06-10 12:00+02:00"
    Then the plan's next run is at "2026-06-11 03:00+02:00"

  Scenario Outline: Jitter spreads a run over the stretch after its moment
    Given a plan scheduled "0 3 * * *" in the time zone "UTC"
    And the plan's runs are spread over "10m"
    And the random draw is the <draw> there is
    When the plan's run starts at "2026-06-10 03:04+00:00"
    Then the plan's next run is at "<next run>"

    Examples:
      | draw    | next run               |
      | lowest  | 2026-06-11 03:00+00:00 |
      | middle  | 2026-06-11 03:05+00:00 |
      | highest | 2026-06-11 03:10+00:00 |

  Scenario: Jitter never puts a run as late as the schedule's following moment
    Given a plan scheduled "0 * * * *" in the time zone "UTC"
    And the plan's runs are spread over "2h"
    And the random draw is the highest there is
    When the plan's run starts at "2026-06-10 09:00+00:00"
    Then the plan's next run is at "2026-06-10 10:59:59.999+00:00"

  Scenario: A time the clocks skip runs when they jump forward
    Given a plan scheduled "30 2 * * *" in the time zone "Europe/Berlin"
    When the plan's run starts at "2026-03-28 02:30+01:00"
    Then the plan's next run is at "2026-03-29 03:00+02:00"
    When the plan's run starts at "2026-03-29 03:00+02:00"
    Then the plan's next run is at "2026-03-30 02:30+02:00"

  Scenario: The moments of a skipped hour run once, when the clocks jump forward
    Given a plan scheduled "*/15 * * * *" in the time zone "Europe/Berlin"
    When the plan's run starts at "2026-03-29 01:45+01:00"
    Then the plan's next run is at "2026-03-29 03:00+02:00"
    When the plan's run starts at "2026-03-29 03:00+02:00"
    Then the plan's next run is at "2026-03-29 03:15+02:00"

  Scenario: A time the clocks repeat runs once, the first time round
    Given a plan scheduled "30 2 * * *" in the time zone "Europe/Berlin"
    When the plan's run starts at "2026-10-24 02:30+02:00"
    Then the plan's next run is at "2026-10-25 02:30+02:00"
    When the plan's run starts at "2026-10-25 02:30+02:00"
    Then the plan's next run is at "2026-10-26 02:30+01:00"

  Scenario: The moments of a repeated hour run once, the first time round
    Given a plan scheduled "*/30 * * * *" in the time zone "Europe/Berlin"
    When the plan's run starts at "2026-10-25 02:00+02:00"
    Then the plan's next run is at "2026-10-25 02:30+02:00"
    When the plan's run starts at "2026-10-25 02:30+02:00"
    Then the plan's next run is at "2026-10-25 03:00+01:00"

  Scenario: A zone without daylight saving time keeps its wall clock
    Given a plan scheduled "30 2 * * *" in the time zone "Asia/Tokyo"
    When the plan's run starts at "2026-03-28 02:30+09:00"
    Then the plan's next run is at "2026-03-29 02:30+09:00"
