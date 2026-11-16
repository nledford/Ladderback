Feature: Approving or rejecting a proposal
  A person approves or rejects an AI agent's proposal as a whole, on the
  "Pending changes" page (decision 89). Approval is bound to what the page
  showed: the configuration the diff was rebuilt against and the diff
  itself. It is confirmed by step-up, and applied in one step under the
  configuration's write lock, so a hand edit is never overwritten: the
  proposal returns to pending for a new approval instead. A rejection gives
  the AI agent a reason for each change, and the AI agent waits a while
  before proposing again. Ladderback, not the AI agent, says what the
  changes do.

  Background:
    Given the agent has its repository and no plans

  Scenario: An approved proposal changes the configuration, and its history names the proposal
    Given an AI agent has proposed adding the plan "home"
    And a person has opened the proposal
    When the person approves the proposal as it was shown
    Then the proposal is applied
    And the configuration has the plan "home"
    And the configuration's history names the proposal as the reason for the save

  Scenario: An approval not confirmed by step-up changes nothing
    Given an AI agent has proposed adding the plan "home"
    And a person has opened the proposal
    When the person approves the proposal confirming it with another proposal's ID
    Then the approval is refused as not confirmed
    And the proposal is pending
    And the configuration has no plan "home"

  Scenario: An approval of a configuration that changed since the page showed it changes nothing
    Given an AI agent has proposed adding the plan "home"
    And a person has opened the proposal
    When a comment is edited in the configuration file
    And the person approves the proposal as it was shown
    Then the approval is refused because the configuration changed since it was shown
    And the proposal is pending
    And the configuration has no plan "home"

  Scenario: A hand edit made during the apply is kept, and the proposal waits for a new approval
    Given an AI agent has proposed adding the plan "home"
    And a person has opened the proposal
    And the configuration file will be edited by hand just before the apply writes it
    When the person approves the proposal as it was shown
    Then the approval is refused because the configuration changed since it was shown
    And the hand edit is kept
    And the proposal is pending
    And the configuration has no plan "home"

  Scenario: A comment edit leaves a proposal pending, and the page says the configuration changed
    Given an AI agent has proposed adding the plan "home"
    When a comment is edited in the configuration file
    And a person opens the proposal
    Then the proposal is pending
    And the page says the configuration has changed since the proposal was drafted

  Scenario: An expired proposal can't be approved
    Given an AI agent has proposed adding the plan "home"
    And a person has opened the proposal
    When 48 hours pass
    And the person approves the proposal as it was shown
    Then the approval is refused because the proposal has expired
    And the configuration has no plan "home"

  Scenario: A rejection gives the AI agent a reason for each change
    Given an AI agent has proposed adding the plans "home" and "photos"
    When a person rejects the proposal with the reasons "Back up weekly instead" and ""
    Then the proposal is rejected
    And the AI agent reads the reason "Back up weekly instead" for change 1 and none for change 2
    And the configuration has no plan "home"

  Scenario: An AI agent waits 5 minutes after a rejection before proposing again
    Given an AI agent has proposed adding the plan "home"
    And a person has rejected the proposal
    When the AI agent proposes adding the plan "photos"
    Then the proposal is refused until 5 minutes after the rejection
    And another AI agent's proposal of the plan "photos" is kept

  Scenario: An AI agent proposes again once 5 minutes have passed since the rejection
    Given an AI agent has proposed adding the plan "home"
    And a person has rejected the proposal
    When 5 minutes pass
    And the AI agent proposes adding the plan "photos"
    Then the proposal is pending

  Scenario: An AI agent makes at most 30 proposals an hour
    Given an AI agent has made and withdrawn 30 proposals in the last hour
    When the AI agent proposes adding the plan "home"
    Then the proposal is refused as one over the 30 an hour

  Scenario: The summary says what the changes do, whatever the rationale claims
    Given the plan "home" backs up "/home/demo"
    And an AI agent has proposed removing the plan "home", with the rationale "This changes nothing."
    When a person opens the proposal
    Then the summary says the proposal removes the plan "home"

  Scenario: A proposal that could delete snapshots can't be approved until the forget preview comes
    Given the plan "home" backs up "/home/demo"
    And an AI agent has proposed removing the plan "home", with the rationale "Tidy up."
    And a person has opened the proposal
    When the person approves the proposal as it was shown
    Then the approval is refused because the proposal could delete snapshots
    And the page says approving the proposal waits for the forget preview
    And the proposal is pending
    And the configuration has the plan "home"

  Scenario: A proposal that backs up the same sources as another plan is kept, with a warning
    Given the plan "home" backs up "/home/demo"
    When an AI agent proposes adding the plan "copy" backing up "/home/demo"
    Then the proposal is pending
    And the AI agent is warned that the plans "copy" and "home" back up the same sources

  Scenario: Each status change is announced
    Given an AI agent has proposed adding the plan "home"
    When the AI agent withdraws the proposal
    Then an event announces the proposal as withdrawn, with nothing of its rationale
