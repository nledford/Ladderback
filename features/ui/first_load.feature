@ui
Feature: A page's first load
  While a page waits for the daemon's first answer, the region that answer
  fills is busy and says the wait in words, beside the still shape of what is
  coming: a table's header and a few rows, or a form's fields and its button
  (§4.5). Once the answer comes, the content takes the region's place, and
  nothing on the page is busy or says the wait any more.

  Scenario: The start page waits for its plans in the shape of a table, and the table takes its place
    Given a demo plan "demo-home" with a scratch repository
    And the daemon holds back its answer to the start page's reading of the plans list
    When I open the web UI
    Then a region of the page is busy and shows the words "Loading…" beside the still shape of a table, its header and 3 rows of 2 columns
    And the page has no accessibility violations
    When the daemon gives the answer it held back to the start page's reading of the plans list
    Then no region of the page is busy
    And the table "Plans" has the row "demo-home"
    And the page doesn't show "Loading…"

  Scenario: Signing in waits for how long signing in lasts in the shape of a form, and the form takes its place
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    And the daemon holds back its answer to the Settings page's reading of how long signing in lasts
    When I open the address "/settings#signing-in"
    Then a region of the page is busy and shows the words "Loading…" beside the still shape of a form, 1 field and its button
    And the page has no accessibility violations
    When the daemon gives the answer it held back to the Settings page's reading of how long signing in lasts
    Then no region of the page is busy
    And the radio "30 days" of the group "Stay signed in for" is checked
    And the page doesn't show "Loading…"

  Scenario: The Snapshots page whose reading of the repositories reaches no daemon stops waiting for them
    Given a demo plan "demo-home" with a scratch repository
    And the Snapshots page's reading of the repositories reaches no daemon
    When I open the address "/snapshots"
    Then the page shows the heading "Cannot reach Ladderback"
    And no region of the page is busy
    And the page doesn't show "Loading…"
