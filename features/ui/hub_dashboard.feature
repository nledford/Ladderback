@ui
Feature: The hub's dashboard of its approved machines
  On a running hub, the Dashboard shows each approved machine: that it is
  online now, while the hub holds its connection, or else when it last
  connected (§16, decision 159), its latest operations as its reports told
  them (when each started and ended, how it came out, and the error code of
  a failure), and a warning when the hub let go of its oldest reports. It
  shows the machine's plans as it last reported them (how and when each
  last ran, and when it runs next), a warning when it has more than it
  reports, and the progress of the operation it runs in the shared progress
  bar, which is never announced (§7.2; §16, decision 83). The scenarios
  here show machines that paired from their terminals and hold no
  connection; the hub showing a connected machine online is in
  `features/fleet/staying_connected.feature`. A machine's times read in the
  viewer's zone, and in its own beside them when the two differ (#115). The
  page follows the hub's events, and "Pause live updates" holds it still until
  it is pressed again (A3; §16, decision 65). It is the hub's start page,
  after the start page's notices, so the product name in the banner leads
  to it, and the Machines tab links to it; the hub's own plans and
  operations stay on its Plans and Operations pages. Its old address,
  `/dashboard`, leads to the start page on a hub, and is a page not found
  on a machine that isn't one (§16, decision 173).

  Scenario: Each approved machine shows its latest operations, a failure's code and the reports let go of
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" is approved on the hub
    And a machine named "demo-tablet" waits for approval on the hub
    And the hub took these reports of "demo-laptop":
      | operation | happening | minutes ago | code                   |
      | 1         | started   | 90          |                        |
      | 1         | succeeded | 80          |                        |
      | 2         | started   | 30          |                        |
      | 2         | failed    | 25          | repository-unreachable |
      | 3         | started   | 5           |                        |
    And the hub let go of 3 of the reports of "demo-laptop"
    When I open the web UI
    Then the table "Latest operations of demo-laptop" lists, newest first:
      | result    | error code             |
      | Running   |                        |
      | Failed    | repository-unreachable |
      | Succeeded |                        |
    And the page says "The hub let go of 3 of this machine's oldest reports to keep within its limit, so older operations may be missing."
    And the page says "No operation reported yet."
    And the page shows no machine named "demo-tablet"
    And the page has no accessibility violations

  Scenario: A machine in another time zone shows its own time beside the viewer's
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the machine "demo-laptop" runs in the time zone "Pacific/Chatham"
    When I open the web UI
    Then the time "demo-laptop" last connected reads in the zone "Pacific/Chatham" too

  Scenario: A dashboard the hub didn't answer says so, and asks again until it does
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the hub fails its first two answers about its machines
    When I open the web UI
    Then the page says "The hub didn't answer, so its machines can't be shown. The page asks again in a few seconds."
    And the page shows the machine "demo-laptop"

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

  Scenario: A hub's start page says its notices first, then shows each approved machine
    Given a hub with sign-in off
    And the agent found its data folder was copied from another computer
    And a machine named "demo-laptop" is approved on the hub
    When I open the web UI
    Then the main heading reads "Dashboard"
    And the page is titled "Dashboard – Ladderback"
    And the page shows the machine "demo-laptop"
    And the page says the data folder came from another computer
    And the region "This data folder came from another computer" comes before the region "demo-laptop"
    And the page doesn't show "There are no plans yet"
    And the page has no accessibility violations

  Scenario: The product name in the banner leads a hub's reader to its approved machines, by keyboard
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

  Scenario: The Machines tab leads to the hub's start page, where the approved machines are
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    When I open the address "/settings#machines"
    And I skip to the content
    And I press Tab until focus is on the link "See the approved machines' latest operations"
    And I press Enter
    Then focus is on the main heading "Dashboard"
    And the page's address is "/"
    And the page shows the machine "demo-laptop"

  Scenario: Each machine shows its plans as it last reported them, and says when it left some out or reported none
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" is approved on the hub
    And a machine named "demo-tablet" is approved on the hub
    And "demo-tablet" reported no plans to the hub
    And "demo-laptop" reported these plans to the hub, and 2 more it left out:
      | plan      | last run  | last run minutes ago | next run in minutes |
      | documents | succeeded | 60                   | 30                  |
      | photos    | failed    | 120                  |                     |
      | music     |           |                      | 90                  |
    When I open the web UI
    Then the table "Plans of demo-laptop" lists, in order:
      | plan      | last run    | next run     |
      | documents | Succeeded   |              |
      | photos    | Failed      | None planned |
      | music     | Not run yet |              |
    And the page says "This machine has 2 more plans than it reports to the hub, so those plans aren't shown."
    And the page says "This machine hasn't reported its plans yet."
    And the page says "This machine reported no plans."
    And the page has no accessibility violations

  Scenario: A machine's running operation shows its progress, which the keyboard reaches
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the hub took these reports of "demo-laptop":
      | operation | happening | minutes ago | code |
      | 1         | started   | 5           |      |
    And "demo-laptop" reported operation 1 backing up 512 of 2048 bytes at "2026-11-09T10:01:00Z"
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the progressbar "Progress of the operation running on demo-laptop"
    Then the progress bar "Progress of the operation running on demo-laptop" shows 25 percent
    And the page has no live region but the announcer's
    And the page has no accessibility violations
