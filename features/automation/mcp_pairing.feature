@e2e
Feature: AI agents explain pairing a machine with its hub through MCP
  A person asks their AI agent how to add a machine to the hub. MCP reads
  and explains; a person pairs and approves (components §4.6, security
  §10.5). The resource `ladderback://pairing` holds the steps of pairing:
  make a token on the hub, pair on the machine, approve on the hub, and for
  each the refusals a person most often meets, by code, in the catalog's
  words. The tool `pairing_state` says whether this machine is paired and
  with which hub, by the hub's address and its key's fingerprint, never a
  token or a secret (I4). It doesn't connect to the hub, so it can't tell
  whether the hub has approved the machine yet.

  Scenario: The guide walks through the three steps of pairing
    Given an MCP server running in a scratch folder
    When the MCP client reads the pairing guide
    Then the guide's steps are "make-token" on the hub, "pair" on the machine and "approve" on the hub
    And every refusal the guide names is a code Ladderback has, said in the catalog's words

  Scenario: The guide says what an expired token means and what to do
    Given an MCP server running in a scratch folder
    When the MCP client reads the pairing guide
    Then the step "pair" names the refusal "token-expired", saying "This token lapsed. Make a new one on the hub."

  Scenario: The guide says a paired machine waits for approval on the hub
    Given an MCP server running in a scratch folder
    When the MCP client reads the pairing guide
    Then the step "approve" says the hub holds the machine waiting for approval

  Scenario: A machine that never paired says so
    Given Ladderback is serving on loopback
    And an MCP server connected to it
    When the MCP client calls the tool "pairing_state"
    Then the pairing state says this machine isn't paired with a hub

  Scenario: A paired machine names its hub and nothing of the token
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And an MCP server running headless on the machine's folders
    When the MCP client calls the tool "pairing_state"
    Then the pairing state names the hub's address and its key's fingerprint, and no certificate, since the address isn't encrypted
    And nothing the MCP server wrote shows the pairing token

  Scenario: A token without Backups can't read the pairing state
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the permissions "daemon-log: read" and the label "agent"
    And an MCP server connected to it with that API token in a file
    When the MCP client calls the tool "pairing_state"
    Then the result is the error "api-token-permission"
    And the error names the permission "backups"
