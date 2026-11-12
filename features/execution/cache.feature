@e2e
Feature: A repository's cache
  Each worker of a repository whose ID is known keeps rustic's cache in a
  cache slot of its own, a folder under the agent's cache folder named by the
  repository ID and the slot's number. The next operation on that repository
  reads the index and the snapshots from the cache its slot holds instead of
  from the repository again. No two running workers share a slot.

  Background:
    Given Ladderback is serving on loopback with the plan "home"
    And the configuration records the repository's ID

  Scenario: A backup keeps its cache slot to itself while it runs
    Given the plan's source holds many more files
    When the daemon starts a backup of the plan "home"
    Then the repository's cache slot 0 is held while the backup runs
    And the repository's cache slot 0 is free once the backup has ended

  Scenario: The next backup of the repository reads the cache the last one left
    When the daemon has finished 1 backup of the plan "home"
    Then the repository's cache slot 0 holds rustic's cache of the index and the snapshot
    And the repository's cache slot 0 is free once the backup has ended
    When the daemon has finished 1 backup of the plan "home"
    Then the second backup kept what the cache slot held and added its snapshot to it
