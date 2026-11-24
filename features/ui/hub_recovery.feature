@ui
Feature: Ending hub recovery mode from the Machines page
  When a restored hub has heard a newer lease than its counters hold, it
  pauses grants and renewals until a person confirms that the old hub is
  stopped or gone. The Machines page explains why and lets the person end
  recovery mode with the hub key's step-up.

  Scenario: A person ends recovery mode by keyboard alone
    Given a hub with sign-in off
    And the hub is in recovery mode
    When I open the address "/settings#agents"
    And I skip to the content
    Then the page says "A machine showed this hub a newer lease than it counts, as after a restore from an older backup. Until you confirm the hub that granted it is stopped or gone, this hub grants and renews no lease, so two copies of the hub can't give one repository to two machines."
    When I press Tab until focus is on the button "End recovery mode"
    And I press Enter
    Then focus is on the button "Cancel"
    And the dialog "End recovery mode?" opens with focus inside it
    When I press Tab until focus is on the textbox "Hub key fingerprint" in the dialog
    And I type the hub key's fingerprint
    And I press Tab until focus is on the button "End recovery mode" in the dialog
    And I press Enter
    Then the dialog "End recovery mode?" is closed
    And the page says "Recovery mode ended. This hub grants leases again."
    And the page has no accessibility violations
