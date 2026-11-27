@ui
Feature: A machine's page on the hub
  Each machine on a hub's dashboard has a page of its own,
  `/machines/<machine-id>`, named by its hostname (§16, decision 175). It
  says that the machine is online now, while the hub holds its connection,
  or else when it last connected (§16, decision 159), and shows its latest
  operations as its reports told them (when each started and ended, how it
  came out, and the error code of a failure), with a warning when the hub
  let go of its oldest reports. It shows the machine's plans as it last
  reported them (how and when each last ran, and when it runs next), a
  warning when it has more than it reports, and the progress of the
  operation it runs in the shared progress bar, which is never announced
  (§7.2; §16, decision 83). A machine's times read in the viewer's zone,
  and in its own beside them when the two differ (#115). The hub's own page
  shows its own plans and operations. The page follows the hub's events, and
  "Pause live updates" holds it still, as the dashboard does (A3; §16,
  decision 65). It is a page not found for a machine the hub hasn't
  approved, and on a machine that isn't a hub. While the hub doesn't say
  which machine it is, an approved machine's page shows, and any other
  says so in place of a page not found, since it may be the hub's own
  (#1692).

  Scenario: A machine's page shows its latest operations, a failure's code and the reports let go of
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the hub took these reports of "demo-laptop":
      | operation | happening | minutes ago | code                   |
      | 1         | started   | 90          |                        |
      | 1         | succeeded | 80          |                        |
      | 2         | started   | 30          |                        |
      | 2         | failed    | 25          | repository-unreachable |
      | 3         | started   | 5           |                        |
    And the hub let go of 3 of the reports of "demo-laptop"
    When I open the page of the machine "demo-laptop"
    Then the main heading reads "demo-laptop"
    And the page is titled after the machine: "demo-laptop – Ladderback"
    And the table "Latest operations of demo-laptop" lists, newest first:
      | result    | error code             |
      | Running   |                        |
      | Failed    | repository-unreachable |
      | Succeeded |                        |
    And the page says "The hub let go of 3 of this machine's oldest reports to keep within its limit, so older operations may be missing."
    And the page has no accessibility violations

  Scenario: A machine's page in another time zone shows its own time beside the viewer's
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the machine "demo-laptop" runs in the time zone "Pacific/Chatham"
    When I open the page of the machine "demo-laptop"
    Then the time "demo-laptop" last connected reads in the zone "Pacific/Chatham" too

  Scenario: A machine's page shows its plans as it last reported them, and says when it left some out
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And "demo-laptop" reported these plans to the hub, and 2 more it left out:
      | plan      | last run  | last run minutes ago | next run in minutes |
      | documents | succeeded | 60                   | 30                  |
      | photos    | failed    | 120                  |                     |
      | music     |           |                      | 90                  |
    When I open the page of the machine "demo-laptop"
    Then the table "Plans of demo-laptop" lists, in order:
      | plan      | last run    | next run     |
      | documents | Succeeded   |              |
      | photos    | Failed      | None planned |
      | music     | Not run yet |              |
    And the page says "This machine has 2 more plans than it reports to the hub, so those plans aren't shown."
    And the page has no accessibility violations

  Scenario: A machine's page says when the machine reported no plans, or none yet
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-tablet" is approved on the hub
    And "demo-tablet" reported no plans to the hub
    When I open the page of the machine "demo-laptop"
    Then the page says "This machine hasn't reported its plans yet."
    And the page says "No operation reported yet."
    When I open the page of the machine "demo-tablet"
    Then the page says "This machine reported no plans."

  Scenario: A machine's running operation shows its progress on its page, which the keyboard reaches
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the hub took these reports of "demo-laptop":
      | operation | happening | minutes ago | code |
      | 1         | started   | 5           |      |
    And "demo-laptop" reported operation 1 backing up 512 of 2048 bytes at "2026-11-09T10:01:00Z"
    When I open the page of the machine "demo-laptop"
    And I skip to the content
    And I press Tab until focus is on the progressbar "Progress of the operation running on demo-laptop"
    Then the progress bar "Progress of the operation running on demo-laptop" shows 25 percent
    And the page has no live region but the announcer's
    And the page has no accessibility violations

  Scenario: The hub's own page shows its own plans, and the backup its Back up menu started
    Given a hub with sign-in off and a demo plan "demo-home" with a tiny scratch repository
    When I open the web UI
    And I skip to the content
    And I press Tab until focus is on the hub's own Back up button
    And I press Enter
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    Then the hub's own card tells its status "Succeeded"
    When I press Shift+Tab until focus is on the hub's own Open link
    And I press Enter
    Then focus is on the hub's own main heading
    And the hub's own table of plans lists "demo-home" with its last run "Succeeded"
    And the hub's own table of latest operations lists one, "Succeeded"
    And the page has no accessibility violations

  Scenario: A machine's page is a page not found for a machine the hub hasn't approved
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-tablet" waits for approval on the hub
    When I open the page of the machine "demo-laptop"
    Then the main heading reads "demo-laptop"
    When I open the page of the machine "demo-tablet"
    Then the main heading reads "Page not found"
    And the page is titled "Page not found – Ladderback"
    When I open the address "/machines/018f0000-0000-7000-8000-00000000dead"
    Then the main heading reads "Page not found"
    And the page has no accessibility violations

  Scenario: A machine's page shows while the hub doesn't say which machine it is, and another says so
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the hub doesn't answer when asked which machine it is
    When I open the page of the machine "demo-laptop"
    Then the main heading reads "demo-laptop"
    When I open the address "/machines/018f0000-0000-7000-8000-00000000dead"
    Then the page says "The hub didn't say which machine it is, so this hub's own machine can't be shown. The page asks again in a few seconds."
    And the main heading reads "Machine"
    And the page has no accessibility violations

  Scenario: A machine's page is a page not found on a machine that isn't a hub
    When I open the address "/machines/018f0000-0000-7000-8000-00000000dead"
    Then the main heading reads "Page not found"
    And the page is titled "Page not found – Ladderback"
