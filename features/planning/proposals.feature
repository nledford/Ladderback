Feature: Proposing a backup setup for a person to confirm
  An AI agent, or any other API client, helps a person set up their backups
  by proposing configuration changes, with its rationale, and nothing
  changes until a person approves the whole proposal (decision 89). A
  proposal is checked when it is made by the configuration's own rules, and
  nothing else: no secret is resolved, no command runs and no location is
  contacted. It waits as pending for 48 hours, then expires; the AI agent
  can withdraw it before that, and a proposal whose change no longer applies
  to the configuration goes stale, so the AI agent proposes again. Ended
  proposals stay listed as history.

  Background:
    Given the agent has its repository and no plans

  Scenario: A proposal changes nothing until a person approves it
    When an AI agent proposes adding the plan "home"
    Then the proposal is pending, to expire 48 hours after it was made
    And the configuration has no plan "home"

  Scenario: A change the configuration refuses is refused when it is proposed
    When an AI agent proposes changing the plan "home"
    Then the proposal is refused because its change 1 names a plan the configuration doesn't have
    And no proposal is kept

  Scenario: A proposal of more changes than a proposal holds is refused
    When an AI agent proposes adding 51 plans
    Then the proposal is refused as holding more than 50 changes
    And no proposal is kept

  Scenario: A proposal can't give a new plan a hook
    When an AI agent proposes adding the plan "home" with a hook that runs before each backup
    Then the proposal is refused because its change 1 changes a plan's hooks
    And no proposal is kept

  Scenario: A proposal can't take a plan's hooks away either
    Given the plan "home" has a hook that runs before each backup
    When an AI agent proposes changing the plan "home" to have no hooks
    Then the proposal is refused because its change 1 changes a plan's hooks
    And no proposal is kept

  Scenario: A proposal whose change no longer applies goes stale
    Given an AI agent has proposed adding the plan "home"
    When a person creates the plan "home" scheduled "0 3 * * *"
    Then the proposal is stale

  Scenario: Saving a plan brings each pending proposal up to date at once
    Given an AI agent has proposed adding the plan "home"
    When a person creates the plan "home" scheduled "0 3 * * *"
    Then the proposal is stale and announced as stale, before anyone reads it

  Scenario: A change elsewhere in the configuration leaves a proposal pending
    Given an AI agent has proposed adding the plan "home"
    When a person creates the plan "photos" scheduled "0 3 * * *"
    Then the proposal is pending

  Scenario: A proposal nobody approved expires 48 hours after it was made
    Given an AI agent has proposed adding the plan "home"
    When 48 hours pass
    Then the proposal has expired
    And the AI agent can't withdraw it, because it has expired

  Scenario: A withdrawn proposal stays listed
    Given an AI agent has proposed adding the plan "home"
    When the AI agent withdraws the proposal
    Then the proposal is withdrawn
    And the AI agent's proposals list it as withdrawn

  Scenario: An AI agent has at most 20 proposals pending
    Given an AI agent has 20 proposals pending
    When the AI agent proposes adding the plan "home"
    Then the proposal is refused as one over the 20 pending
    And another AI agent's proposal of the plan "home" is kept

  Scenario: An AI agent whose pending proposals have expired can propose again
    Given an AI agent has 20 proposals pending
    When 48 hours pass
    And the AI agent proposes adding the plan "home"
    Then the proposal is pending

  Scenario: An AI agent sees only its own proposals
    Given an AI agent has proposed adding the plan "home"
    Then another AI agent's proposals list none
    And another AI agent can't withdraw it, as it can't see it
