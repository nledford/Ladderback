@ui
Feature: The hub's dashboard of its machines
  On a running hub, the Dashboard shows one card for each machine (§16,
  decision 175): the hub's own first, marked "This hub", then each approved
  machine in the order it was approved. A card says that its machine is
  online now, while the hub holds its connection, or else when it last
  connected (§16, decision 159), its status in a badge, the progress of the
  operation it runs in the shared progress bar, which is never announced
  (§16, decision 83), when its last backup ended, when it runs next and how
  many plans it has. A card whose machine needs attention, offline past a
  planned run, the last run of a plan failed, or a planned run past due,
  starts with a mark and words that say why. A sentence above the cards
  counts the machines and names, as links, those that need attention. Each
  card links to its machine's page, where its latest operations and plans
  are (features/ui/machine_page.feature), and the hub's own card has a Back
  up menu that starts a backup of one of its plans, and says when the hub
  didn't answer that the backup didn't start. While the hub doesn't say
  which machine it is, the approved machines show without its own card,
  with a sentence saying so, until it answers (#1692). Both that sentence and
  the one that says the hub didn't answer are said politely through the
  announcer once each time they appear (WCAG 4.1.3). The scenarios here show
  machines that paired from their terminals and hold no connection; the hub
  showing a connected machine online is in
  `features/fleet/staying_connected.feature`. The page follows the hub's
  events, and "Pause live updates" holds it still until it is pressed again
  (A3; §16, decision 65). It is the hub's start page, after the start page's
  notices, so the product name in the banner leads to it, and the Machines
  tab links to it. Its old address, `/dashboard`, leads to the start page on
  a hub, and is a page not found on a machine that isn't one (§16, decision
  173).

  Scenario: The hub's own card comes first, then each approved machine's in the order they were approved
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" is approved on the hub
    And a machine named "demo-tablet" waits for approval on the hub
    When I open the web UI
    Then the first card is the hub's own, marked "This hub"
    And the cards after it are, in order:
      | machine      |
      | demo-laptop  |
      | demo-desktop |
    And the page shows no machine named "demo-tablet"
    And the page has no accessibility violations

  Scenario: Each card tells its machine's status, last backup, next run and plans
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" is approved on the hub
    And a machine named "demo-tablet" is approved on the hub
    And "demo-laptop" reported these plans to the hub, and 1 more it left out:
      | plan      | last run  | last run minutes ago | next run in minutes |
      | documents | succeeded | 60                   | 30                  |
      | music     |           |                      | 90                  |
    And the hub took these reports of "demo-desktop":
      | operation | happening | minutes ago | code |
      | 1         | started   | 5           |      |
    And "demo-desktop" reported operation 1 backing up 512 of 2048 bytes at "2026-11-09T10:01:00Z"
    When I open the web UI
    Then the card "demo-laptop" tells its status "Succeeded"
    And the card "demo-laptop" tells "Plans": "3"
    And the card "demo-laptop" tells its "Last backup" and "Next run" as times
    And the card "demo-laptop" has no attention mark
    And the card "demo-desktop" tells its status "Running"
    And the progress bar "Progress of the operation running on demo-desktop" shows 25 percent
    And the card "demo-tablet" tells its status "Not run yet"
    And the card "demo-tablet" tells "Plans": "Not reported yet"
    And the page has no live region but the announcer's
    And the page has no accessibility violations

  Scenario: The sentence names an offline machine past its schedule, whose card is marked with words
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" is approved on the hub
    And "demo-laptop" reported these plans to the hub, and 0 more it left out:
      | plan      | last run  | last run minutes ago | next run in minutes |
      | documents | succeeded | 1500                 | -60                 |
    When I open the web UI
    Then the page says "3 machines. One needs attention:"
    And the page links to the page of the machine "demo-laptop" with words starting "demo-laptop last connected"
    And the card "demo-laptop" is marked as needing attention, its words starting "It last connected"
    And the card "demo-desktop" has no attention mark
    And the page has no accessibility violations

  Scenario: A failed run marks its machine's card, and the sentence's link leads to the machine's page by keyboard
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And "demo-laptop" reported these plans to the hub, and 0 more it left out:
      | plan   | last run | last run minutes ago | next run in minutes |
      | photos | failed   | 120                  |                     |
    When I open the web UI
    Then the card "demo-laptop" tells its status "Failed"
    And the card "demo-laptop" is marked as needing attention, its words starting "The last run of photos failed."
    When I skip to the content
    And I press Tab until focus is on the link "The last run of photos on demo-laptop failed."
    And I press Enter
    Then focus is on the main heading "demo-laptop"
    And the page's address is the page of the machine "demo-laptop"
    And the page has no accessibility violations

  Scenario: A card's Open link leads to its machine's page by keyboard
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the link "Open demo-laptop"
    And I press Enter
    Then focus is on the main heading "demo-laptop"
    And the page's address is the page of the machine "demo-laptop"
    And the page is titled after the machine: "demo-laptop – Ladderback"

  Scenario: At 320 pixels with text at 200 percent the cards stand in one column and nothing scrolls sideways
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    And a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    When I open the web UI
    Then the cards stand in one column, each inside the page
    And the page doesn't scroll sideways
    And the page has no accessibility violations

  Scenario: On a wide page the cards stand side by side
    Given a viewport of 1280 by 800 pixels
    And a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    When I open the web UI
    Then the card "demo-laptop" stands beside the hub's own

  Scenario: The hub's own Back up menu starts a backup of the plan chosen, by keyboard, and closes back to its button
    Given a hub with sign-in off and a demo plan "demo-home" with a tiny scratch repository
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the hub's own Back up button
    And I press Enter
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    Then the hub's own Back up button is collapsed
    And focus is on the hub's own Back up button
    And the polite region reads "Backup started."
    And the hub's own card tells its status "Succeeded"
    And the page has no accessibility violations

  Scenario: The hub's own Back up menu says the backup didn't start when the hub doesn't answer
    Given a hub with sign-in off and a demo plan "demo-home" with a tiny scratch repository
    And the hub doesn't answer a request to start a backup
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the hub's own Back up button
    And I press Enter
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    Then the page shows an error alert containing "The backup didn't start because the hub couldn't be reached."
    And focus is on the hub's own Back up button
    When I press Enter
    Then the button "Start backup of demo-home" can be pressed again
    And the page has no accessibility violations

  Scenario Outline: The hub's own Back up menu opens inside the window on a narrow page, at <size> percent text
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to <size> percent
    And a hub with sign-in off and a demo plan "demo-home" with a tiny scratch repository
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the hub's own Back up button
    And I press Enter
    Then the menu of the hub's own Back up button lies inside the window

    Examples:
      | size |
      | 100  |
      | 200  |

  Scenario: A hub with no plan has no Back up menu on its card
    Given a hub with sign-in off
    When I open the web UI
    Then the hub's own card has its Open link and no button

  Scenario: A paired machine's card has no Back up menu
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    When I open the web UI
    Then the card "demo-laptop" has the link "Open demo-laptop" and no button

  Scenario: A dashboard the hub didn't answer says so politely, and asks again until it does
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the hub fails its first two answers about its machines
    When I open the web UI
    Then the page says "The hub didn't answer, so its machines can't be shown. The page asks again in a few seconds."
    And the polite region reads "The hub didn't answer, so its machines can't be shown. The page asks again in a few seconds."
    And the polite region keeps reading "The hub didn't answer, so its machines can't be shown. The page asks again in a few seconds."
    And the page shows the machine "demo-laptop"

  Scenario: A dashboard shows the approved machines while the hub doesn't say which machine it is, says so politely, and shows its own card once it does
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the hub doesn't answer when asked which machine it is
    When I open the web UI
    Then the page says "The hub didn't say which machine it is, so this hub's own machine can't be shown. The page asks again in a few seconds."
    And the polite region reads "The hub didn't say which machine it is, so this hub's own machine can't be shown. The page asks again in a few seconds."
    And the polite region keeps reading "The hub didn't say which machine it is, so this hub's own machine can't be shown. The page asks again in a few seconds."
    And the page shows the machine "demo-laptop"
    And the page has no accessibility violations
    When the hub answers when asked which machine it is
    Then the first card is the hub's own, marked "This hub"
    And the page no longer says "The hub didn't say which machine it is, so this hub's own machine can't be shown. The page asks again in a few seconds."

  Scenario: A machine approved while the dashboard is open shows at once, and a pause holds the page until it resumes
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" waits for approval on the hub
    When I open the web UI
    Then the page shows the machine "demo-laptop"
    When I skip to the content
    And I press Tab until focus is on the button "Pause live updates"
    And I press Enter
    And a person approves "demo-desktop" on another page of the hub
    Then the button "Pause live updates" is described by "Live updates are paused. One change is waiting."
    And the page shows no machine named "demo-desktop"
    And focus stays on the button "Pause live updates"
    When I press Enter
    Then the polite region reads "Live updates are on again. One machine changed while they were paused."
    And the page shows the machine "demo-desktop"
    And the page has no live region but the announcer's

  Scenario: A hub's start page says its notices first, then shows each machine's card
    Given a hub with sign-in off
    And the agent found its data folder was copied from another computer
    And a machine named "demo-laptop" is approved on the hub
    When I open the web UI
    Then the main heading reads "Dashboard"
    And the page is titled "Dashboard – Ladderback"
    And the page shows the machine "demo-laptop"
    And the page says the data folder came from another computer
    And the region "This data folder came from another computer" comes before the cards
    And the page doesn't show "There are no plans yet"
    And the page has no accessibility violations

  Scenario: The product name in the banner leads a hub's reader to its machines, by keyboard
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    When I open the address "/operations"
    And I press Tab until focus is on the link "Ladderback"
    And I press Enter
    Then focus is on the main heading "Dashboard"
    And the page shows the machine "demo-laptop"
    And the link "Ladderback" in the banner is marked as the current page
    And the page is titled "Dashboard – Ladderback"
    And the page has no accessibility violations

  Scenario: A hub's old dashboard address leads to its start page
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    When I open the address "/dashboard"
    Then the page shows the machine "demo-laptop"
    And the page's address is "/"
    And the main heading reads "Dashboard"
    And the page is titled "Dashboard – Ladderback"

  Scenario: The dashboard's address is a page not found on a machine that isn't a hub
    When I open the address "/dashboard"
    Then the main heading reads "Page not found"
    And the page is titled "Page not found – Ladderback"
    And the page has no accessibility violations

  Scenario: The Machines tab leads to the hub's start page, where the machines are
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    When I open the address "/settings#machines"
    And I skip to the content
    And I press Tab until focus is on the link "See the machines on the dashboard"
    And I press Enter
    Then focus is on the main heading "Dashboard"
    And the page's address is "/"
    And the page shows the machine "demo-laptop"
