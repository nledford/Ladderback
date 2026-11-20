@e2e
Feature: Proposals made over HTTP
  An AI agent proposes configuration changes through the daemon's proposal
  routes, with an API token that has the permission Configuration while
  sign-in is on (security §10.5, decision 89). The token makes, lists, reads
  and withdraws its own proposals and nobody else's; a proposal changes
  nothing in `config.toml`, and a token without the permission is refused,
  naming it.

  Background:
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the daemon's config file holds only the repository "demo"
    And the admin has signed in over HTTP with "demo-admin-pass"

  Scenario: A proposal made with a token changes nothing in the config file
    Given the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
    When the API token proposes adding the plan "nightly" over "/home/demo"
    Then the proposal is answered as pending, proposed by "assistant", to expire 48 hours after it was made
    And the daemon's config file still holds only the repository "demo"

  Scenario: A token without Configuration can't propose, and the refusal names the permission
    Given the admin has created an API token over HTTP with the permissions "backups: read" and the label "reader"
    And a "POST" request for "/api/v1/proposals" with the API token
    And the request has the header "Content-Type: application/json"
    And the request has the body "{\"changes\":[]}"
    When the request is sent
    Then the route answers with status 403 and the error code "api-token-permission"
    And the refusal names the permission "configuration"

  Scenario: A withdrawn proposal stays in the token's list
    Given the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
    And the API token has proposed adding the plan "nightly" over "/home/demo"
    When the API token withdraws its proposal
    Then the API token's proposals list it as withdrawn

  Scenario: A change the configuration refuses is refused at the change, with no value in the refusal
    Given the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
    When the API token proposes adding the plan "nightly" into the repository "elsewhere"
    Then the route answers with status 422 and the error code "proposal-invalid"
    And the refusal names change 1 and its setting "repository"

  Scenario: A token can't approve a proposal, even with the admin password, and the admin's session can
    Given the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
    And the API token has proposed adding the plan "nightly" over "/home/demo"
    When the API token approves its proposal, confirming it with "demo-admin-pass"
    Then the route answers with status 403 and the error code "api-token-permission"
    And the refusal names the permission "session"
    And the daemon's config file still holds only the repository "demo"
    When the admin approves the proposal over HTTP, confirming it with "demo-admin-pass"
    Then the daemon's config file has the plan "nightly"

  Scenario: An approved proposal keeps what AI agents may do without a token
    Given the daemon's config file also holds:
      """
      [without-token]
      backup-runs = "none"
      """
    And the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
    And the API token has proposed adding the plan "nightly" over "/home/demo"
    When the admin approves the proposal over HTTP, confirming it with "demo-admin-pass"
    Then the daemon's config file has the plan "nightly"
    And the daemon's config file still holds:
      """
      [without-token]
      backup-runs = "none"
      """
