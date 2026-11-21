@ui
Feature: Making a pairing token on the hub's page
  On a hub, Settings has a Machines tab where a person makes a pairing token
  for another machine, confirmed by a step-up each time. The dialog shows the
  token once, with the hub key's fingerprint in full, the certificate it
  trusts and when it lapses, counted down on the page's own clock. Closing
  the dialog, by Close or Escape, cancels a token no machine has used, and the
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
    And the dialog "Copy the pairing token" says "Closing this dialog cancels the token if no machine has used it"
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

  Scenario: Escape cancels a token no machine has used, and the page says so
    Given a hub with sign-in off
    When I make a pairing token on the Machines tab
    And I press Escape
    Then the dialog "Copy the pairing token" is closed
    And the page says "The unused pairing token was canceled."
    And the hub has canceled the token the dialog showed
    And nothing on the page holds the pairing token the dialog showed

  Scenario: A token the hub fails to cancel on Escape is said to still work until it lapses
    Given a hub with sign-in off
    And the hub fails to cancel pairing tokens
    When I make a pairing token on the Machines tab
    And I press Escape
    Then the dialog "Copy the pairing token" is closed
    And the page says "The hub couldn't cancel the unused pairing token, so it still works until it lapses."
    And nothing on the page holds the pairing token the dialog showed

  Scenario: Cancel this token ends it in the dialog, and focus goes to Make a new token
    Given a hub with sign-in off
    When I make a pairing token on the Machines tab
    And I press Tab until focus is on the button "Cancel this token"
    And I press Enter
    Then focus is on the button "Make a new token"
    And the dialog "Copy the pairing token" says "The token is canceled"
    And the dialog "Copy the pairing token" doesn't hold the pairing token it showed
    And the hub has canceled the token the dialog showed
    When I press Enter
    Then the dialog "Copy the pairing token" is closed
    And focus is on the button "Make a pairing token"

  Scenario: Escape while Cancel this token is on its way sends no second cancel, and the page says the first one's answer
    Given a hub with sign-in off
    And the hub holds its answer to the page's first cancel of a pairing token
    When I make a pairing token on the Machines tab
    And I press Tab until focus is on the button "Cancel this token"
    And I press Enter
    And I press Escape
    Then the dialog "Copy the pairing token" is closed
    When the hub answers the cancel it held
    Then the page says "The unused pairing token was canceled."
    And the page sent the hub one cancel of the token the dialog showed, which it agreed to
    And nothing on the page holds the pairing token the dialog showed

  Scenario: A cancel answered after a later token's is said of an earlier token
    Given a hub with sign-in off
    And the hub holds the page's first cancel of a pairing token, then fails it
    When I make a pairing token on the Machines tab
    And I press Escape
    And I make another pairing token
    And I press Escape
    Then the page says "The unused pairing token was canceled."
    When the hub answers the cancel it held
    Then the page says "The hub couldn't cancel an earlier unused pairing token, so it still works until it lapses."
    And the page says "The unused pairing token was canceled."

  Scenario: A token the hub makes once Settings is left is canceled, unseen
    Given a hub with sign-in off
    And the hub holds its answer to the page's first making of a pairing token
    When I ask for a pairing token on the Machines tab
    And I leave Settings by the link "Repositories"
    And the hub answers the making it held
    Then the hub has canceled the token it made

  Scenario: What the dialog said of one token's copy isn't said of the next
    Given a hub with sign-in off
    And the page may use the clipboard
    When I make a pairing token on the Machines tab
    And I press Enter
    Then the dialog "Copy the pairing token" politely says "Copied."
    When I press Escape
    And I make another pairing token
    Then the dialog "Copy the pairing token" doesn't say "Copied."

  Scenario: A token that lapses while the dialog is open is replaced by a status, and focus goes to Make a new token
    Given a hub with sign-in off
    And the page's clock can be moved on
    When I make a pairing token on the Machines tab
    And the page is left unused for "00:15:01"
    Then focus is on the button "Make a new token"
    And the dialog "Copy the pairing token" says "Lapsed at"
    And the dialog "Copy the pairing token" doesn't hold the pairing token it showed

  Scenario: A token that lapses while its field has focus hands focus to Make a new token
    Given a hub with sign-in off
    And the page's clock can be moved on
    When I make a pairing token on the Machines tab
    And I press Shift+Tab until focus is on the textbox "The pairing token"
    And the page is left unused for "00:15:01"
    Then focus is on the button "Make a new token"

  Scenario: A browser whose clock is a day ahead still counts the token's life from the hub's answer
    Given a hub with sign-in off
    And the browser's clock is a day ahead
    When I make a pairing token on the Machines tab
    Then the dialog "Copy the pairing token" says "in 15 minutes"
    And focus is on the button "Copy the token"

  Scenario: An address another machine can't use is warned of in its field's description, and the warning goes once it is one
    Given a hub with sign-in off
    When I open the address "/settings#agents"
    Then the textbox "Address machines will use" warns "This address is loopback"
    When I set the address machines will use to "http://hub.example:8080"
    Then the textbox "Address machines will use" warns "This address is plain"
    When I set the address machines will use to "https://127.1:9443"
    Then the textbox "Address machines will use" warns "This address is loopback"
    When I set the address machines will use to "https://hub.example:9443"
    Then the textbox "Address machines will use" warns of nothing

  Scenario: A printout of the dialog holds none of the token
    Given a hub with sign-in off
    When I make a pairing token on the Machines tab
    And the page is printed
    Then the dialog "Copy the pairing token" shows no pairing token

  Scenario: A daemon that isn't a hub has no Machines tab
    Given a daemon with sign-in off
    When I open the address "/settings"
    Then Settings has no tab "Machines"

  Scenario: A hub whose key doesn't come still shows its pages, and Settings without the Machines tab
    Given a hub with sign-in off
    And the hub doesn't answer the page's read of its key
    When I open the address "/settings"
    Then Settings has no tab "Machines"

  Scenario: Signed in, the page reads the hub's key once, so a later refusal can't take the Machines tab away
    Given a hub with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    And the hub refuses each read of its key after the first
    When I open the address "/settings#agents"
    And I skip to the content
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-admin-pass"
    And I press Tab until focus is on the button "Make a pairing token"
    And I press Enter
    Then the dialog "Copy the pairing token" shows the hub key's fingerprint in full

  Scenario: The person chooses the hub's own certificate, and the dialog says the token pins it
    Given a hub with sign-in off
    When I open the address "/settings#agents"
    And I skip to the content
    And I press Tab until focus is on the radio "A certificate authority's certificate"
    And I press the up arrow until focus is on the radio "The hub's own certificate"
    And I press Tab until focus is on the textbox "Hub key ID"
    And I type the hub's key ID
    And I press Tab until focus is on the button "Make a pairing token"
    And I press Enter
    Then the dialog "Copy the pairing token" says "pinned to the hub's own certificate"
    And the page has no accessibility violations
