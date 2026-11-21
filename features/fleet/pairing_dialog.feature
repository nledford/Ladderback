Feature: Making a pairing token on the hub's page
  A pairing token authorizes a machine, so making one on the hub's web page
  is a high-impact action: it needs a fresh step-up every time, and beyond
  loopback it needs sign-in to be on. Closing the dialog cancels a token no
  agent has used (§16, decisions 120 and 121).

  Scenario: A token can't be made without a recent confirmation
    Given a hub with sign-in on
    When the person asks for a new pairing token and confirms with a wrong password
    Then the hub refuses it as a step-up that failed
    And the hub lists no unused token

  Scenario: A token is made with the admin password entered again
    Given a hub with sign-in on
    When the person asks for a new pairing token and confirms with the admin password
    Then the hub shows a token for its own key and the address asked for
    And the hub lists 1 unused token

  Scenario: A token pins the hub's own certificate when the person chooses it
    Given a hub with sign-in on
    When the person asks for a new pairing token for the hub's own certificate and confirms with the admin password
    Then the token pins the certificate of the hub's built-in TLS

  Scenario: A remote page can't pair when sign-in is off
    Given a hub with sign-in off
    When the person asks for a new pairing token from another computer, confirming with the hub's key ID
    Then the hub refuses it and says to set an admin password
    And the hub lists no unused token

  Scenario: A step-up without a password asks for the key ID
    Given a hub with sign-in off
    When the person asks for a new pairing token on this machine and types a key ID that doesn't match
    Then the hub refuses it as a step-up that failed
    When the person asks for a new pairing token on this machine and types the hub's key ID
    Then the hub shows a token for its own key and the address asked for

  Scenario: Closing the dialog cancels an unused token
    Given a hub with sign-in off
    And the person has made a pairing token on this machine
    When the person closes the dialog
    And an agent pairs with the token
    Then the pairing is refused as revoked

  Scenario: The life a token is shown with is counted from when the hub has kept it
    Given a hub with sign-in off
    And the hub's registry takes 2 seconds to keep a token
    When the person asks for a new pairing token on this machine and types the hub's key ID
    Then the token is shown with 898 seconds of its life left
