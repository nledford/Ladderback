@ui
Feature: A machine whose ID the hub refused for a second computer says so on the hub's page
  The hub takes one computer under each machine ID and refuses a second one
  that connects under it, as a copied data folder makes it do (§16,
  decision 150). On a running hub, Settings → Machines says so in that
  machine's note: that the hub refused another computer as this machine and
  how long ago it last did, by the hub's clock as the list's other times are.
  A warning above the tables, which names the machine, gives the two ways
  back at the page's full width: a new machine ID on the refused computer
  when it is the copy, or, when it should keep the ID, forgetting the
  computer the hub took, at the hub's terminal with its daemon stopped, by a
  command on a line of its own. The note says where that warning is. A
  machine the hub refused no second computer for notes nothing of it.

  Scenario Outline: A <standing> machine whose ID the hub refused for a second computer says so on its row, reached by keyboard alone
    Given a hub with sign-in off
    And a machine named "demo-laptop" <is listed> on the hub
    And the hub refused a second computer as "demo-laptop" 5 minutes ago
    When I open the address "/settings#machines"
    And I skip to the content
    And I press Tab until focus is on the button "<button> demo-laptop"
    Then the row of "demo-laptop" in the table "<table>" says "The hub refused another computer as demo-laptop 5 min 0 s ago. Its warning above the tables says what to do."
    And the page shows a warning alert containing "The hub refused another computer as demo-laptop. This warning stays until the command below has run, whichever way back is taken. If the refused computer is the copy, take a new machine ID on its start page, restart Ladderback and pair it again. If it should keep this ID, stop Ladderback on the computer the hub took or give that one a new ID, then stop the hub's daemon and run this at the hub's terminal:"
    And the page shows the command "ladderback pairing-token --forget-computer demo-laptop" on a line of its own
    And the page has no accessibility violations

    Examples:
      | standing | is listed          | button  | table                |
      | waiting  | waits for approval | Approve | Waiting for approval |
      | approved | is approved        | Remove  | Approved machines    |

  Scenario: Two machines the hub refused a second computer for each get a warning, in the tables' order
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" waits for approval on the hub
    And the hub refused a second computer as "demo-laptop" 5 minutes ago
    And the hub refused a second computer as "demo-desktop" 2 minutes ago
    When I open the address "/settings#machines"
    Then the row of "demo-desktop" in the table "Waiting for approval" says "The hub refused another computer as demo-desktop 2 min 0 s ago. Its warning above the tables says what to do."
    And the row of "demo-laptop" in the table "Approved machines" says "The hub refused another computer as demo-laptop 5 min 0 s ago. Its warning above the tables says what to do."
    And the warnings above the tables give the commands for "demo-desktop", then "demo-laptop"

  Scenario: A machine the hub refused no second computer for notes nothing of it
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" is approved on the hub
    And the hub refused a second computer as "demo-laptop" 5 minutes ago
    When I open the address "/settings#machines"
    Then the row of "demo-laptop" in the table "Approved machines" says "The hub refused another computer as demo-laptop 5 min 0 s ago."
    And the row of "demo-desktop" in the table "Approved machines" says nothing of a refused computer
    And the page shows no warning alert containing "The hub refused another computer as demo-desktop"

  Scenario: A refusal that a later read of the list brings in is said politely, once
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" is approved on the hub
    And the hub refused a second computer as "demo-laptop" 5 minutes ago
    When I open the address "/settings#machines"
    Then the page shows a warning alert containing "The hub refused another computer as demo-laptop."
    And the polite region never says "The hub refused another computer as demo-laptop. Its warning is above the tables."
    When the hub refused a second computer as "demo-desktop" 2 minutes ago
    And a machine named "demo-tablet" waits for approval on the hub
    Then the page shows a warning alert containing "The hub refused another computer as demo-desktop."
    And the polite region comes to contain "The hub refused another computer as demo-desktop. Its warning is above the tables."

  Scenario: A refusal that came while the tables were paused is said on resume
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" is approved on the hub
    And the hub refused a second computer as "demo-laptop" 5 minutes ago
    When I open the address "/settings#machines"
    And I skip to the content
    And I press Tab until focus is on the button "Pause live updates"
    And I press Enter
    And the hub refused a second computer as "demo-desktop" 2 minutes ago
    And a machine named "demo-tablet" waits for approval on the hub
    Then the button "Pause live updates" is described by "Live updates are paused. 2 changes are waiting."
    And the page shows no warning alert containing "The hub refused another computer as demo-desktop"
    When I press Enter
    Then the polite region comes to contain "The hub refused another computer as demo-desktop. Its warning is above the tables."
    And the page shows a warning alert containing "The hub refused another computer as demo-desktop."
