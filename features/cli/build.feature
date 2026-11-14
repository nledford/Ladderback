@e2e
Feature: The build that is running
  A build is named by its version alone only when it is its version's
  release: a build from another commit adds the commit it came from, and
  ".dirty" when tracked files had changes (decision 90). Machines running
  builds from between two releases can then be told apart. The program
  names its build wherever it says its version, and the daemon tells a page
  that may see the pages each part of it on its own, so nothing has to read
  them out of the name.

  Scenario: The program's version names its build
    When the owner runs "--version"
    Then the command exits with status 0
    And standard output names this checkout's build

  Scenario: The daemon names its build as it starts
    When the daemon is started with "--listen 127.0.0.1:0"
    Then the daemon's log names this checkout's build as it starts

  Scenario: With sign-in off, the daemon tells the page its build
    Given Ladderback is serving on loopback, configured for the host name "localhost"
    And a "GET" request for "/api/v1/session"
    When the request is sent
    Then the answer names this checkout's build: its version, its commit and whether it had changes

  Scenario: The daemon tells its build only once the page has signed in
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And a "GET" request for "/api/v1/session"
    When the request is sent
    Then the answer doesn't name the build
    When the admin signs in over HTTP with "demo-admin-pass"
    Then the answer names this checkout's build: its version, its commit and whether it had changes
