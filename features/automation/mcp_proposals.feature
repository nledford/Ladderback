@e2e
Feature: AI agents propose configuration changes through MCP
  An AI agent connected to a running Ladderback through MCP proposes
  configuration changes, reads a proposal it made and withdraws it, through
  the daemon's proposal routes and their checks, as any API client does
  (decision 89, §4.6). A proposal changes nothing until a person approves it
  on the "Pending changes" page, and no tool approves one. With sign-in on,
  the AI agent's API token needs the permission Configuration, and it sees and
  withdraws only the proposals it made. A proposal names a repository, never
  its secrets, and nothing the AI agent reads holds one.

  Background:
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the daemon's config file holds only the repository "demo"
    And the admin has signed in over HTTP with "demo-admin-pass"

  Scenario: An AI agent's proposal changes nothing until a person approves it
    Given the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
    And an MCP server connected to it with that API token in a file
    When the MCP client proposes adding the plan "nightly" over "/home/demo"
    Then the result is a proposal "pending", proposed by "assistant"
    And the daemon's config file still holds only the repository "demo"
    When the MCP client reads that proposal
    Then the result is a proposal "pending", proposed by "assistant"
    And the daemon's config file still holds only the repository "demo"
    When the admin approves the proposal over HTTP, confirming it with "demo-admin-pass"
    Then the daemon's config file has the plan "nightly"

  Scenario: A token without Configuration can't propose through MCP, and the refusal names the permission
    Given the admin has created an API token over HTTP with the permissions "backups: read" and the label "reader"
    And an MCP server connected to it with that API token in a file
    When the MCP client proposes adding the plan "nightly" over "/home/demo"
    Then the result is the error "api-token-permission"
    And the error names the permission "configuration"
    And the daemon's config file still holds only the repository "demo"

  # Approval needs a person's browser session with step-up (decision 89,
  # point 7): the server lists no tool that approves, and has none to call.
  Scenario: No tool approves a proposal
    Given the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
    And an MCP server connected to it with that API token in a file
    And the MCP client has proposed adding the plan "nightly" over "/home/demo"
    When the MCP client lists the tools
    Then the tools listed are "status, list_plans, list_repositories, list_snapshots, list_snapshot_folder, find_in_snapshots, list_operations, get_operation, read_log, run_plan, wait_operation, cancel_operation, test_repository, preview_forget, preview_import, export_profile, propose_setup, get_proposal, withdraw_proposal, dry_run_plan"
    When the MCP client calls the tool "approve_proposal" with that proposal
    Then the server answers that it has no such tool
    And the daemon's config file still holds only the repository "demo"

  # I4: the repository a proposal names holds its password and a credential
  # in its address in the config file; the AI agent reads neither.
  Scenario: An AI agent proposing into a repository never sees its secrets
    Given the daemon's config file holds only the repository "demo", its password "canary-inline-password" and its address's password "canary-address-password" written in it
    And the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
    And an MCP server connected to it with that API token in a file
    When the MCP client proposes adding the plan "nightly" over "/home/demo"
    Then the result is a proposal "pending", proposed by "assistant"
    When the MCP client reads that proposal
    And the MCP client withdraws that proposal
    Then the result is a proposal "withdrawn", proposed by "assistant"
    And nothing the MCP server or Ladderback wrote or logged shows "canary-inline-password" or "canary-address-password"

  Scenario: An AI agent withdraws only its own proposal, and only while it is pending
    Given the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "other"
    And the API token has proposed adding the plan "weekly" over "/home/demo"
    And the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
    And an MCP server connected to it with that API token in a file
    When the MCP client withdraws that proposal
    Then the result is the error "proposal-unknown"
    When the MCP client reads that proposal
    Then the result is the error "proposal-unknown"
    And the admin's session reads that proposal as "pending"
    When the MCP client proposes adding the plan "nightly" over "/home/demo"
    And the MCP client withdraws that proposal
    Then the result is a proposal "withdrawn", proposed by "assistant"
    When the MCP client withdraws that proposal
    Then the result is the error "proposal-not-pending"
