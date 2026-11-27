Feature: The daily copy of the database
  While the daemon serves, it keeps a copy of its database beside it in the
  data folder, `ladderback.db.daily`: the operation history is the one part
  of the database nothing else can bring back (§9). A copy is taken when
  there is none, and then whenever the one there is is a day old, so a
  restart within the day takes none. A copy that can't be taken, as while
  the database is unavailable, is said in the daemon's log and tried again
  an hour later; it never fails an operation, and the copy before it stays
  as it was. Times here are written with their offset from UTC.

  Background:
    Given the daemon's clock reads "2026-06-10 09:00+00:00"

  Scenario: A daemon with no copy takes one at once, then one each day
    Given the database has no daily copy
    When the daemon serves for 2 days and 12 hours
    Then the daily copy was taken at "2026-06-10 09:00+00:00", "2026-06-11 09:00+00:00" and "2026-06-12 09:00+00:00"

  Scenario: A restart within a day of the last copy takes none until the day is over
    Given the daily copy was taken at "2026-06-09 12:00+00:00"
    When the daemon serves until its clock reads "2026-06-10 11:59+00:00"
    Then no daily copy was taken
    When the daemon serves until its clock reads "2026-06-10 12:00+00:00"
    Then the daily copy was taken at "2026-06-10 12:00+00:00"

  Scenario: A copy dated after the clock, which was set back, is taken again at once
    Given the daily copy was taken at "2026-06-11 09:00+00:00"
    When the daemon serves until its clock reads "2026-06-10 09:01+00:00"
    Then the daily copy was taken at "2026-06-10 09:00+00:00"

  Scenario: A copy that can't be taken is said in the log and tried again an hour later
    Given the daily copy was taken at "2026-06-09 09:00+00:00"
    And the database is unavailable
    When the daemon serves until its clock reads "2026-06-10 09:30+00:00"
    Then no daily copy was taken
    And the daemon's log said once that the daily copy was skipped
    When the database becomes available again
    And the daemon serves until its clock reads "2026-06-10 10:30+00:00"
    Then the daily copy was taken at "2026-06-10 10:00+00:00"
