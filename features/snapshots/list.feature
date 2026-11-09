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
