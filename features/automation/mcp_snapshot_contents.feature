@e2e
Feature: AI agents look inside snapshots through MCP
  An agent helping a person get a file back first has to find it:
  `list_snapshot_folder` lists one folder of a snapshot a page at a time, and
  `find_in_snapshots` runs a snapshot search, looking a name up across a
  repository's newest snapshots (§4.6). Both show the names inside
  snapshots, so a token needs the permission Snapshot contents for each, as
  for a folder of the snapshot page, and neither costs an unbounded walk: a
  search reads at most 100 snapshots for at most 15 seconds and returns at
  most 500 matches, and says which cap cut it short. Restoring and
  downloading stay the person's, in the web UI. With sign-in off, an MCP
  server is held to what AI agents may do without a token, which leaves
  Snapshot contents out until a person opens it.

  Scenario: A file is found in the snapshots that hold it and its folder listed
    Given Ladderback is serving on loopback with the plan "home"
    And AI agents without a token may look inside snapshots
    And an MCP server connected to it
    When the daemon has finished 1 backup of the plan "home"
    And the plan's source holds the file "Tax-2026.pdf" reading "return"
    And the daemon has finished 1 backup of the plan "home"
    And the MCP client looks for "tax-*.PDF" in the repository "demo"
    Then the search read 2 snapshots and found 1 match
    And that match is in the newest snapshot only
    When the MCP client lists the folder that holds that match in the newest snapshot
    Then the folder lists "Tax-2026.pdf" as a file of 6 bytes

  Scenario: A search a cap cut short says which cap it was
    Given Ladderback is serving on loopback with the plan "home"
    And AI agents without a token may look inside snapshots
    And an MCP server connected to it
    When the daemon has finished 2 backups of the plan "home"
    And the MCP client looks for "file-*.txt" in the repository "demo", in 1 snapshot
    Then the search read 1 snapshot, left 1 out and was cut short by its "snapshots" cap
    When the MCP client looks for "file-*.txt" in the repository "demo", returning 2 matches
    Then the search found 2 matches and was cut short by its "matches" cap

  Scenario: A token with Snapshot contents may look inside snapshots
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the permissions "snapshot-contents: read" and the label "agent"
    And an MCP server connected to it with that API token in a file
    When the MCP client looks for "*.txt" in the repository "demo"
    Then the result is the error "repository-unknown"
    When the MCP client lists the top folder of the snapshot "0123abcd0123abcd0123abcd0123abcd0123abcd0123abcd0123abcd0123abcd" in the repository "demo"
    Then the result is the error "repository-unknown"

  Scenario: A token without Snapshot contents can't look inside snapshots
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the permissions "backups: read, backup-runs: start-and-cancel" and the label "agent"
    And an MCP server connected to it with that API token in a file
    When the MCP client looks for "*.txt" in the repository "demo"
    Then the result is the error "api-token-permission"
    And the error names the permission "snapshot-contents"
    When the MCP client lists the top folder of the snapshot "0123abcd0123abcd0123abcd0123abcd0123abcd0123abcd0123abcd0123abcd" in the repository "demo"
    Then the result is the error "api-token-permission"
    And the error names the permission "snapshot-contents"

  # I4 where a worker opens the repository (§0): the password its file holds
  # opens it, a wrong one fails, and neither is ever shown or logged.
  Scenario: Looking inside snapshots never shows or logs the repository's password
    Given Ladderback is serving on loopback with the plan "home"
    And AI agents without a token may look inside snapshots
    And the plan's repository reads its password from a file
    And an MCP server connected to it
    And the MCP client has run the plan "home" to the end
    When the MCP client looks for "file-1.txt" in the repository "demo"
    Then the search read 1 snapshot and found 1 match
    When the repository's password file holds "canary-not-the-password", which isn't the repository's
    And the MCP client looks for "file-1.txt" in the repository "demo"
    Then the result is the error "password-not-accepted"
    When the MCP client lists the top folder of the snapshot it searched
    Then the result is the error "password-not-accepted"
    And nothing the MCP server or Ladderback wrote or logged shows "e2e-scratch-password" or "canary-not-the-password"
