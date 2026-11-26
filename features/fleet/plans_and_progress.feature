Feature: A machine's plans and its running operation's progress reach its hub
  An approved machine tells its hub its plans, each with its last run and its
  next planned run, whenever they change, and the progress of an operation it
  runs, at most every 5 seconds. The hub keeps the latest of each, and shows
  a progress only while the machine's reports say its operation runs
  (technical design §7.2).

  Background:
    Given an agent paired with a hub
    And a person approved the agent on the hub
    And the agent has the plans "photos" and "documents"

  Scenario: The hub shows each plan with its last run and its next
    Given the plan "photos" last backed up successfully and runs next in an hour
    When the agent's sync with its hub runs
    Then the hub shows the agent's plans "photos" and "documents", in that order
    And the hub shows the plan "photos" last succeeded and runs next in an hour
    And the hub shows the plan "documents" never ran and has no next run

  Scenario: Plans that didn't change aren't sent again, and a change is
    Given the agent's sync with its hub ran
    When the agent's sync with its hub runs
    Then the hub was sent the agent's plans once
    When the plan "documents" last backed up successfully and runs next in an hour
    And the agent's sync with its hub runs
    Then the hub was sent the agent's plans twice
    And the hub shows the plan "documents" last succeeded and runs next in an hour

  Scenario: The hub shows a running backup's progress, and none once it ends
    Given the agent started a backup
    And the backup has backed up 40 of 100 bytes
    When the agent's sync with its hub runs
    Then the hub shows the agent's backup at 40 of 100 bytes
    When the backup ends
    And the agent's sync with its hub runs
    Then the hub shows no progress of the agent

  Scenario: A canceled backup's progress isn't shown
    Given the agent started a backup
    And the backup has backed up 40 of 100 bytes
    And the agent's sync with its hub ran
    When the backup is canceled
    And the agent's sync with its hub runs
    Then the hub shows no progress of the agent
    And the hub shows the agent's backup was canceled

  Scenario: The progress is sent at most every 5 seconds
    Given the agent started a backup
    And the backup has backed up 10 of 100 bytes
    And the agent's sync with its hub ran
    When the agent's clock moves on 2 seconds and the backup has backed up 20 of 100 bytes
    And the agent's sync with its hub runs
    Then the hub shows the agent's backup at 10 of 100 bytes
    When the agent's clock moves on 3 seconds
    And the agent's sync with its hub runs
    Then the hub shows the agent's backup at 20 of 100 bytes
