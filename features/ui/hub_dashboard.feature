@ui
Feature: The hub's dashboard of its approved machines
  On a running hub, the Dashboard shows each approved machine: when it last
  connected, its latest operations as its reports told them (when each
  started and ended, how it came out, and the error code of a failure), and
  a warning when the hub let go of its oldest reports. A machine connects
  only to send its reports, so the page says when it last connected rather
  than whether it is online (§16, decision NN). A machine's times read in
  the viewer's zone, and in its own beside them when the two differ (#115).
  The page follows the hub's events, and "Pause live updates" holds it still
  until it is pressed again (A3; §16, decision 65). It is the hub's main
  page: the hub's start page and its Machines tab lead to it.

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
    When I open the address "/dashboard"
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
    When I open the address "/dashboard"
    Then the time "demo-laptop" last connected reads in the zone "Pacific/Chatham" too

  Scenario: A dashboard the hub didn't answer says so, and asks again until it does
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And the hub fails its first two answers about its machines
    When I open the address "/dashboard"
    Then the page says "The hub didn't answer, so its machines can't be shown. The page asks again in a few seconds."
    And the page shows the machine "demo-laptop"

  Scenario: A machine approved while the dashboard is open shows at once, and a pause holds the page until it resumes
    Given a hub with sign-in off
    And a machine named "demo-laptop" is approved on the hub
    And a machine named "demo-desktop" waits for approval on the hub
    When I open the address "/dashboard"
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

  Scenario: A hub's start page leads to its dashboard
    Given a hub with sign-in off
    When I open the address "/"
    And I skip to the content
    And I press Tab until focus is on the link "See the approved machines' latest operations"
    And I press Enter
    Then focus is on the main heading "Dashboard"
    And the page is titled "Dashboard – Ladderback"

  Scenario: The Machines tab leads to the dashboard
    Given a hub with sign-in off
    When I open the address "/settings#agents"
    And I skip to the content
    And I press Tab until focus is on the link "See the approved machines' latest operations"
    And I press Enter
    Then focus is on the main heading "Dashboard"
