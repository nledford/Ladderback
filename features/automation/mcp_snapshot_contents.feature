@e2e
Feature: AI agents look inside snapshots through MCP
  An agent helping a person get a file back first has to find it:
  `list_snapshot_folder` lists one folder of a snapshot a page at a time, and
  `find_in_snapshots` runs a snapshot search, looking a name up across a
  repository's newest snapshots (§4.6). Both only read, so the `read` scope
  is enough, and neither costs an unbounded walk: a search reads at most 100
  snapshots for at most 15 seconds and returns at most 500 matches, and says
  which cap cut it short. Restoring and downloading stay the person's, in
  the web UI.

  Scenario: A file is found in the snapshots that hold it and its folder listed
    Given Ladderback is serving on loopback with the plan "home"
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
    And an MCP server connected to it
    When the daemon has finished 2 backups of the plan "home"
    And the MCP client looks for "file-*.txt" in the repository "demo", in 1 snapshot
    Then the search read 1 snapshot, left 1 out and was cut short by its "snapshots" cap
    When the MCP client looks for "file-*.txt" in the repository "demo", returning 2 matches
    Then the search found 2 matches and was cut short by its "matches" cap

  Scenario: A read token is enough to look inside snapshots
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the permissions "backups: read, snapshot-contents: read, daemon-log: read" and the label "agent"
    And an MCP server connected to it with that API token in a file
    When the MCP client looks for "*.txt" in the repository "demo"
    Then the result is the error "repository-unknown"

  # I4 where a worker opens the repository (§0): the password its file holds
  # opens it, a wrong one fails, and neither is ever shown or logged.
  Scenario: Looking inside snapshots never shows or logs the repository's password
    Given Ladderback is serving on loopback with the plan "home"
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
