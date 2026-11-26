@ui
Feature: A page's first load
  While a page waits for the daemon's first answer, it says the wait in words
  on a line of their own just before the region that answer fills, outside it,
  since a screen reader may leave a busy region unread until it stops being
  busy. The region is busy and shows the still shape of what is coming: a
  table's header and a few rows, or a form's fields and its button (§4.5).
  Once the answer comes, the content takes the region's place, and nothing on
  the page is busy or says the wait any more.

  The words and the still shape show only once the wait has lasted a fifth of
  a second, so an answer that comes sooner never flashes them, and once shown
  they stay at least 0.3 seconds, long enough to be seen rather than flicker.
  The region is busy from the start.

  Scenario: The start page says it waits for its plans just before a busy region in the shape of a table, and the table takes its place
    Given a demo plan "demo-home" with a scratch repository
    And the daemon holds back its answer to the start page's reading of the plans list
    When I open the web UI
    Then a region of the page is busy and shows the still shape of a table, its header and 3 rows of 2 columns
    And the page says "Loading…" just before its busy region, outside it
    And the page has no accessibility violations
    When the daemon gives the answer it held back to the start page's reading of the plans list
    Then no region of the page is busy
    And the table "Plans" has the row "demo-home"
    And the page doesn't show "Loading…"

  Scenario: Signing in says it waits for how long signing in lasts just before a busy region in the shape of a form, and the form takes its place
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    And the daemon holds back its answer to the Settings page's reading of how long signing in lasts
    When I open the address "/settings#signing-in"
    Then a region of the page is busy and shows the still shape of a form, 1 field and its button
    And the page says "Loading…" just before its busy region, outside it
    And the page has no accessibility violations
    When the daemon gives the answer it held back to the Settings page's reading of how long signing in lasts
    Then no region of the page is busy
    And the radio "30 days" of the group "Stay signed in for" is checked
    And the page doesn't show "Loading…"

  Scenario: The AI agents tab says it waits for what AI agents may do just before a busy region in the shape of a form, and the form takes its place
    Given a daemon with sign-in off
    And the daemon holds back its answer to the Settings page's reading of what AI agents may do without a token
    When I open the address "/settings#ai-agents"
    Then a region of the page is busy and shows the still shape of a form, 7 fields and its button
    And the page says "Loading…" just before its busy region, outside it
    And the page has no accessibility violations
    When the daemon gives the answer it held back to the Settings page's reading of what AI agents may do without a token
    Then no region of the page is busy
    And the group "Backups" has "Read" chosen
    And the page doesn't show "Loading…"

  Scenario: The Snapshots page whose reading of the repositories reaches no daemon stops waiting for them
    Given a demo plan "demo-home" with a scratch repository
    And the Snapshots page's reading of the repositories reaches no daemon
    When I open the address "/snapshots"
    Then the page shows the heading "Cannot reach Ladderback"
    And no region of the page is busy
    And the page doesn't show "Loading…"

  # Once its first answer came, a page's wait is over for good: the daemon
  # going away later says so in the heading, and leaves what the answer drew,
  # and the reader's place in it, as they were.
  Scenario: Picking a repository once the daemon has stopped keeps focus on the repository's button
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And the daemon has stopped
    And I press Enter
    Then the page shows the heading "Cannot reach Ladderback"
    And focus is on the button "Snapshots of demo"

  Scenario: A page whose answer comes within a fifth of a second never shows its wait
    Given a demo plan "demo-home" with a scratch repository
    And the page's clock stands still from the start until a step moves it on
    And the daemon holds back its answer to the start page's reading of the plans list
    When I open the web UI
    Then a region of the page is busy but shows nothing yet, neither the still shape of what is coming nor "Loading…"
    When the page's clock moves on 199 milliseconds
    Then a region of the page is busy but shows nothing yet, neither the still shape of what is coming nor "Loading…"
    When the daemon gives the answer it held back to the start page's reading of the plans list
    Then no region of the page is busy
    And the table "Plans" has the row "demo-home"
    And the page doesn't show "Loading…"

  Scenario: A page whose answer is slow shows its wait after a fifth of a second and keeps it at least 0.3 seconds
    Given a demo plan "demo-home" with a scratch repository
    And the page's clock stands still from the start until a step moves it on
    And the daemon holds back its answer to the start page's reading of the plans list
    When I open the web UI
    And the page's clock moves on 200 milliseconds
    Then a region of the page is busy and shows the still shape of a table, its header and 3 rows of 2 columns
    And the page says "Loading…" just before its busy region, outside it
    When the page's clock moves on 100 milliseconds
    And the daemon gives the answer it held back to the start page's reading of the plans list, and the page has it
    Then the page keeps the still shape of what is coming and says "Loading…" while its clock stands still
    When the page's clock moves on 199 milliseconds
    Then the page keeps the still shape of what is coming and says "Loading…" while its clock stands still
    When the page's clock moves on 1 millisecond
    Then no region of the page is busy
    And the table "Plans" has the row "demo-home"
    And the page doesn't show "Loading…"
