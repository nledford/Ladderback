@ui
Feature: A copied data folder, or a machine its hub refused or removed, is told on the start page
  When the machine identifier this computer gives isn't the one recorded
  beside its machine ID (`features/fleet/copied_data_folder.feature`), the
  start page says the data folder came from another computer: backups run,
  and forget, prune and adoption are stopped. It offers "This is a new
  computer: take a new ID", which asks first, saying what happens to this
  computer's plans, maintenance and snapshots. Once the new ID is taken, the
  notice says to restart Ladderback to use it, and links to the restart.
  A machine its hub refused, signed, as a second computer under its machine
  ID (`features/fleet/refused_new_id.feature`) is told so, and offered the
  same button and review; it may be the original computer, so the page
  never says its data folder came from another one, nor once its new ID is
  taken. A machine its hub removed, or rejected while it waited, signed
  (`features/fleet/removed_new_id.feature`), is told so and offered a new
  machine ID with which to pair again; its review says the snapshots under
  the old ID stay there and that the hub's owner approves it again.
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
    And focus is on the heading "New machine ID taken"
    And the page offers the link "Restart Ladderback"
    And the page has no accessibility violations

  Scenario: A machine its hub refused is told, and takes a new ID by keyboard alone
    Given the agent's hub refused it, signed, as a second computer
    When I open the web UI
    Then the page says the hub took another computer as this one
    And the page doesn't say the data folder came from another computer
    And the page offers the button "This is a new computer: take a new ID"
    And the page has no accessibility violations
    When I skip to the content
    And I press Tab until focus is on the button "This is a new computer: take a new ID"
    And I press Enter
    Then the dialog "Take a new machine ID?" opens with focus inside it
    And the dialog "Take a new machine ID?" says "stops maintaining every repository it maintained"
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Take a new ID" in the dialog
    And I press Enter
    Then the agent was asked once to take a new machine ID
    And the page says a new machine ID was taken, to be used once Ladderback restarts
    And focus is on the heading "New machine ID taken"
    And the page doesn't say the data folder came from another computer
    And the page offers the link "Restart Ladderback"
    And the page has no accessibility violations

  Scenario: A machine its hub removed is told, and takes a new ID by keyboard alone
    Given the agent's hub removed it, signed
    When I open the web UI
    Then the page says the hub removed this machine, and to pair it again under a new ID
    And the page doesn't say the data folder came from another computer
    And the page has no accessibility violations
    When I skip to the content
    And I press Tab until focus is on the button "Take a new machine ID"
    And I press Enter
    Then the dialog "Take a new machine ID?" opens with focus inside it
    And the dialog "Take a new machine ID?" says "stops maintaining every repository it maintained"
    And the dialog "Take a new machine ID?" says "approves it again"
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Take a new ID" in the dialog
    And I press Enter
    Then the agent was asked once to take a new machine ID
    And the page says a new machine ID was taken, to be used once Ladderback restarts
    And focus is on the heading "New machine ID taken"
    And the page offers the link "Restart Ladderback"

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
