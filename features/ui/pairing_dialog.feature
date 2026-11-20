@ui
Feature: Making a pairing token on the hub's page
  On a hub, Settings has an Agents tab where a person makes a pairing token
  for another machine, confirmed by a step-up each time. The dialog shows the
  token once, with the hub key's fingerprint in full, the certificate it
  trusts and when it lapses, counted down on the page's own clock. Closing
  the dialog, by Close or Escape, cancels a token no agent has used, and the
  dialog says so beside Close (§16, decision 121, "The hub's dialog").

  Scenario: A token is made by keyboard alone and shown once, with the hub key's fingerprint in full
    Given a hub with sign-in off
    And the page may use the clipboard
    When I open the address "/settings#agents"
    And I skip to the content
    And I press Tab until focus is on the textbox "Hub key ID"
    And I type the hub's key ID
    And I press Tab until focus is on the button "Make a pairing token"
    And I press Enter
    Then focus is on the button "Copy the token"
    And the dialog "Copy the pairing token" shows the hub key's fingerprint in full
    And the dialog "Copy the pairing token" says "in 15 minutes"
    And the dialog "Copy the pairing token" says "a certificate authority's certificate"
    And the dialog "Copy the pairing token" says "Closing this dialog cancels the token if no agent has used it"
    And the page has no accessibility violations
    When I press Enter
    Then the clipboard holds the pairing token the dialog showed
    When I press Shift+Tab until focus is on the textbox "The pairing token"
    Then the textbox "The pairing token" holds the pairing token the dialog showed

  Scenario: With sign-in on, the admin password confirms a token
    Given a hub with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    When I open the address "/settings#agents"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Tab until focus is on the button "Make a pairing token"
    And I press Enter
    Then the dialog "Copy the pairing token" shows the hub key's fingerprint in full

  Scenario: Escape cancels a token no agent has used, and the page says so
    Given a hub with sign-in off
    When I make a pairing token on the Agents tab
    And I press Escape
    Then the dialog "Copy the pairing token" is closed
    And the page says "The unused pairing token was canceled."
    And the hub has canceled the token the dialog showed
    And nothing on the page holds the pairing token the dialog showed

  Scenario: A token the hub fails to cancel on Escape is said to still work until it lapses
    Given a hub with sign-in off
    And the hub fails to cancel pairing tokens
    When I make a pairing token on the Agents tab
    And I press Escape
    Then the dialog "Copy the pairing token" is closed
    And the page says "The hub couldn't cancel the unused pairing token, so it still works until it lapses."
    And nothing on the page holds the pairing token the dialog showed

  Scenario: Cancel this token ends it in the dialog, and focus goes to Make a new token
    Given a hub with sign-in off
    When I make a pairing token on the Agents tab
    And I press Tab until focus is on the button "Cancel this token"
    And I press Enter
    Then focus is on the button "Make a new token"
    And the dialog "Copy the pairing token" says "The token is canceled"
    And the dialog "Copy the pairing token" doesn't hold the pairing token it showed
    And the hub has canceled the token the dialog showed
    When I press Enter
    Then the dialog "Copy the pairing token" is closed
    And focus is on the textbox "Hub key ID"

  Scenario: A token that lapses while the dialog is open is replaced by a status, and focus goes to Make a new token
    Given a hub with sign-in off
    And the page's clock can be moved on
    When I make a pairing token on the Agents tab
    And the page is left unused for "00:15:01"
    Then focus is on the button "Make a new token"
    And the dialog "Copy the pairing token" says "Lapsed at"
    And the dialog "Copy the pairing token" doesn't hold the pairing token it showed

  Scenario: A browser whose clock is a day ahead still counts the token's life from the hub's answer
    Given a hub with sign-in off
    And the browser's clock is a day ahead
    When I make a pairing token on the Agents tab
    Then the dialog "Copy the pairing token" says "in 15 minutes"
    And focus is on the button "Copy the token"

  Scenario: An address another machine can't use is warned of, and the warning goes once it is one
    Given a hub with sign-in off
    When I open the address "/settings#agents"
    Then the page warns "This address is loopback"
    When I set the address agents will use to "http://hub.example:8080"
    Then the page warns "This address is plain"
    When I set the address agents will use to "https://hub.example:9443"
    Then the page warns of no address

  Scenario: A printout of the dialog holds none of the token
    Given a hub with sign-in off
    When I make a pairing token on the Agents tab
    And the page is printed
    Then the dialog "Copy the pairing token" shows no pairing token

  Scenario: A daemon that isn't a hub has no Agents tab
    Given a daemon with sign-in off
    When I open the address "/settings"
    Then Settings has no tab "Agents"
