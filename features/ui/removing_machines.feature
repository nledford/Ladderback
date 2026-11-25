@ui
Feature: Rejecting a waiting machine, or removing an approved one, on the hub's page
  On a running hub, Settings → Machines gives each waiting machine a Reject
  button beside Approve, and each approved machine a Remove button. Either
  opens a review that names the machine and shows its key fingerprint in
  full; it starts on Cancel and asks for no step-up, since it only takes
  authority away (decision 154). Once confirmed, the machine leaves its
  table, the page says so, and every other page of the hub drops it at once.

  Scenario: A waiting machine is rejected by keyboard alone, and the machine beside it stays
    Given a hub with sign-in off
    And a machine named "demo-laptop" waits for approval on the hub
    And a machine named "demo-desktop" waits for approval on the hub
    When I open the address "/settings#machines"
    And I skip to the content
    And I press Tab until focus is on the button "Reject demo-laptop"
    And I press Enter
    Then focus is on the button "Cancel"
    And the dialog "Reject demo-laptop?" shows the key fingerprint the machine showed
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Reject demo-laptop" in the dialog
    And I press Enter
    Then the dialog "Reject demo-laptop?" is closed
    And the page says "demo-laptop is rejected."
    And focus is on the heading "Waiting for approval"
    And the table "Waiting for approval" no longer lists "demo-laptop"
    And the table "Waiting for approval" lists "demo-desktop" with the key fingerprint the machine showed

  Scenario: An approved machine is removed by keyboard alone, and focus goes to the approved machines' heading
    Given a hub with sign-in off
    And a machine named "demo-macbook" is approved on the hub
    When I open the address "/settings#machines"
    And I skip to the content
    And I press Tab until focus is on the button "Remove demo-macbook"
    And I press Enter
    Then focus is on the button "Cancel"
    And the dialog "Remove demo-macbook?" shows the key fingerprint the machine showed
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Remove demo-macbook" in the dialog
    And I press Enter
    Then the dialog "Remove demo-macbook?" is closed
    And the page says "demo-macbook is removed."
    And the page says "No machine is approved yet."
    And focus is on the heading "Approved machines"

  Scenario: A machine removed on another page of the hub leaves the Machines tab at once
    Given a hub with sign-in off
    And the page's clock stands still from the start until a step moves it on
    And a machine named "demo-laptop" is approved on the hub
    When I open the address "/settings#machines"
    Then the table "Approved machines" lists "demo-laptop" with the key fingerprint the machine showed
    When a person removes "demo-laptop" on another page of the hub
    Then the page says "No machine is approved yet."
