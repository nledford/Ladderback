@ui
Feature: Maintenance of a repository on the Snapshots page
  The Snapshots page shows a repository's history grouped by owner: a section
  for each plan of this agent, and apart from them the orphaned history no
  plan of this agent owns. A forget, an adoption, a prune and a check are
  each reviewed in a dialog before they run, and nothing changes until the
  review is confirmed. The journeys use the keyboard alone, against a daemon
  this run started with a demo plan and a scratch repository, so every
  backup, forget and prune they start is real. Maintenance from this agent
  is turned on or off on the same page, each after a dialog that says what
  it means. Unmanaged history and cancelling a running maintenance are not
  tried here.

  Scenario: The snapshots page groups a repository's history under its plan
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the section heading "Plan home"
    And the plan "home" shows 2 snapshot rows

  Scenario: A repository this agent does not maintain shows its history and says why it offers no maintenance
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the plan "home" is now called "away"
    And the repository "demo" has no maintenance setting
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the section heading "Orphaned history"
    And the page lists 2 snapshots
    And the main content shows the line "This agent does not maintain this repository, so it cannot forget, prune or check it, or adopt history into its plans."
    And the page offers no button "Forget snapshots"
    And the page offers no button "Prune unused data"
    And the page offers no button "Check the repository"
    And the page offers no adoption button into the plan "away"

  Scenario: Maintenance from this agent is turned on after a warning that only one place may maintain a repository
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the repository "demo" has no maintenance setting
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Maintain from this agent"
    And I press Enter
    Then the dialog "Maintain demo from this agent" opens with focus inside it
    And focus is on the button "Cancel"
    And the dialog "Maintain demo from this agent" says "Only one place may maintain a repository"
    When I press Escape
    Then the dialog "Maintain demo from this agent" is closed
    And focus is on the button "Maintain from this agent"
    And the page offers no button "Forget snapshots"
    When I press Enter
    And I press Tab until focus is on the button "Turn on maintenance"
    And I press Enter
    Then the dialog "Maintain demo from this agent" is closed
    And the polite region comes to contain "This agent now maintains demo."
    And the page offers the button "Forget snapshots"
    And the page offers the button "Stop maintaining from this agent"

  Scenario: Maintenance from this agent is turned off after saying what stops
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Stop maintaining from this agent"
    And I press Enter
    Then the dialog "Stop maintaining demo from this agent" opens with focus inside it
    And focus is on the button "Cancel"
    And the dialog "Stop maintaining demo from this agent" says "nothing removes old snapshots until another place maintains it"
    When I press Tab until focus is on the button "Turn off maintenance"
    And I press Enter
    Then the dialog "Stop maintaining demo from this agent" is closed
    And the polite region comes to contain "This agent no longer maintains demo."
    And the main content shows the line "This agent does not maintain this repository, so it cannot forget, prune or check it, or adopt history into its plans."
    And the page offers no button "Forget snapshots"
    And the page offers the button "Maintain from this agent"

  Scenario: A change of who maintains a repository the daemon refuses is said in an error alert of the dialog's own, which stays open
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the daemon refuses each change of who maintains a repository
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Stop maintaining from this agent"
    And I press Enter
    And I press Tab until focus is on the button "Turn off maintenance"
    And I press Enter
    Then the dialog "Stop maintaining demo from this agent" alerts "could not read or write its configuration file /etc/ladderback/config.toml"
    And the page shows an error alert containing "could not read or write its configuration file /etc/ladderback/config.toml"
    And the page has no accessibility violations
    When I press Escape
    Then focus is on the button "Stop maintaining from this agent"
    And the assertive region stays empty

  Scenario: The dialog that turns maintenance on has no accessibility violations
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the repository "demo" has no maintenance setting
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Maintain from this agent"
    And I press Enter
    Then the dialog "Maintain demo from this agent" opens with focus inside it
    And the page has no accessibility violations

  Scenario: A forget is reviewed before it runs and changes nothing until it is confirmed
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I skip to the content
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
    And I skip to the content
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
    And I skip to the content
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
    And I skip to the content
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
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the dialog "Forget snapshots of demo" says "orphaned"
    And the dialog "Forget snapshots of demo" offers the button "Forget 2 snapshots"

  Scenario: A repository holding snapshots from another host warns of it on the page and in the review
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 2 times
    And another machine called "demo-desktop" has backed up the plan "home" once
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the main content warns "This repository holds snapshots from another host, demo-desktop. Ladderback forgets and prunes it from here, and cannot tell whether that host does too. Make sure only one machine maintains it."
    When I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the dialog "Forget snapshots of demo" is described as saying "This repository holds snapshots from another host, demo-desktop. Ladderback forgets and prunes it from here, and cannot tell whether that host does too. Make sure only one machine maintains it."
    And the page has no accessibility violations

  Scenario: History no plan of this agent owns is listed under its own heading
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the plan "home" is now called "away"
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the section heading "Orphaned history"

  Scenario: Adopting orphaned history is reviewed, then rewrites it into the plan
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the plan "home" is now called "away"
    When I open the address "/snapshots"
    And I skip to the content
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
    And I skip to the content
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
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Check the repository"
    And I press Enter
    Then the dialog "Check demo" opens with focus inside it
    And the dialog "Check demo" says "changes nothing"
    When I press Tab until focus is on the button "Start check"
    And I press Enter
    Then the dialog "Check demo" is closed
    And the polite region comes to contain "Check finished. It found nothing wrong."

  Scenario: A check that finds a pack no index lists names it and says what to do
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    And a backup killed before it saved its index left a pack in the repository that no index lists
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Check the repository"
    And I press Enter
    Then the dialog "Check demo" opens with focus inside it
    When I press Tab until focus is on the button "Start check"
    And I press Enter
    Then the dialog "Check demo" is closed
    And the polite region comes to contain "It found one thing to tidy"
    And the main content names the pack no index lists in the engine's words, marked as English
    And the main content shows a line containing "the next prune tidies"

  Scenario: A forget whose plan would keep nothing is refused when it is reviewed, in an error alert said assertively
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    And the plan "home" keeps the last 0 snapshots
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the page shows an error alert containing "would keep none of its snapshots"
    And the assertive region comes to contain "would keep none of its snapshots"

  Scenario: The snapshots page with a repository's history has no accessibility violations
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the plan "home" is now called "away"
    And the plan "away" has been backed up 1 times
    When I open the address "/snapshots"
    And I skip to the content
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
    When I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
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
    When I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
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
    And I skip to the content
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
  Scenario: The engine's words in a refusal are an error alert, said assertively and marked as English
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the repository has been removed
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows an error alert containing "No repository was found at this location"
    And the assertive region comes to contain "No repository was found at this location"
    And the alert marks the sender's words as English
    And the page has no accessibility violations

  Scenario: The end of the last run is not shown once another review is asked for
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I skip to the content
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
    When I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
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
    When I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
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
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the main content shows the line "No snapshots yet."
    When I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the dialog "Forget snapshots of demo" says "No plan of this agent backs up to this repository"
    And the dialog "Forget snapshots of demo" offers only the button "Close"

  Scenario: Orphaned history that no plan of this agent could take says why it cannot be adopted
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the config names no plan
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the section heading "Orphaned history"
    And the main content shows the line "No plan of this agent could take this history, so it cannot be adopted."

  Scenario Outline: The Snapshots page is in the language of the page, history of both kinds showing
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And the plan "home" is now called "away"
    When I open the address "/snapshots?locale=<locale>"
    And I skip to the content
    And I press Tab until focus is on the button of the repository "demo"
    And I press Enter
    Then the page lists 2 snapshots
    And every visible text but the data of the tables and lists is transformed
    And the page title is transformed

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario: The Snapshots page reflows on a small screen
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 2 times
    And each snapshot's ID reaches the page in the widest hex digits
    And the plan "home" is now called "away"
    And a viewport of 320 by 256 pixels
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    Then the page shows the section heading "Orphaned history"
    And no word of a table is broken across lines
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  # rustic picks a snapshot's ID at random, so the widest IDs are given to the
  # page rather than left to chance (#1129).
  Scenario: The history tables keep their words whole on a small screen, the review of a forget open
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    And each snapshot's ID reaches the page in the widest hex digits
    And a viewport of 320 by 256 pixels
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And no word of a table is broken across lines
    And each table's row header leaves two fifths of its frame to the cells beside it
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  # Text spacing (WCAG 1.4.12) makes an ID about a sixth wider: the widest
  # case its room is set by.
  Scenario: The history tables keep the widest snapshot IDs whole on a small screen with wider text spacing, the review of a forget open
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    And each snapshot's ID reaches the page in the widest hex digits
    And a viewport of 320 by 256 pixels
    And the reader increases the text spacing
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And no word of a table is broken across lines
    And each table's row header leaves two fifths of its frame to the cells beside it
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: The review of a forget keeps to a readable width on a wide screen
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And the dialog "Forget snapshots of demo" is no wider than 768 pixels

  Scenario: The Snapshots page reflows with larger text and wider spacing, a review open
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" keeps the last 1 snapshots
    And the plan "home" has been backed up 3 times
    And each snapshot's ID reaches the page in the widest hex digits
    And a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the reader increases the text spacing
    When I open the address "/snapshots"
    And I skip to the content
    And I press Tab until focus is on the button "Snapshots of demo"
    And I press Enter
    And I press Tab until focus is on the button "Forget snapshots"
    And I press Enter
    Then the dialog "Forget snapshots of demo" opens with focus inside it
    And each table's row header leaves two fifths of its frame to the cells beside it
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: A mistake in the configuration file is shown on the Snapshots page where it is as an error alert, by keyboard alone
    Given a daemon with a scratch demo repository and no repositories
    And someone has written a setting the daemon does not know on line 6 of its configuration file
    When I open the web UI
    And I press Tab until focus is on the link "Snapshots"
    And I press Enter
    Then focus is on the main heading "Snapshots"
    And the page shows an error alert containing "did not accept its configuration file"
    And the page shows an error alert containing "line 6: repositories.demo.colour"
    And the assertive region comes to contain "did not accept its configuration file"
    And the alert marks the sender's words as English
    And the page doesn't show "demo-password"
    And the page has no accessibility violations
