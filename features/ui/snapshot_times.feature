@ui
Feature: The time a listed snapshot was taken
  Wherever a snapshot is listed (a plan's history on the Snapshots page, the
  orphaned and unmanaged groups, a forget review and an adoption review), its
  time is the snapshot time: what the clock that took it read, on the day a
  calendar retention rule counts it on (§16, decision 56). The offset of that
  clock follows the time only when it differs from the viewer's own offset at
  that moment, so someone in the zone the snapshots were taken in sees each
  time as their own clock reads it, with nothing added. The browser's zone is
  set for each scenario; the times of the operations page stay the viewer's.

  Scenario: A container whose clock runs in UTC, viewed from Chicago
    Given a demo plan "home" with a tiny scratch repository on a machine whose clock runs in "UTC"
    And the browser is in the time zone "America/Chicago"
    And the plan "home" has been backed up 2 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the table "Snapshots of plan home" has 2 rows
    And the table "Snapshots of plan home" shows its "Time" column as a clock in "UTC" read it, with the offset "UTC+00:00"

  Scenario: A forget review lists each snapshot at the time its own clock read
    Given a demo plan "home" with a tiny scratch repository on a machine whose clock runs in "UTC"
    And the browser is in the time zone "America/Chicago"
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the table "Snapshots forgotten from plan home" has 2 rows
    And the table "Snapshots forgotten from plan home" shows its "Time" column as a clock in "UTC" read it, with the offset "UTC+00:00"

  # Decision 56's laptop, flown from Auckland to Hawaii: two snapshots 46
  # hours apart, both taken on 10 November by their own clocks, viewed in
  # Hawaii. Only the one taken in Auckland names its offset.
  Scenario: A laptop that travelled shows each snapshot on the day its own clock read
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" holds snapshots taken at:
      | 2026-11-10 23:30:00-10:00 |
      | 2026-11-10 00:30:00+13:00 |
    And the browser is in the time zone "Pacific/Honolulu"
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the table "Snapshots of plan home" shows these times in its "Time" column:
      | 11/10/26, 11:30:00 PM           |
      | 11/10/26, 12:30:00 AM UTC+13:00 |

  Scenario: Someone in the zone the snapshots were taken in sees no offset
    Given a demo plan "home" with a tiny scratch repository on a machine whose clock runs in "America/Chicago"
    And the browser is in the time zone "America/Chicago"
    And the plan "home" has been backed up 1 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the table "Snapshots of plan home" has 1 rows
    And the table "Snapshots of plan home" shows its "Time" column in the time zone "America/Chicago"

  # The offset compared is the viewer's at the moment each snapshot was taken:
  # the night daylight saving time ends in Chicago, a viewer there sees both
  # snapshots without an offset, though their offsets differ.
  Scenario: A viewer whose clock changed for daylight saving time between two snapshots sees no offset
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" holds snapshots taken at:
      | 2026-11-01 23:30:00-06:00 |
      | 2026-11-01 00:30:00-05:00 |
    And the browser is in the time zone "America/Chicago"
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the table "Snapshots of plan home" shows these times in its "Time" column:
      | 11/1/26, 11:30:00 PM |
      | 11/1/26, 12:30:00 AM |
