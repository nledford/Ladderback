Feature: How long signing in lasts
  Every self-hosted setup has its own threat model, so the admin chooses how
  long signing in lasts (§10.1, decision 70). "Keep me signed in on this
  device" on the sign-in form lifts a session's idle limit of 12 hours unused.
  "Stay signed in for" in Settings sets the session length: how long every
  session lasts after its sign-in, open ones included. Sessions are kept in the
  store by their token's digest, never the token, so they outlive a restart.

  Rule: Keep me signed in on this device lifts the idle limit of 12 hours unused

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario: A session signed in with Keep me signed in is still live after 13 hours unused
      Given the admin has signed in from 192.0.2.10, keeping signed in on this device
      When 13 hours pass
      Then the session is live
      And the session is said to have no idle limit

    Scenario: A session signed in without Keep me signed in ends after 12 hours unused, even under Until I sign out
      Given Stay signed in for is "Until I sign out"
      And the admin has signed in from 192.0.2.10
      When 11 hours and 59 minutes pass
      Then the session is live
      And the session is said to end after 12 hours unused
      When 1 minute passes
      Then the session is not live

  Rule: Stay signed in for sets how long every session lasts after its sign-in

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario: A session lasts 30 days unless the admin chooses otherwise
      Given the admin has signed in from 192.0.2.10, keeping signed in on this device
      When 29 days, 23 hours and 59 minutes pass
      Then the session is live
      When 1 minute passes
      Then the session is not live

    Scenario Outline: With Stay signed in for set to <option>, a session ends <days> days after its sign-in
      Given Stay signed in for is "<option>"
      And the admin has signed in from 192.0.2.10, keeping signed in on this device
      When <before> days, 23 hours and 59 minutes pass
      Then the session is live
      When 1 minute passes
      Then the session is not live

      Examples:
        | option   | days | before |
        | 7 days   | 7    | 6      |
        | 30 days  | 30   | 29     |
        | 90 days  | 90   | 89     |
        | 1 year   | 365  | 364    |

    Scenario: Until I close the browser ends a session 12 hours after its last use, Keep me signed in or not
      Given Stay signed in for is "Until I close the browser"
      And the admin has signed in from 192.0.2.10, keeping signed in on this device
      Then the session is said to end after 12 hours unused
      When 12 hours pass
      Then the session is not live

    Scenario: Until I close the browser sets no end after sign-in to a session used within every 12 hours
      Given Stay signed in for is "Until I close the browser"
      And the admin has signed in from 192.0.2.10
      When the session is used every hour for 60 days
      Then the session is live

    Scenario: Until I close the browser offers no Keep me signed in on the sign-in form
      When Stay signed in for is set to "Until I close the browser"
      Then the sign-in form doesn't offer Keep me signed in on this device

    Scenario: Until I sign out keeps a session that is used for 2 years
      Given Stay signed in for is "Until I sign out"
      And the admin has signed in from 192.0.2.10, keeping signed in on this device
      When the session is used every day for 730 days
      Then the session is live

    Scenario: Until I sign out still ends a session unused for 12 hours without Keep me signed in
      Given Stay signed in for is "Until I sign out"
      And the admin has signed in from 192.0.2.10
      When 12 hours pass
      Then the session is not live

    Scenario: Until I sign out still ends every session when the admin password is set anew
      Given Stay signed in for is "Until I sign out"
      And the admin has signed in from 192.0.2.10, keeping signed in on this device
      When the admin password is set to "another-admin-pass"
      Then the session is not live

  Rule: Stay signed in for applies to sessions already open, measured from each one's sign-in

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario: Shortening Stay signed in for ends a session older than the new length on its next request
      Given the admin has signed in from 192.0.2.10, keeping signed in on this device
      When 8 days pass
      And Stay signed in for is set to "7 days"
      Then the session is not live

    Scenario: Shortening Stay signed in for keeps a session younger than the new length
      Given the admin has signed in from 192.0.2.10, keeping signed in on this device
      When 6 days pass
      And Stay signed in for is set to "7 days"
      Then the session is live

    Scenario: Lengthening Stay signed in for extends the sessions already open
      Given Stay signed in for is "7 days"
      And the admin has signed in from 192.0.2.10, keeping signed in on this device
      When 6 days pass
      And Stay signed in for is set to "30 days"
      And 2 days pass
      Then the session is live

  Rule: A person's use renews the session's cookie, at most once a day, and the cookie never outlives its session

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario: A sign-in's cookie lasts as long as the session
      Given Stay signed in for is "90 days"
      When the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the sign-in's cookie lasts 90 days

    Scenario: Until I close the browser gives a cookie that lasts until the browser closes
      Given Stay signed in for is "Until I close the browser"
      When the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the sign-in's cookie lasts until the browser closes

    Scenario: A 1-year session's cookie is renewed to last as long as the session has left
      Given Stay signed in for is "1 year"
      And the admin has signed in from 192.0.2.10, keeping signed in on this device
      When the session is used every day for 200 days
      Then the last use renewed the session's cookie to last 165 days

    Scenario: An Until I sign out session's cookie is renewed past the 400 days a browser keeps a cookie
      Given Stay signed in for is "Until I sign out"
      And the admin has signed in from 192.0.2.10, keeping signed in on this device
      Then the sign-in's cookie lasts 400 days
      When the session is used every day for 401 days
      Then the last use renewed the session's cookie to last 400 days
      And the session is live

    Scenario: A cookie is renewed at most once a day
      Given the admin has signed in from 192.0.2.10, keeping signed in on this device
      When 1 day passes
      And the session is used
      Then the use renewed the session's cookie to last 29 days
      When 1 hour passes
      And the session is used
      Then the use didn't renew the session's cookie

    Scenario: Shortening Stay signed in for renews the cookie at the next use so it ends with its session
      Given the admin has signed in from 192.0.2.10, keeping signed in on this device
      And the session is used
      When 1 hour passes
      And Stay signed in for is set to "7 days"
      And the session is used
      Then the use renewed the session's cookie to last 6 days and 23 hours

    Scenario: Reads the page makes on its own never renew the cookie
      Given the admin has signed in from 192.0.2.10, keeping signed in on this device
      When 2 days pass
      And the page reads on its own
      Then the read didn't renew the session's cookie

  Rule: Sign out everywhere ends every browser's session, and API tokens keep working

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario: Sign out everywhere ends another browser's session and this one's, and an API token still works
      Given the admin has signed in from 192.0.2.10
      And the admin has signed in again from 192.0.2.20
      And the admin has created an API token "nightly-mcp" with the scopes "read"
      When the admin signs out everywhere
      Then the session is not live
      And the second session is not live
      And Ladderback keeps no session for after a restart
      When a program presents the token from 192.0.2.30
      Then the program is let in with the scopes "read"

  Rule: Sessions and Stay signed in for outlive a restart of Ladderback, and a session that ended stays ended

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario: A session signed in before a restart is still live after it
      Given the admin has signed in from 192.0.2.10
      When Ladderback is restarted
      Then the session is live

    Scenario: A session signed out before a restart stays ended after it
      Given the admin has signed in from 192.0.2.10
      When the admin signs out
      And Ladderback is restarted
      Then the session is not live

    Scenario: A session's last use before a restart still counts after it
      Given the admin has signed in from 192.0.2.10
      When 11 hours pass
      And the session is used
      And Ladderback is restarted
      And 11 hours pass
      Then the session is live

    Scenario: A session that ended unused while Ladderback was stopped stays ended
      Given the admin has signed in from 192.0.2.10
      When 12 hours pass
      And Ladderback is restarted
      Then the session is not live

    Scenario: An admin password set anew while Ladderback was stopped ends the sessions from before
      Given the admin has signed in from 192.0.2.10
      When the admin password is set to "another-admin-pass"
      And Ladderback is restarted
      Then the session is not live

    Scenario: A new admin password has Ladderback forget every session it kept for after a restart
      Given the admin has signed in from 192.0.2.10
      And the admin has signed in again from 192.0.2.10
      When the admin password is set to "another-admin-pass"
      Then Ladderback keeps no session for after a restart

    Scenario: An admin password changed in Settings has Ladderback forget every session it kept for after a restart
      Given the admin has signed in from 192.0.2.10
      When the admin changes the admin password from "demo-admin-pass" to "another-admin-pass" from 192.0.2.10
      Then Ladderback keeps no session for after a restart

    Scenario: Keep me signed in outlives a restart
      Given the admin has signed in from 192.0.2.10, keeping signed in on this device
      When Ladderback is restarted
      And 13 hours pass
      Then the session is live

    Scenario: Stay signed in for outlives a restart
      Given Stay signed in for is "7 days"
      And the admin has signed in from 192.0.2.10, keeping signed in on this device
      When Ladderback is restarted
      And 7 days pass
      Then the session is not live

    Scenario: A session's CSRF token from before a restart is replaced, and the page can read the new one
      Given the admin has signed in from 192.0.2.10
      When Ladderback is restarted
      Then the session refuses the CSRF token it had before the restart
      And the CSRF token the page reads from the session now is accepted

  @e2e
  Rule: A session signed in before Ladderback restarts still answers after it, and only its token's digest is kept

    Background:
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"

    Scenario: A session signed in before a restart still reaches the API after it, and no file Ladderback keeps holds its token
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And Ladderback has been restarted
      And a "GET" request for "/api/v1/plans" with the session cookie
      When the request is sent
      Then the request is answered with status 200
      And no file Ladderback keeps holds the session cookie's token

    Scenario: set-password while Ladderback is stopped has it forget every session it kept for after a restart
      Given the admin has signed in over HTTP with "demo-admin-pass"
      When Ladderback is stopped
      And the admin password "another-admin-pass" is set from standard input
      Then the command succeeds
      And Ladderback keeps no session for after a restart

    Scenario: Of two browsers' sessions, the one signed out before a restart is still refused after it, and the other still answers
      Given the admin has signed in over HTTP with "demo-admin-pass" in a second browser
      And the admin has signed in over HTTP with "demo-admin-pass"
      And the admin has signed out over HTTP
      And Ladderback has been restarted
      And a "GET" request for "/api/v1/plans" with the session cookie
      When the request is sent
      Then the request is refused with status 401 and the error code "sign-in-required"
      When a "GET" request for "/api/v1/plans" is sent with the second browser's session cookie
      Then the request is answered with status 200

    Scenario: Until I close the browser sets a session cookie with no Max-Age, which the browser drops when it closes
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And the admin has set Stay signed in for to "Until I close the browser" over HTTP
      When the admin signs in over HTTP with "demo-admin-pass"
      Then the sign-in sets a session cookie with "HttpOnly", "SameSite=Strict" and "Path=/"
      And the session cookie has neither "Max-Age" nor "Expires"

    Scenario: Stay signed in for outlives a restart
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And the admin has set Stay signed in for to "7 days" over HTTP
      And Ladderback has been restarted
      When the admin signs in over HTTP with "demo-admin-pass"
      Then the session cookie lasts 7 days

    Scenario: An API token can't change Stay signed in for
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And the admin has created an API token over HTTP with the scopes "read, run, preview, propose" and the label "demo-token"
      And a "PUT" request for "/api/v1/settings/sign-in" with the API token
      And the request has the header "Content-Type: application/json"
      And the request has the body "{\"session-length\":\"7-days\"}"
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-scope"
      And the refusal names the scope "session"
      Given a "GET" request for "/api/v1/settings/sign-in" with the session cookie
      When the request is sent
      Then the request is answered with status 200
      And the answer doesn't hold "7-days"

  @e2e
  Rule: With sign-in off, Stay signed in for can't be set

    Scenario: Setting Stay signed in for with sign-in off is refused
      Given Ladderback is serving on loopback with sign-in off
      And a "PUT" request for "/api/v1/settings/sign-in"
      And the request has the header "Content-Type: application/json"
      When the request is sent
      Then the request is refused with status 409 and the error code "sign-in-off"

  @ui
  Rule: Keep me signed in and Stay signed in for work by keyboard alone, and the pages stay free of accessibility violations

    Scenario: Signing in with Keep me signed in by keyboard alone never warns of an idle end
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the page's clock can be moved on
      When I open the address "/repositories"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Tab until focus is on the checkbox "Keep me signed in on this device"
      Then the checkbox "Keep me signed in on this device" is not checked
      And the page has no accessibility violations
      When I press Space
      And I press Enter
      Then focus is on the heading "Repositories"
      When the page is left unused for "11:58:00"
      Then the dialog "You will be signed out soon" is closed
      When the page is left unused for "01:02:00"
      Then the dialog "Sign in again" is closed

    Scenario: Choosing Stay signed in for by keyboard alone keeps the choice
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      When I open the address "/settings"
      And I press Tab until focus is on the radio "30 days"
      Then the radio "30 days" of the group "Stay signed in for" is checked
      And the page has no accessibility violations
      When I press the down arrow until focus is on the radio "Until I sign out"
      And I press Tab until focus is on the button "Save the session length"
      And I press Enter
      Then the page shows a message containing "Saved."
      When I reload the page
      Then the radio "Until I sign out" of the group "Stay signed in for" is checked
      And the page has no accessibility violations

    Scenario: With Until I close the browser, the sign-in form offers no Keep me signed in
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has set Stay signed in for to "Until I close the browser"
      When I open the address "/settings"
      And I press Shift+Tab until focus is on the button "Sign out"
      And I press Enter
      Then focus is on the heading "Sign in to Ladderback"
      And the page offers no checkbox "Keep me signed in on this device"
      And the page has no accessibility violations

    Scenario: Sign out everywhere by keyboard alone lands on the sign-in page
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      When I open the address "/settings"
      And I press Tab until focus is on the button "Sign out everywhere"
      And I press Enter
      Then the dialog "Sign out everywhere?" opens with focus on the button "Cancel"
      And the page has no accessibility violations
      When I press Tab until focus is on the button "Sign out every browser"
      And I press Enter
      Then focus is on the heading "Sign in to Ladderback"
      And the page has no accessibility violations
      When I reload the page
      Then the main heading reads "Sign in to Ladderback"

    Scenario: With sign-in off, Settings says Stay signed in for applies once sign-in is on
      When I open the address "/settings"
      Then the page shows a message containing "applies once sign-in is on"
      And the page offers no button "Sign out everywhere"
      And the page has no accessibility violations
