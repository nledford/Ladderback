@e2e
Feature: AI agents read the recent log through MCP
  When a backup fails, an agent helping someone needs the daemon's log
  around it. `read_log` reads the recent log (§3.1) newest first, narrowed
  as the Log page narrows it, by level (that level or worse), and further by
  an operation or a plan the lines name and by when they were written. One
  call answers at most 200 lines and 32 KiB of text, a page at a time, and
  says which cap cut it short. The lines are the log's own, masked as the
  log masks them (I4), so a token needs the permission Daemon log, as for
  the Log page.

  Scenario: Lines at a level or worse are the lines the Log page shows for that level
    Given the daemon's config file holds:
      """
      schema = 1

      [repositories.nas]
      repository = "rest:https://demo:CANARY-location-password@backup.example.com/nas"
      password-env = "DEMO_PASSWORD"
      """
    And the daemon's config file lets AI agents without a token read the log
    And Ladderback is serving on loopback with that config file
    And an MCP server connected to it
    When the MCP client reads the log at the level "warn"
    Then every line read is at the level "warn" or worse
    And a line read says "repository nas: The address in the repository setting holds a password as text"
    And the lines read are those the Log page shows under "Warnings and errors"
    And nothing the MCP server wrote shows "CANARY-location-password"

  Scenario: The lines of one operation are those that name it
    Given Ladderback is serving on loopback with the plan "home"
    And AI agents without a token may read the daemon's log
    And an MCP server connected to it
    And the MCP client has run the plan "home" to the end
    When the MCP client reads the log of that operation
    Then every line read names that operation
    And a line read says that operation ended

  Scenario: The lines of one plan are those that name it
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's repository reads its password from a command that prints "e2e-scratch-password" and then fails saying "vault sealed"
    And AI agents without a token may read the daemon's log
    And an MCP server connected to it
    When the MCP client calls the tool "run_plan" with the arguments '{"plan": "home"}'
    And the MCP client reads the log of the plan "home"
    Then every line read names the plan "home"
    And a line read says the manual run of the plan "home" didn't start

  Scenario: Lines are read within a time range
    Given Ladderback is serving on loopback with the plan "home"
    And AI agents without a token may read the daemon's log
    And an MCP server connected to it
    And the MCP client has run the plan "home" to the end
    When the MCP client reads the log since the time of the line saying that operation ended
    Then every line read was written at that time or later
    And a line read says that operation ended
    And no line read says "starting"
    When the MCP client reads the log until the time of the line saying "starting"
    Then every line read was written at that time or earlier
    And a line read says "starting"
    And no line read says that operation ended

  Scenario: The log is read newest first, a page at a time, and a page cut short by its lines says so
    Given Ladderback is serving on loopback with the plan "home"
    And AI agents without a token may read the daemon's log
    And an MCP server connected to it
    And the MCP client has run the plan "home" to the end
    When the MCP client reads the log, 2 lines to a page
    Then 2 lines are read, newest first
    And the read was cut short by its "lines" cap
    When the MCP client reads the next page of the log
    Then 2 lines are read, newest first, each older than the page before's

  Scenario: A page cut short by its bytes says so
    Given the daemon's config file holds 150 repositories, each with a password in its address
    And the daemon's config file lets AI agents without a token read the log
    And Ladderback is serving on loopback with that config file
    And an MCP server connected to it
    When the MCP client reads the log at the level "warn", 200 lines to a page
    Then fewer than 150 lines are read, holding at most 32 KiB of text
    And the read was cut short by its "bytes" cap

  Scenario: A token with Daemon log reads the log
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the permissions "daemon-log: read" and the label "agent"
    And an MCP server connected to it with that API token in a file
    When the MCP client reads the log
    Then a line read says "starting"

  Scenario: A token without Daemon log can't read the log
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the permissions "backups: read, backup-runs: start-and-cancel" and the label "agent"
    And an MCP server connected to it with that API token in a file
    When the MCP client reads the log
    Then the result is the error "api-token-permission"
    And the error names the permission "daemon-log"

  Scenario: A session of its own reads its own log
    Given an MCP server running in a scratch folder
    And the MCP client has run the plan "scratch" to the end
    When the MCP client reads the log of that operation
    Then every line read names that operation
    And a line read says that operation ended
