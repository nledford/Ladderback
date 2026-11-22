@ui
Feature: A copied data folder is told on the start page
  When the machine identifier this computer gives isn't the one recorded
  beside its machine ID (`features/fleet/copied_data_folder.feature`), the
  start page says the data folder came from another computer: backups run,
  and forget, prune and adoption are stopped. It offers "This is a new
  computer: take a new ID", which asks first, saying what happens to this
  computer's plans, maintenance and snapshots. Once the new ID is taken, the
  notice says to restart Ladderback to use it, and links to the restart.
  Everything works by keyboard alone. An unchanged machine is told nothing.

  Scenario: A copied data folder is told, with a way to take a new ID
    Given the agent found its data folder was copied from another computer
    When I open the web UI
    Then the page says the data folder came from another computer
    And the page offers the button "This is a new computer: take a new ID"
    And the page has no accessibility violations

  Scenario: Taking a new ID, by keyboard, asks first and then asks for a restart
    Given the agent found its data folder was copied from another computer
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the button "This is a new computer: take a new ID"
    And I press Enter
    Then the dialog "Take a new machine ID?" opens with focus inside it
    And the dialog "Take a new machine ID?" says "keeps its plans"
    And the dialog "Take a new machine ID?" says "stops maintaining every repository it maintained"
    And the dialog "Take a new machine ID?" says "stay the other computer's"
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Take a new ID" in the dialog
    And I press Enter
    Then the agent was asked once to take a new machine ID
    And the page says a new machine ID was taken, to be used once Ladderback restarts
    And focus is on the heading "This data folder came from another computer"
    And the page offers the link "Restart Ladderback"
    And the page has no accessibility violations

  Scenario: Cancel takes no new ID, and focus goes back to the button
    Given the agent found its data folder was copied from another computer
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the button "This is a new computer: take a new ID"
    And I press Enter
    Then the dialog "Take a new machine ID?" opens with focus inside it
    When I press Escape
    Then the dialog "Take a new machine ID?" is closed
    And focus is on the button "This is a new computer: take a new ID"
    And the agent was never asked to take a new machine ID
    And the page says the data folder came from another computer

  Scenario: An unchanged machine sees nothing new
    Given the agent found its data folder on the computer it was made on
    When I open the web UI
    Then the page says nothing of a copied data folder
