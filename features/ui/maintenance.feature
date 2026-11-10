@ui
Feature: Maintenance of a repository on the Snapshots page
  The Snapshots page shows a repository's history grouped by owner: a section
  for each plan of this agent, and apart from them the orphaned history no
  plan of this agent owns. A forget, an adoption, a prune and a check are
  each reviewed in a dialog before they run, and nothing changes until the
  review is confirmed. The journeys use the keyboard alone, against a daemon
  this run started with a demo plan and a scratch repository, so every
  backup, forget and prune they start is real. Unmanaged history and
  cancelling a running maintenance are not tried here.

  Scenario: The snapshots page groups a repository's history under its plan
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the section heading "Plan home"
    And the plan "home" shows 2 snapshot rows

  Scenario: A forget is reviewed before it runs and changes nothing until it is confirmed
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And focus is on the button "Cancel"
    And the dialog "Forget snapshots of demo" says "home"
    And the dialog "Forget snapshots of demo" offers the button "Forget 2 snapshots"
    When I press Escape
    Then the dialog "Forget snapshots of demo" is closed
    And focus is on the button "Forget snapshots"
    And the plan "home" shows 3 snapshot rows

  Scenario: A review of a forget has no accessibility violations
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the page has no accessibility violations

  Scenario: Confirming a forget removes what the review named and announces it
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    When I press Tab until focus is on the button "Forget 2 snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" is closed
    And the polite region comes to contain "Forget finished."
    And the plan "home" shows 1 snapshot rows

  Scenario: A forget with nothing to remove offers only to close the review
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 5 snapshots
    And the plan "home" has been backed up 2 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the dialog "Forget snapshots of demo" offers only the button "Close"
    When I press Enter
    Then the dialog "Forget snapshots of demo" is closed
    And the plan "home" shows 2 snapshot rows

  Scenario: A forget review names the history it leaves alone
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the plan "home" is now called "away"
    And the plan "away" has been backed up 3 times
    And the plan "away" keeps the last 1 snapshots
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the dialog "Forget snapshots of demo" says "orphaned"
    And the dialog "Forget snapshots of demo" offers the button "Forget 2 snapshots"

  Scenario: History no plan of this agent owns is listed under its own heading
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the plan "home" is now called "away"
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the section heading "Orphaned history"

  Scenario: Adopting orphaned history is reviewed, then rewrites it into the plan
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the plan "home" is now called "away"
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the adoption button into the plan "away"
    And I press Enter
    Then the dialog "Adopt history into away" opens with focus inside it
    And focus is on the button "Cancel"
    When I press Tab until focus is on the button "Adopt 2 snapshots"
    And I press Enter
    Then the dialog "Adopt history into away" is closed
    And the plan "away" shows 2 snapshot rows
    And the page shows no section heading "Orphaned history"

  Scenario: A prune review says the packs are marked now and deleted later
    Given a demo plan "home" with a tiny scratch repository
    And the repository holds data no snapshot uses
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Prune unused data"
    And I press Enter
    Then the dialog "Prune demo" opens with focus inside it
    And the dialog "Prune demo" says "marked"
    And the dialog "Prune demo" offers the button "Start prune"

  Scenario: A check review says it changes nothing, then the check runs
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Check the repository"
    And I press Enter
    Then the dialog "Check demo" opens with focus inside it
    And the dialog "Check demo" says "changes nothing"
    When I press Tab until focus is on the button "Start check"
    And I press Enter
    Then the dialog "Check demo" is closed
    And the polite region comes to contain "Check finished."

  Scenario: A forget whose plan would keep nothing is refused when it is reviewed
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    And the plan "home" keeps the last 0 snapshots
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the page shows an alert containing "would keep none of its snapshots"

  Scenario: The snapshots page with a repository's history has no accessibility violations
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the plan "home" is now called "away"
    And the plan "away" has been backed up 1 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the section heading "Plan away"
    And the page shows the section heading "Orphaned history"
    And the page has no accessibility violations

  # Every answer of the daemon arrives late in the next two scenarios, so each
  # wait of the page lasts long enough to be read.
  Scenario: The waits before a review are shown and announced
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 5 snapshots
    And the plan "home" has been backed up 2 times
    And the daemon's answers to the UI are delayed by 3 seconds
    When I open the address "/snapshots"
    Then the page shows the button "Snapshots of demo"
    When I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the paragraph "Loading…"
    When I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the polite region comes to contain "Preparing the review…"
    And the dialog "Forget snapshots of demo" opens with focus inside it

  Scenario: A review that is being confirmed says so and cannot be dismissed
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    And the daemon's answers to the UI are delayed by 3 seconds
    When I open the address "/snapshots"
    Then the page shows the button "Snapshots of demo"
    When I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    When I press Tab until focus is on the button "Forget 2 snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" says "Starting the forget"
    And the dialog "Forget snapshots of demo" has its button "Forget 2 snapshots" unavailable
    And the dialog "Forget snapshots of demo" has its button "Cancel" unavailable
    When I press Escape
    Then the dialog "Forget snapshots of demo" stays open
    And the dialog "Forget snapshots of demo" is closed
    And the polite region comes to contain "A forget is running."

  Scenario: The start of a forget is announced
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    When I press Tab until focus is on the button "Forget 2 snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" is closed
    And the polite region comes to contain "A forget is running."

  # What the engine says is English whatever the language of the page, and the
  # page marks it so.
  Scenario: The engine's words in a refusal are marked as English
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the repository has been removed
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows an alert containing "The operation failed"
    And the alert marks the engine's words as English
    And the page has no accessibility violations

  Scenario: The end of the last run is not shown once another review is asked for
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    When I press Tab until focus is on the button "Forget 2 snapshots"
    And I press Enter
    Then the main content shows the line "Forget finished."
    When I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the main content shows no line "Forget finished."

  # The answers arrive four seconds late in the next two scenarios, so the run
  # is on the page for long enough to act on it.
  Scenario: Picking the repository again reads its history afresh and leaves the run on the page
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    And the daemon's answers to the UI are delayed by 4 seconds
    When I open the address "/snapshots"
    Then the page shows the button "Snapshots of demo"
    When I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    When I press Tab until focus is on the button "Forget 2 snapshots"
    And I press Enter
    Then the main content shows the line "A forget is running."
    When I press Shift+Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the paragraph "Loading…"
    And the main content shows the line "A forget is running."
    And the main content shows the line "Forget finished."

  Scenario: Focus stays on the page when the control it was on goes with the run
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    And the daemon's answers to the UI are delayed by 4 seconds
    When I open the address "/snapshots"
    Then the page shows the button "Snapshots of demo"
    When I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    When I press Tab until focus is on the button "Forget 2 snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" is closed
    When I press Tab until focus is on the button "Cancel the run"
    Then the main content shows the line "Forget finished."
    And focus is on the heading "Maintenance of demo"

  Scenario: A repository no plan backs up to says it holds nothing, and a forget says why it has nothing to do
    Given a demo plan "home" with a tiny scratch repository
    And the config names no plan
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the main content shows the line "No snapshots yet."
    When I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the dialog "Forget snapshots of demo" says "No plan of this agent backs up to this repository"
    And the dialog "Forget snapshots of demo" offers only the button "Close"
