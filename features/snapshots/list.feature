@snapshots
Feature: Listing a repository's snapshots
  The agent lists a repository's snapshots live, newest first: the list is a
  read, so it takes no maintenance slot and writes nothing to the operation
  log. A repository the engine can't open fails the listing with the same
  failure a test of it reports, and nothing is listed.

  @e2e
  Scenario: A repository with three snapshots lists all three, newest first
    Given a repository with three snapshots
    When the agent lists the repository's snapshots
    Then the listing shows the three snapshots newest first with id, time, host and label

  @e2e
  Scenario: An empty repository lists none
    Given a repository with no snapshots
    When the agent lists the repository's snapshots
    Then the listing is empty

  @e2e
  Scenario: A location with no repository fails the listing
    Given a location with no repository
    When the agent lists the repository's snapshots
    Then the listing fails

  @e2e
  Scenario: A wrong password fails the listing and lists nothing
    Given a repository with three snapshots
    And a wrong password is used for the repository
    When the agent lists the repository's snapshots
    Then the listing fails

  # A snapshot time is when the snapshot was taken, with the offset of the
  # clock that took it (§3.1): the listing carries that offset beside the
  # instant, so a page can show the day retention counts the snapshot on
  # (§16, decision 56). The examples are decision 56's: a container whose
  # clock runs in UTC, a late evening in Chicago, and a laptop flown from
  # Auckland to Hawaii.
  @e2e
  Scenario Outline: A listed snapshot carries the offset of the clock that took it
    Given a repository with a snapshot taken at "<taken>"
    When the agent lists the repository's snapshots
    Then the listing shows the snapshot at "<instant>" with the offset "<offset>"

    Examples:
      | case                         | taken                     | instant              | offset |
      | a container's clock in UTC   | 2026-11-11 05:30:00+00:00 | 2026-11-11T05:30:00Z | +00:00 |
      | late evening in Chicago      | 2026-11-10 23:30:00-06:00 | 2026-11-11T05:30:00Z | -06:00 |
      | a laptop that reached Hawaii | 2026-11-10 23:30:00-10:00 | 2026-11-11T09:30:00Z | -10:00 |
      | a laptop still in Auckland   | 2026-11-10 00:30:00+13:00 | 2026-11-09T11:30:00Z | +13:00 |
