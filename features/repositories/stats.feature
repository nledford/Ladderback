@repositories
Feature: A repository's stats
  A repository's stats are four figures: how many snapshots it holds, their
  total size (the sources they hold, added up), the size the repository
  stores for them, and the share of the data's raw size compression saved.
  They are read when a person asks and after each check this agent runs,
  and every reading is kept in the history for a year, so it can be charted
  later (§16, decision 104). A read is not an operation: like a listing, it
  takes no maintenance slot, waits for no maintenance owner, and runs
  alongside whatever else runs on the repository.

  Scenario: A repository's stats are read and kept
    Given the repository "demo" holds 3 snapshots of 1000 bytes each, stored in 1200 bytes of 2400 raw
    When the person reads the stats of "demo"
    Then the reading shows 3 snapshots, a total size of 3000 bytes and a stored size of 1200 bytes
    And the reading shows that compression saved 50 percent
    And the history keeps the reading of "demo", taken now

  Scenario: An empty repository has no compression to show
    Given the repository "demo" holds no snapshots and stores nothing
    When the person reads the stats of "demo"
    Then the reading shows 0 snapshots, a total size of 0 bytes and a stored size of 0 bytes
    And the reading shows no compression
    And the history keeps the reading of "demo", taken now

  Scenario: A repository another host maintains at the time is read like any other
    Given the repository "demo" holds 3 snapshots of 1000 bytes each, stored in 1200 bytes of 2400 raw
    And the hub maintains the repository "demo", not this agent
    When the person reads the stats of "demo"
    Then the reading shows 3 snapshots, a total size of 3000 bytes and a stored size of 1200 bytes
    And the history keeps the reading of "demo", taken now

  Scenario: A repository that can't be reached keeps no reading
    Given the repository "demo" can't be reached
    When the person reads the stats of "demo"
    Then the read fails because the repository can't be reached
    And the history keeps no reading of "demo"

  Scenario: A check reads the stats before its end is recorded
    Given the repository "demo" holds 3 snapshots of 1000 bytes each, stored in 1200 bytes of 2400 raw
    When a check of "demo" runs
    Then the check succeeded
    And the history keeps a reading of "demo" the check took while it still held the repository

  Scenario: A check whose stats can't be read still succeeds
    Given the repository "demo" holds 3 snapshots of 1000 bytes each, stored in 1200 bytes of 2400 raw
    And the stats of "demo" can't be read
    When a check of "demo" runs
    Then the check succeeded
    And the history keeps no reading of "demo"

  Scenario: A check that fails reads no stats
    Given the repository "demo" holds 3 snapshots of 1000 bytes each, stored in 1200 bytes of 2400 raw
    And the check of "demo" fails
    When a check of "demo" runs
    Then the check failed
    And the history keeps no reading of "demo"

  Scenario: Readings are kept for a year
    Given the history keeps a reading of "demo" taken 366 days ago and one taken 364 days ago
    And the repository "demo" holds 3 snapshots of 1000 bytes each, stored in 1200 bytes of 2400 raw
    When the person reads the stats of "demo"
    Then the history of "demo" holds 2 readings, the one taken 364 days ago first

  Scenario: A cancel while a check reads the stats ends the check with its result
    Given the repository "demo" holds 3 snapshots of 1000 bytes each, stored in 1200 bytes of 2400 raw
    And the stats of "demo" are never answered
    When a check of "demo" is cancelled while it reads the stats
    Then the check succeeded
    And the history keeps no reading of "demo"

  Scenario: A reading of another repository once saved under the name is not shown
    Given the history keeps a reading of "demo" taken of another repository
    And the configuration records the ID of the repository "demo"
    When the person looks at the stats of "demo"
    Then no reading is shown
