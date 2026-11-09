@e2e
Feature: Hot/cold repositories
  A repository can keep the files that are read all the time, its snapshots
  and indexes, in a hot half, and the rest in a cold half (rustic's
  `repo-hot`). A job opens both halves, so what rustic writes to the hot half
  gets there.

  Background:
    Given a new hot/cold repository
    And a plan "home" backing up a folder with 3 files

  Scenario: A backup writes its snapshot to the hot half as well as the cold half
    When the agent backs up the plan
    Then the operation ends as succeeded
    And the snapshot is in the hot half of the repository
    And the snapshot is in the cold half of the repository
    And the repository has 1 snapshot
