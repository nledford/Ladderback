@ui
Feature: The app's tables
  Every table of the app is the shared table (§4.5, #876). It sits in a frame
  of its own, a region named as the table is, which Tab stops on, so a
  keyboard scrolls it sideways where its columns are wider than the page, each
  row's header pinned at its start. On a small screen its words stay whole and
  the frame scrolls, rather than a word breaking letter by letter (#524). An
  instant in a cell is the cell's own words, in a time element that gives it
  to a program, and never in a title, which only a pointer resting on the cell
  shows (#851).

  Scenario: On a small screen the Operations table keeps its words whole in a frame of its own, each start time in its words
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    And a viewport of 400 by 800 pixels
    When I open the address "/operations"
    Then the table "Operations" has the row "home"
    And the table "Operations" sits in a frame of its own that Tab stops on, named as the table is
    And no word of a table breaks inside, an address only between its parts
    And the table "Operations" gives each instant of its "Started" column in its words, in a time element, with no title
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: On a small screen the Repositories table keeps its words whole in a frame of its own, where a password comes from too
    Given a demo plan "demo-home" with a scratch repository
    And a viewport of 400 by 800 pixels
    When I open the address "/repositories"
    Then the table "Repositories" has a row "demo" that shows "Kept in the configuration file"
    And the table "Repositories" sits in a frame of its own that Tab stops on, named as the table is
    And no word of a table breaks inside, an address only between its parts
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: On a small screen the Repositories table keeps the words of a repository's locations whole in a frame of its own
    Given a daemon whose repository "nas" has the locations "sftp:me@nas.home.lan:/srv/data/repo/pc/main" and "rest:https://backup.example.org/laptop" and its password in the file "/etc/ladderback/example.password"
    And a viewport of 400 by 800 pixels
    When I open the address "/repositories"
    Then the repository "nas" lists 2 locations
    And the table "Repositories" sits in a frame of its own that Tab stops on, named as the table is
    And no word of a table breaks inside, an address only between its parts
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: The plans tables of the start page and the Plans page sit in frames of their own, each next run in its words
    Given a demo plan "demo-home" on the schedule "0 3 * * *" and a demo plan "demo-photos" without one
    When I open the web UI
    Then the table "Plans" has the row "demo-home"
    And the table "Plans" sits in a frame of its own that Tab stops on, named as the table is
    And the table "Plans" gives each instant of its "Next run" column in its words, in a time element, with no title
    And the page has no accessibility violations
    When I open the address "/plans"
    Then the table "Plans" has the row "demo-home"
    And the table "Plans" sits in a frame of its own that Tab stops on, named as the table is
    And the table "Plans" gives each instant of its "Next run" column in its words, in a time element, with no title
    And the page has no accessibility violations

  Scenario: The API tokens table sits in a frame of its own, each instant in its words
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    And the admin has created the API token "nightly-mcp" with the scopes "read"
    When I open the address "/settings#api-tokens"
    Then the table "API tokens" has the row "nightly-mcp"
    And the table "API tokens" sits in a frame of its own that Tab stops on, named as the table is
    And the table "API tokens" gives each instant of its "Created" column in its words, in a time element, with no title
    And the page has no accessibility violations

  Scenario: The Log table sits in a frame of its own, each line's time in its words
    Given the recent log holds:
      | at                       | level | source     | message             |
      | 2026-11-15T09:00:00.000Z | info  | ladderback | Ladderback starting |
    When I open the address "/log"
    Then the table "Log" lists, from the top, the messages:
      | Ladderback starting |
    And the table "Log" sits in a frame of its own that Tab stops on, named as the table is
    And the table "Log" gives each instant of its "Time (UTC)" column in its words, in a time element, with no title
    And the page has no accessibility violations

  Scenario: The Snapshots page's tables of a history, of its stats and of a forget's review sit in frames of their own, each time in its words
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the table "Snapshots of plan home" has 3 rows
    And the table "Snapshots of plan home" sits in a frame of its own that Tab stops on, named as the table is
    And the table "Snapshots of plan home" gives each instant of its "Time" column in its words, in a time element, with no title
    When I press Tab until focus is on the button "Read the stats now"
    And I press Enter
    Then the polite region comes to contain "Stats read."
    And the table "Stats of demo" has a row "Snapshots" that shows "3"
    And the table "Stats of demo" sits in a frame of its own that Tab stops on, named as the table is
    And the page has no accessibility violations
    When I press Shift+Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the table "Snapshots forgotten from plan home" has 2 rows
    And the table "Snapshots forgotten from plan home" sits in a frame of its own that Tab stops on, named as the table is
    And the table "Snapshots forgotten from plan home" gives each instant of its "Time" column in its words, in a time element, with no title
    And the page has no accessibility violations

  Scenario: A proposal's forget preview lists its snapshots in a frame of its own, each time in its words
    Given a demo plan "demo-home" with a tiny scratch repository
    And the plan "demo-home" has been backed up 3 times
    And the plan "demo-home" keeps the last 1 snapshots
    And a proposal to remove the plan "demo-home"
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    And I press Tab until focus is on the button "Show what it would forget"
    And I press Enter
    Then the proposal says "Now, retention forgets 2 of the 3 snapshots of the plan demo-home."
    And the table "After the proposal, retention would no longer forget 2 snapshots of the plan demo-home" sits in a frame of its own that Tab stops on, named as the table is
    And the table "After the proposal, retention would no longer forget 2 snapshots of the plan demo-home" gives each instant of its "Time" column in its words, in a time element, with no title
    And the page has no accessibility violations
