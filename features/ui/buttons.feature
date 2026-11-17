@ui
Feature: Buttons on every page
  Every action a page offers is the shared button (components.md §4.5,
  decision 113), drawn from the tokens rather than in the browser's own
  look, so it looks the same in every browser and what axe-core reads is
  what is painted. An action row's main action is the one filled button in
  Ladder Blue, a destructive action is drawn in the color of errors, and the
  others are outlined. A button whose press waits for the daemon shows it is
  busy, keeps focus and its name, and takes no second press until the
  answer comes.

  Scenario Outline: Every button on the <page> page is drawn from the tokens
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "<address>"
    Then the button "<button>" has the look "<look>"
    And every button on the page is drawn from the tokens, none in the browser's own look

    Examples:
      | page         | address       | button                    | look                           |
      | Operations   | /operations   | Start backup of demo-home | outlined in Ladder Blue        |
      | Plans        | /plans        | Add a plan                | filled in Ladder Blue          |
      | Plans        | /plans        | Delete demo-home          | outlined in the color of errors |
      | Repositories | /repositories | Test the repository       | outlined                       |
      | Snapshots    | /snapshots    | Snapshots of demo         | outlined                       |
      | Log          | /log          | Copy for a bug report     | outlined                       |
      | Settings     | /settings     | Set the admin password    | filled in Ladder Blue          |

  Scenario: A snapshot's own page restores as its main action and downloads beside it
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    Then the button "Restore demo-1.txt" has the look "filled in Ladder Blue"
    And the button "Download demo-1.txt" has the look "outlined"
    And every button on the page is drawn from the tokens, none in the browser's own look
    When I press Tab until focus is on the button "Download demo-1.txt"
    And I press Enter
    Then the dialog "Download demo-1.txt" opens with focus inside it
    And the button "Download" has the look "filled in Ladder Blue"

  Scenario: The sign-in page's one button is its main action
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    When I open the web UI
    Then the button "Sign in" has the look "filled in Ladder Blue"
    And every button on the page is drawn from the tokens, none in the browser's own look

  Scenario: Saving a plan shows its button busy until the daemon answers, and takes no second press
    Given a demo plan "demo-home" with a scratch repository
    And the daemon answers the next save of a plan 2 seconds late
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    And I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the button "Save the plan" is busy, a spinner a screen reader skips before its label
    And focus is on the button "Save the plan"
    When I press Enter
    Then the polite region comes to contain "The plan demo-home is saved."
    And the page asked to save a plan once

  Scenario: A row's Create shows it is busy while the repository is created
    Given a daemon with a profile imported as "demo" whose repository folder is empty
    When a backup of "demo-home" fails because no repository is there
    And I open the address "/repositories"
    And the list of repositories is slow to answer from now on
    And I skip to the content
    And I press Tab until focus is on the button "Create demo"
    And I press Enter
    Then focus is on the message "Created demo."
    And the button "Create demo" is busy, a spinner a screen reader skips before its label

  Scenario: While a repository is tested, the form's other two buttons are unavailable
    Given a daemon with a scratch demo repository and no repositories
    And the daemon answers the next test of a repository 2 seconds late
    When I open the address "/repositories"
    And I skip to the content
    And I press Tab until focus is on the textbox "Location"
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Test the repository"
    And I press Enter
    Then the button "Test the repository" is busy, a spinner a screen reader skips before its label
    And the button "Connect the repository" is unavailable
    And the button "Create the repository" is unavailable
    And the polite region comes to contain "The repository opens."

  Scenario: The Full Disk Access warning's main action is its one filled button
    Given the agent has found Full Disk Access missing
    And the browser runs on the agent's own Mac
    When I open the web UI
    Then the button "Open Full Disk Access settings" has the look "filled in Ladder Blue"
    And the button "Check again" has the look "outlined"
    And every button on the page is drawn from the tokens, none in the browser's own look

  Scenario: While the agent checks again, the warning's other button is unavailable
    Given the agent has found Full Disk Access missing
    And the browser runs on the agent's own Mac
    And the agent takes 2 seconds to check again
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the button "Check again"
    And I press Enter
    Then the button "Check again" is busy, a spinner a screen reader skips before its label
    And the button "Open Full Disk Access settings" is unavailable

  Scenario: The Pending changes page approves as its main action, and rejects and reviews beside it
    Given a demo plan "demo-home" with a scratch repository, and a proposal to add the plan "demo-docs"
    When I open the address "/proposals"
    Then every button on the page is drawn from the tokens, none in the browser's own look
    When I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    Then the button "Approve the proposal" has the look "filled in Ladder Blue"
    And the button "Reject the proposal" has the look "outlined"
    And every button on the page is drawn from the tokens, none in the browser's own look

  Scenario: Approving a proposal shows its button busy until the daemon answers, and Reject unavailable meanwhile
    Given a demo plan "demo-home" with a scratch repository, and a proposal to add the plan "demo-docs"
    And the daemon answers the next approval of a proposal 2 seconds late
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    And I press Tab until focus is on the textbox "Proposal ID"
    And I type the proposal's short ID
    And I press Tab until focus is on the button "Approve the proposal"
    And I press Enter
    Then the button "Approve the proposal" is busy, a spinner a screen reader skips before its label
    And focus is on the button "Approve the proposal"
    And the button "Reject the proposal" is unavailable
    When I press Enter
    Then the polite region comes to contain "is applied, and the configuration file is saved."
    And the page asked to approve the proposal once

  Scenario: Rejecting a proposal shows its button busy until the daemon answers, and Approve unavailable meanwhile
    Given a demo plan "demo-home" with a scratch repository, and a proposal to add the plan "demo-docs"
    And the daemon answers the next rejection of a proposal 2 seconds late
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    And I press Tab until focus is on the button "Reject the proposal"
    And I press Enter
    Then the button "Reject the proposal" is busy, a spinner a screen reader skips before its label
    And focus is on the button "Reject the proposal"
    And the button "Approve the proposal" is unavailable
    And the polite region comes to contain "is rejected. The AI agent can read your reasons."

  Scenario: Rejecting a proposal that could delete snapshots leaves its forget preview unavailable meanwhile
    Given a demo plan "demo-home" with a scratch repository, and a proposal to remove it
    And the daemon answers the next rejection of a proposal 2 seconds late
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    And I press Tab until focus is on the button "Reject the proposal"
    And I press Enter
    Then the button "Reject the proposal" is busy, a spinner a screen reader skips before its label
    And the button "Show what it would forget" is unavailable
    And the polite region comes to contain "is rejected. The AI agent can read your reasons."
