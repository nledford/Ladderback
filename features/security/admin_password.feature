Feature: Setting and changing the admin password in Settings
  The admin password can be set and changed in Settings as well as with
  `ladderback set-password`, by the same rules and with the same effects
  (§10.1, decision 70). With sign-in off, anyone who can reach the page can
  already run and change everything, so setting one there opens nothing new:
  it waits for a restart to turn sign-in on, as one the command sets does.
  With sign-in on, changing it takes the current password, counted as a
  sign-in, and a session, never an API token; a new admin password ends every
  session and token. Turning sign-in off and recovering a forgotten password
  stay at the terminal.

  Rule: With sign-in off, Settings sets the admin password by the rules set-password keeps

    Background:
      Given no admin password is set

    Scenario: A password of 8 characters or more is kept as the admin password
      When the admin password "demo-admin-pass" is set in Settings
      Then the admin password is "demo-admin-pass"

    Scenario: A password shorter than 8 characters is refused, and none is kept
      When the admin password "short" is set in Settings
      Then the password is refused as shorter than 8 characters
      And no admin password is kept

  Rule: With sign-in on, the current password changes the admin password, and a wrong one counts as a failed sign-in

    Background:
      Given the admin password is "demo-admin-pass"
      And the admin has signed in from 192.0.2.10

    Scenario: The current password changes the admin password, and the new one signs in
      When the admin changes the admin password from "demo-admin-pass" to "another-admin-pass" from 192.0.2.10
      Then the admin password is "another-admin-pass"
      When the admin signs in with "another-admin-pass" from 192.0.2.10
      Then the admin is signed in

    Scenario: A changed admin password ends every session and every API token
      Given the admin has created an API token "nightly-mcp" with the scopes "read"
      When the admin changes the admin password from "demo-admin-pass" to "another-admin-pass" from 192.0.2.10
      Then the session is not live
      When a program presents the token from 192.0.2.20
      Then the token is refused as invalid

    Scenario: A wrong current password is refused, changes nothing, and makes the next sign-in wait
      When the admin changes the admin password from "not-the-password" to "another-admin-pass" from 192.0.2.10
      Then the change is refused as a wrong current password
      And the admin password is "demo-admin-pass"
      And the session is live
      When the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the sign-in is refused, to be tried again in 1 second

    Scenario: A change during the wait after a failed sign-in is refused unchecked
      When the admin signs in with "not-the-password" from 192.0.2.10
      And the admin changes the admin password from "demo-admin-pass" to "another-admin-pass" from 192.0.2.10
      Then the change is refused, to be tried again in 1 second
      And the admin password is "demo-admin-pass"

    Scenario: A right current password starts the waits over, as a sign-in does
      When the admin signs in with "not-the-password" from 192.0.2.10
      And 1 second passes
      And the admin changes the admin password from "demo-admin-pass" to "another-admin-pass" from 192.0.2.10
      And the admin signs in with "not-the-password" from 192.0.2.10
      And the admin signs in with "another-admin-pass" from 192.0.2.10
      Then the sign-in is refused, to be tried again in 1 second

    Scenario: A new password shorter than 8 characters is refused, and the admin password stays
      When the admin changes the admin password from "demo-admin-pass" to "short" from 192.0.2.10
      Then the password is refused as shorter than 8 characters
      And the admin password is "demo-admin-pass"
      And the session is live

  @e2e
  Rule: Over HTTP, the admin password is set with sign-in off, and changed only in a session with the current password

    Scenario: With sign-in off, an admin password set over HTTP waits for a restart, and is never logged
      Given Ladderback is serving on loopback with sign-in off
      When the admin password "demo-admin-pass" is set over HTTP
      Then the request is answered with status 204
      And the answer doesn't hold "demo-admin-pass"
      And Ladderback says sign-in is still off and a restart turns it on
      And the data folder holds the admin password's hash, readable by its owner alone
      And no file Ladderback keeps contains "demo-admin-pass"
      And the daemon's log doesn't hold "demo-admin-pass"
      When Ladderback is restarted
      Then sign-in is on

    Scenario: With sign-in on, the admin password changed over HTTP ends the session that changed it
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"
      When the admin changes the admin password over HTTP from "demo-admin-pass" to "another-admin-pass"
      Then the request is answered with status 204
      And the answer doesn't hold "another-admin-pass"
      And the daemon's log doesn't hold "another-admin-pass"
      And the daemon's log doesn't hold "demo-admin-pass"
      Given a "GET" request for "/api/v1/plans" with the session cookie
      When the request is sent
      Then the request is refused with status 401 and the error code "sign-in-required"
      When the admin signs in over HTTP with "another-admin-pass"
      Then the sign-in sets a session cookie with "HttpOnly", "SameSite=Strict" and "Path=/"

    Scenario: With sign-in on, a wrong current password over HTTP is refused, and the next sign-in waits
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"
      When the admin changes the admin password over HTTP from "not-the-password" to "another-admin-pass"
      Then the route answers with status 403 and the error code "current-password-wrong"
      And the daemon's log doesn't hold "not-the-password"
      When the admin signs in over HTTP with "demo-admin-pass"
      Then the sign-in is refused with status 429, to be tried again after 1 second

    Scenario: An API token can't change the admin password, though the session that made it can
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"
      And the admin has created an API token over HTTP with the scopes "read, run, preview, propose" and the label "every-scope"
      When the admin changes the admin password over HTTP from "demo-admin-pass" to "another-admin-pass" with the API token
      Then the route answers with status 403 and the error code "api-token-scope"
      And the refusal names the scope "session"
      When the admin changes the admin password over HTTP from "demo-admin-pass" to "another-admin-pass"
      Then the request is answered with status 204

  @ui
  Rule: Settings sets and changes the admin password by keyboard alone

    Scenario: With sign-in off, an admin password set in Settings by keyboard alone waits for a restart
      Given a daemon with sign-in off
      When I open the address "/settings#admin-password"
      Then the page shows a message containing "Once it is set, sign-in protects Ladderback"
      When I skip to the content
      And I press Tab until focus is on the textbox "New password"
      And I type "demo-admin-pass"
      And I press Tab until focus is on the textbox "New password again"
      And I type "demo-admin-pass"
      And I press Tab until focus is on the button "Set the admin password"
      And I press Enter
      Then the page shows a message containing "Admin password set. Restart Ladderback to turn sign-in on."
      And the page doesn't show "demo-admin-pass"
      And the page has no accessibility violations

    Scenario: A password shown while it was typed is hidden again once it is set and the form is cleared
      Given a daemon with sign-in off
      When I open the address "/settings#admin-password"
      And I skip to the content
      And I press Tab until focus is on the textbox "New password"
      And I type "demo-admin-pass"
      And I press Tab
      Then focus is on the button "Show password" described by "New password"
      When I press Space
      Then the field "New password" shows what it holds, "demo-admin-pass", its caret 0 characters before its end
      When I press Tab until focus is on the textbox "New password again"
      And I type "demo-admin-pass"
      And I press Tab until focus is on the button "Set the admin password"
      And I press Enter
      Then the page shows a message containing "Admin password set. Restart Ladderback to turn sign-in on."
      And the field "New password" hides what it holds
      And the textbox "New password" still holds ""

    Scenario: Passwords that differ move focus to an error summary that says how to fix it
      Given a daemon with sign-in off
      When I open the address "/settings#admin-password"
      And I skip to the content
      And I press Tab until focus is on the textbox "New password"
      And I type "demo-admin-pass"
      And I press Tab until focus is on the textbox "New password again"
      And I type "demo-admin-pas"
      And I press Tab until focus is on the button "Set the admin password"
      And I press Enter
      Then focus is on the heading "There is 1 problem with the form"
      And the field "New password again" is marked invalid
      And the page has no accessibility violations

    Scenario: With sign-in on, changing the admin password in Settings by keyboard alone asks to sign in with the new one in an information alert
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      When I open the address "/settings#admin-password"
      And I skip to the content
      And I press Tab until focus is on the textbox "Current password"
      And I type "demo-admin-pass"
      And I press Tab until focus is on the textbox "New password"
      And I type "another-admin-pass"
      And I press Tab until focus is on the textbox "New password again"
      And I type "another-admin-pass"
      And I press Tab until focus is on the button "Change the admin password"
      And I press Enter
      Then the main heading reads "Sign in to Ladderback"
      And the page shows an information alert containing "Admin password changed. Sign in with the new password."
      And the page has no accessibility violations
      When I press Tab until focus is on the textbox "Admin password"
      And I type "another-admin-pass"
      And I press Enter
      Then focus is on the heading "Settings"

    Scenario: A wrong current password moves focus to an error summary that says so
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      When I open the address "/settings#admin-password"
      And I skip to the content
      And I press Tab until focus is on the textbox "Current password"
      And I type "not-the-password"
      And I press Tab until focus is on the textbox "New password"
      And I type "another-admin-pass"
      And I press Tab until focus is on the textbox "New password again"
      And I type "another-admin-pass"
      And I press Tab until focus is on the button "Change the admin password"
      And I press Enter
      Then focus is on the heading "There is 1 problem with the form"
      And the field "Current password" is marked invalid
      And the page doesn't show "not-the-password"
      And the page has no accessibility violations
