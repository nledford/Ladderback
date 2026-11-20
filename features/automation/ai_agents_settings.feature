@ui
Feature: Choosing in Settings what AI agents may do without a token
  The AI agents tab of Settings holds what AI agents may do without a token
  (§10.1, decision 127): the same permissions as a token, each a group of its
  own, kept in the `[without-token]` table of `config.toml`. It limits
  `ladderback mcp` in a session of its own, and when it connects while
  sign-in is off. With sign-in off any other program may use the API all the
  same, so the tab says so and recommends turning sign-in on. With sign-in on
  a change takes the current admin password, as a token's does.

  Scenario: With sign-in off, the tab says what the permissions limit, and saves a change by keyboard alone
    Given a daemon with sign-in off
    When I open the address "/settings#ai-agents"
    Then the page shows a message containing "limit only ladderback mcp"
    And the group "Backups" has "Read" chosen
    And the group "Backup runs" has "Start and cancel" chosen
    And the group "Daemon log" has "No access" chosen
    And the page has no accessibility violations
    When I skip to the content
    And I press Tab until focus is in the group "Daemon log"
    And I press the down arrow until focus is on the choice "Read" of the group "Daemon log"
    And I press Tab until focus is on the button "Save what AI agents may do"
    And I press Enter
    Then the toast reads "AI agents without a token now have the permissions you chose." after its icon "Success"
    When I reload the page
    Then the group "Daemon log" has "Read" chosen

  Scenario: With sign-in on, a change takes the current admin password
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    When I open the address "/settings#ai-agents"
    Then the page shows a message containing "while sign-in is off"
    When I skip to the content
    And I press Tab until focus is in the group "Backup runs"
    And I press the down arrow until focus is on the choice "No access" of the group "Backup runs"
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Tab until focus is on the button "Save what AI agents may do"
    And I press Enter
    Then the toast reads "AI agents without a token now have the permissions you chose." after its icon "Success"
    And the page has no accessibility violations
    When I reload the page
    Then the group "Backup runs" has "No access" chosen
