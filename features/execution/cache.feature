@e2e
Feature: A repository's cache
  Each worker of a repository whose ID is known keeps rustic's cache in a
  cache slot of its own, a folder under the agent's cache folder named by the
  repository ID and the slot's number. The next operation on that repository
  reads the index and the snapshots from the cache its slot holds instead of
  from the repository again. No two running workers share a slot.

  Nothing in the cache is needed, so each time it starts the agent tidies its
  cache folder: it removes the cache of a repository no entry records, the
  cache slots above the highest one in use, and the unfinished files a worker
  that died left in a slot it keeps, once they are a day old. While an entry
  records no repository ID, it can't tell which repository's cache is that
  entry's, so it keeps every repository's cache until the entry's next job
  records the ID. It says in its log what it removed and its size. It never
  touches a slot a running worker holds, anything in the cache folder not
  named as its own, or anything a link points to.

  Background:
    Given Ladderback is serving on loopback with the plan "home"
    And the configuration records the repository's ID

  # A file in the plan's source that the backup waits on holds the worker
  # running, its slot locked, until the file is let go.
  Scenario: A backup keeps its cache slot to itself while it runs
    Given the plan's source holds a file the backup waits on until it is let go
    When the daemon starts a backup of the plan "home"
    Then the repository's cache slot 0 is held while the backup runs
    When the file the backup waits on is let go
    Then the repository's cache slot 0 is free once the backup has ended

  Scenario: The next backup of the repository reads the cache the last one left
    When the daemon has finished 1 backup of the plan "home"
    Then the repository's cache slot 0 holds rustic's cache of the index and the snapshot
    And the repository's cache slot 0 is free once the backup has ended
    When the daemon has finished 1 backup of the plan "home"
    Then the second backup kept what the cache slot held and added its snapshot to it

  Scenario: Starting again removes the cache of a repository no entry records
    Given the repository's cache slot 0 holds a cache
    And the cache folder holds the cache of a repository no entry records
    And the cache folder holds a folder of the user's own
    When the daemon is told to stop
    And Ladderback is started again on the same folders
    Then the cache of the repository no entry records is gone
    And the daemon's log says it removed that cache, with its size
    And the cache folder still holds the user's own folder
    And the repository's cache slot 0 still holds its cache

  Scenario: Starting again keeps every repository's cache while an entry records no ID
    Given the repository's cache slot 0 holds a cache
    And the repository's cache slot 1 holds a cache
    And the cache folder holds the cache of a repository no entry records
    And the configuration no longer records the repository's ID
    When the daemon is told to stop
    And Ladderback is started again on the same folders
    Then the repository no entry records keeps its cache slot 0 and loses its cache slot 1
    And the repository's cache slot 0 still holds its cache
    And the repository's cache slot 1 is gone

  Scenario: Starting again removes the spare cache slots and the unfinished files a worker left
    Given the repository's cache slot 0 holds a cache
    And the repository's cache slot 1 holds a cache
    And the repository's cache slot 2 holds a cache
    And the repository's cache slot 3 holds a cache
    And a worker of another daemon holds the repository's cache slot 2
    And the repository's cache slot 0 holds an unfinished file written 48 hours ago
    And the repository's cache slot 0 holds an unfinished file written 1 hours ago
    And the repository's cache slot 2 holds an unfinished file written 48 hours ago
    When the daemon is told to stop
    And Ladderback is started again on the same folders
    Then the repository's cache slot 3 is gone
    And the repository's cache slot 0 still holds its cache
    And the repository's cache slot 1 still holds its cache
    And the repository's cache slot 2 still holds its cache
    And the unfinished file written 48 hours ago in the repository's cache slot 0 is gone
    And the unfinished file written 1 hours ago in the repository's cache slot 0 is kept
    And the unfinished file written 48 hours ago in the repository's cache slot 2 is kept
    And the daemon's log says it removed the spare cache slot and the unfinished file, with their sizes

  Scenario: Tidying the cache never follows a link out of the cache folder
    Given a folder outside the cache folder holds files
    And the cache folder holds a link to that folder, named as a repository no entry records
    And the repository's cache slot 0 holds a cache
    And the repository's cache slot 5 is a link to that folder
    And the repository's cache slot 6 holds a cache and a link to that folder
    When the daemon is told to stop
    And Ladderback is started again on the same folders
    Then the repository's cache slot 6 is gone
    And the folder outside the cache folder still holds its files
