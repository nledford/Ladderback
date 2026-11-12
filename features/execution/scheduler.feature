Feature: The scheduler
  While the agent serves, it runs each plan that has a schedule at the plan's
  next run, and plans the run after it as the run starts (technical design
  §5). It keeps each planned run while it is off or asleep, so a run that
  passed meanwhile is found when it is back: a plan that catches up runs
  once, at once, and a plan that doesn't is skipped to its schedule's next
  moment. After a wake, the agent decides what became of each planned run
  before it starts any run that is due, and the catch-up runs wait together
  for the network first. A cron schedule is read in the time zone it names,
  or on the machine's clock when it names none, and a change to its time
  zone plans its next run again, as a change to its expression does. Times
  here are written with their offset from UTC.

  Scenario: A plan runs at its next run, and its run after that is planned
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    When the agent serves until "2026-06-10 03:30+00:00"
    Then the plan "home" ran once, on schedule, at "2026-06-10 03:00+00:00"
    And the next run of the plan "home" is planned for "2026-06-11 03:00+00:00"

  Scenario: A plan the agent finds for the first time waits for its schedule's first moment
    Given the agent starts at "2026-06-10 03:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    When the agent serves until "2026-06-10 23:00+00:00"
    Then the plan "home" hasn't run
    And the next run of the plan "home" is planned for "2026-06-11 03:00+00:00"

  Scenario: A run missed while the agent was off is made up for once, as it starts
    Given the agent starts at "2026-06-12 05:00+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the next run of the plan "home" was planned for "2026-06-10 03:00+00:00"
    When the agent serves until "2026-06-12 05:10+00:00"
    Then the plan "home" ran once, to catch up, at "2026-06-12 05:00+00:00"
    And the next run of the plan "home" is planned for "2026-06-13 03:00+00:00"

  Scenario: A plan that doesn't catch up skips the run it missed while the agent was off
    Given the agent starts at "2026-06-12 05:00+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" doesn't make up for runs it missed
    And the next run of the plan "home" was planned for "2026-06-10 03:00+00:00"
    When the agent serves until "2026-06-12 05:10+00:00"
    Then the plan "home" hasn't run
    And the next run of the plan "home" is planned for "2026-06-13 03:00+00:00"

  Scenario: A run missed while the machine slept catches up once the network is up
    Given the agent starts at "2026-06-10 02:00+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "docs" is scheduled "0 4 * * *"
    When the agent serves until "2026-06-10 02:30+00:00"
    And the network goes down
    And the machine sleeps until "2026-06-10 05:00+00:00"
    And the network comes back 40 seconds after the machine wakes
    And the agent serves until "2026-06-10 05:10+00:00"
    Then the plan "home" ran once, to catch up, once the network was up
    And the plan "docs" ran once, to catch up, once the network was up
    And the agent waited for the network once
    And the next run of the plan "home" is planned for "2026-06-11 03:00+00:00"

  Scenario: A plan that doesn't catch up skips the run it missed while the machine slept
    Given the agent starts at "2026-06-10 02:00+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" doesn't make up for runs it missed
    When the agent serves until "2026-06-10 02:30+00:00"
    And the machine sleeps until "2026-06-10 05:00+00:00"
    And the agent serves until "2026-06-10 05:10+00:00"
    Then the plan "home" hasn't run
    And the next run of the plan "home" is planned for "2026-06-11 03:00+00:00"

  Scenario: A run skipped for its conditions still plans the run after it
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" runs only on AC power
    And the machine runs on battery
    When the agent serves until "2026-06-10 03:30+00:00"
    Then the plan "home" hasn't run
    And the agent said the run of the plan "home" was skipped because the machine is on battery
    And the next run of the plan "home" is planned for "2026-06-11 03:00+00:00"

  Scenario: A cron schedule that names no time zone is read on the machine's clock
    Given the machine's time zone is "Europe/Berlin"
    And the agent starts at "2026-06-10 02:30+02:00"
    And the plan "home" is scheduled "0 3 * * *"
    When the agent serves until "2026-06-10 03:30+02:00"
    Then the plan "home" ran once, on schedule, at "2026-06-10 03:00+02:00"

  Scenario: A cron schedule is read in the time zone it names
    Given the machine's time zone is "UTC"
    And the agent starts at "2026-06-10 00:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    And the plan "home" reads its schedule in the time zone "Europe/Berlin"
    When the agent serves until "2026-06-10 01:30+00:00"
    Then the plan "home" ran once, on schedule, at "2026-06-10 01:00+00:00"
    And the next run of the plan "home" is planned for "2026-06-11 01:00+00:00"

  Scenario: A plan whose time zone changes is planned again
    Given the machine's time zone is "UTC"
    And the agent starts at "2026-06-10 00:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    When the agent serves until "2026-06-10 00:40+00:00"
    And the plan "home" reads its schedule in the time zone "Europe/Berlin"
    And the agent serves until "2026-06-10 01:30+00:00"
    Then the plan "home" ran once, on schedule, at "2026-06-10 01:00+00:00"
    And the next run of the plan "home" is planned for "2026-06-11 01:00+00:00"

  Scenario: A plan whose schedule is removed doesn't run again
    Given the agent starts at "2026-06-10 02:30+00:00"
    And the plan "home" is scheduled "0 3 * * *"
    When the agent serves until "2026-06-10 02:40+00:00"
    And the schedule of the plan "home" is removed
    And the agent serves until "2026-06-10 03:30+00:00"
    Then the plan "home" hasn't run
    And the plan "home" has no planned run
