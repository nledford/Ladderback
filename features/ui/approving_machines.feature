@ui
Feature: Approving a waiting machine on the hub's page
  On a running hub, Settings → Machines lists the machines waiting for
  approval: each one's hostname, agent ID, key fingerprint in full and how
  long it has waited, with a note when its hostname looks like another
  machine's. Approve opens a review that names the machine and shows its key
  fingerprint in full, to compare with the machine's own screen, and asks for
  the step-up; it starts on Cancel. Once approved, the machine is listed
  with the approved machines (§16, decision 121, "Waiting for approval").

  Scenario: A waiting machine is approved by keyboard alone, after its key is compared
    Given a hub with sign-in off
    And a machine named "demo-laptop" waits for approval on the hub
    When I open the address "/settings#agents"
    And I skip to the content
    Then the table "Waiting for approval" lists "demo-laptop" with the key fingerprint the machine showed
    When I press Tab until focus is on the button "Approve demo-laptop"
    And the page reads the list of machines again
    Then focus is on the button "Approve demo-laptop"
    When I press Enter
    Then focus is on the button "Cancel"
    And the dialog "Approve demo-laptop?" shows the key fingerprint the machine showed
    And the page has no accessibility violations
    When I press Tab until focus is on the textbox "Hub key ID" in the dialog
    And I type the hub's key ID
    And I press Tab until focus is on the button "Approve demo-laptop" in the dialog
    And I press Enter
    Then the dialog "Approve demo-laptop?" is closed
    And the page says "demo-laptop is approved."
    And the table "Approved machines" lists "demo-laptop" with the key fingerprint the machine showed
    And the page lists no machine waiting for approval
    And the machine "demo-laptop" is told the hub approved it

  Scenario: A wrong key ID approves nothing, and the review says so
    Given a hub with sign-in off
    And a machine named "demo-laptop" waits for approval on the hub
    When I open the address "/settings#agents"
    And I approve "demo-laptop" on the Machines tab, typing a key ID that doesn't match
    Then the dialog "Approve demo-laptop?" says "That isn't right, so the machine wasn't approved."
    And the table "Waiting for approval" lists "demo-laptop" with the key fingerprint the machine showed

  Scenario: A waiting machine whose hostname looks like an approved one's is noted on its row
    Given a hub with sign-in off
    And a machine named "demo-macbook" is approved on the hub
    And a machine named "demo-rnacbook" waits for approval on the hub
    When I open the address "/settings#agents"
    Then the table "Waiting for approval" says "Looks like demo-macbook"
    And the page has no accessibility violations
