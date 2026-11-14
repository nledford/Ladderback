Feature: Sign-in
  Sign-in is optional (§10.1, decision 11). With no admin password set, sign-in
  is off: Ladderback says that anyone who can reach it can use it, on this
  machine or, when it listens on the network, from anywhere that reaches that
  address. Once `ladderback set-password` has stored the admin password's hash,
  sign-in is on: the web UI asks for the password, and the API and the events
  stream answer a session only. The request checks stay on either way.

  Rule: The admin password starts a session, and no other password does

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario: Signing in with the admin password starts a session
      When the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the admin is signed in
      And the session is live

    Scenario: A wrong password starts no session
      When the admin signs in with "not-the-password" from 192.0.2.10
      Then the sign-in is refused as a wrong password
      And no session is live

    Scenario: Signing out ends the session
      Given the admin has signed in from 192.0.2.10
      When the admin signs out
      Then the session is not live

  Rule: A session ends after 12 hours unused, and after 30 days in any case

    Background:
      Given the admin password is "demo-admin-pass"
      And the admin has signed in from 192.0.2.10

    Scenario: A session used within every 12 hours stays live
      When 11 hours and 59 minutes pass
      And the session is used
      And 11 hours and 59 minutes pass
      Then the session is live

    Scenario: A session unused for 12 hours ends
      When 12 hours pass
      Then the session is not live

    Scenario: A session used every hour still ends after 30 days
      When the session is used every hour for 30 days
      Then the session is not live

    Scenario: A session that ended stays ended once it is used again
      When 12 hours pass
      And the session is used
      And 1 minute passes
      Then the session is not live

    Scenario: Reads the page makes on its own never keep a session from ending
      When the page reads on its own every hour for 12 hours
      Then the session is not live

  Rule: Setting a new admin password ends every session started before it

    Background:
      Given the admin password is "demo-admin-pass"
      And the admin has signed in from 192.0.2.10

    Scenario: A session started before the admin password was set anew ends
      When the admin password is set to "another-admin-pass"
      Then the session is not live

    Scenario: Setting the same admin password again still ends every session
      When the admin password is set to "demo-admin-pass"
      Then the session is not live

    Scenario: The new admin password signs in
      When the admin password is set to "another-admin-pass"
      And the admin signs in with "another-admin-pass" from 192.0.2.10
      Then the admin is signed in

  Rule: A changed request carries the session's own CSRF token

    Background:
      Given the admin password is "demo-admin-pass"
      And the admin has signed in from 192.0.2.10

    Scenario: The session's own CSRF token is accepted
      Then the session accepts its own CSRF token

    Scenario: Another session's CSRF token is refused
      Given the admin has signed in again from 192.0.2.10
      Then the first session refuses the second session's CSRF token

  Rule: Each failed sign-in makes the next one from that address wait longer, and none locks the admin out

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario Outline: After <failures> failed sign-ins, the next one waits <wait>
      When the admin signs in with "not-the-password" from 192.0.2.10 <failures> times, each once the wait has passed
      And the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the sign-in is refused, to be tried again in <seconds> seconds

      Examples:
        | failures | wait      | seconds |
        | 1        | 1 second  | 1       |
        | 2        | 2 seconds | 2       |
        | 3        | 4 seconds | 4       |
        | 9        | 256 s     | 256     |
        | 10       | 5 minutes | 300     |
        | 40       | 5 minutes | 300     |

    Scenario: Once the wait has passed, the admin password signs in, however many sign-ins failed
      When the admin signs in with "not-the-password" from 192.0.2.10 40 times, each once the wait has passed
      And 5 minutes pass
      And the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the admin is signed in

    Scenario: A sign-in that waits is not checked, so a guess made during the wait never counts
      When the admin signs in with "not-the-password" from 192.0.2.10
      And the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the sign-in is refused, to be tried again in 1 seconds
      And no session is live

    Scenario: A failed sign-in from one address doesn't delay another
      When the admin signs in with "not-the-password" from 192.0.2.10
      And the admin signs in with "demo-admin-pass" from 192.0.2.20
      Then the admin is signed in

    Scenario: Addresses of one IPv6 network wait together
      When the admin signs in with "not-the-password" from 2001:db8::1
      And the admin signs in with "demo-admin-pass" from 2001:db8::2
      Then the sign-in is refused, to be tried again in 1 seconds

    Scenario: A sign-in starts the address's waits over
      When the admin signs in with "not-the-password" from 192.0.2.10 3 times, each once the wait has passed
      And 4 seconds pass
      And the admin signs in with "demo-admin-pass" from 192.0.2.10
      And the admin signs in with "not-the-password" from 192.0.2.10
      And the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the sign-in is refused, to be tried again in 1 seconds

    Scenario: An address that keeps failing keeps its wait while thousands of others fail once
      When the admin signs in with "not-the-password" from 192.0.2.10 10 times, each once the wait has passed
      And 3000 other addresses each sign in with a wrong password once
      And the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the sign-in is refused, to be tried again in 300 seconds
      And Ladderback remembers at most 1024 addresses

  @e2e
  Rule: The admin password is set at the terminal or from standard input, and only its hash is kept

    Scenario: set-password reads the password from standard input and keeps only its hash, readable by its owner alone
      When the admin password "demo-admin-pass" is set from standard input
      Then the command succeeds
      And the data folder holds the admin password's hash, readable by its owner alone
      And no file Ladderback keeps contains "demo-admin-pass"

    Scenario: set-password refuses a password shorter than 8 characters
      When the admin password "short" is set from standard input
      Then the command refuses the password
      And the data folder holds no admin password

    Scenario: set-password takes no password as an argument
      When set-password is given the password "demo-admin-pass" as an argument
      Then set-password rejects the argument "demo-admin-pass"
      And the data folder holds no admin password

  @e2e
  Rule: Beyond loopback, Ladderback listens only for a configured host name, and with sign-in off it warns that anyone who can reach it can use it

    Scenario: With sign-in on and a host name configured, Ladderback listens on every address and warns that plain HTTP is unencrypted
      Given the admin password "demo-admin-pass" is set
      When Ladderback is started with the arguments "--listen 0.0.0.0:0 --allowed-host nas.demo.test"
      Then Ladderback is listening
      And its log warns "the password crosses the network unencrypted"

    Scenario: With sign-in on and no host name configured, Ladderback refuses to listen on every address
      Given the admin password "demo-admin-pass" is set
      When Ladderback is started with the arguments "--listen 0.0.0.0:0"
      Then Ladderback refuses to start, naming "--allowed-host"

    Scenario: With sign-in off and a host name configured, Ladderback listens on every address and warns that anyone who can reach it can use it
      When Ladderback is started with the arguments "--listen 0.0.0.0:0 --allowed-host backup.example"
      Then Ladderback is listening
      And Ladderback warns "sign-in is off and Ladderback answers on the network at" and the address it listens on, before it says where it listens
      And Ladderback warns "anyone who can reach that address can run and change backups"
      And Ladderback warns "ladderback set-password"

    Scenario: With sign-in off, the warning is given even with the log turned off
      When Ladderback is started with the log turned off and the arguments "--listen 0.0.0.0:0 --allowed-host backup.example"
      Then Ladderback is listening
      And Ladderback warns "anyone who can reach that address can run and change backups"

    Scenario: With sign-in off and no host name configured, Ladderback refuses to listen on every address
      When Ladderback is started with the arguments "--listen 0.0.0.0:0"
      Then Ladderback refuses to start, naming "--allowed-host"

    Scenario: With sign-in off on the network, the configured host name reaches the API without a session
      Given Ladderback is serving on every address with sign-in off, configured for the host name "backup.example"
      And a "GET" request for "/api/v1/plans"
      And the request has the header "Host: backup.example:{port}"
      When the request is sent
      Then the request is answered with status 200

    Scenario Outline: With sign-in off on the network, a rebound name still can't reach the API with a <method> request
      Given Ladderback is serving on every address with sign-in off, configured for the host name "backup.example"
      And a "<method>" request for "/api/v1/plans"
      And the request has the header "Host: evil.example:{port}"
      And the request has the header "Origin: http://evil.example:{port}"
      And the request has the header "Content-Type: application/json"
      When the request is sent
      Then the request is refused with status 403 and the error code "host-not-allowed"

      Examples:
        | method |
        | GET    |
        | POST   |

    Scenario: With sign-in off, the log warns that anyone on this machine can use Ladderback
      When Ladderback is started with the arguments "--listen 127.0.0.1:0"
      Then Ladderback is listening
      And its log warns "sign-in is off"

    Scenario: With sign-in off and host names a proxy reaches it by, Ladderback names each one but loopback
      When Ladderback is started with the arguments "--listen 127.0.0.1:0 --allowed-host localhost --allowed-host backup.example --allowed-host nas.example"
      Then Ladderback is listening
      And Ladderback warns "anyone who can reach backup.example through the proxy can run and change backups"
      And Ladderback warns "anyone who can reach nas.example through the proxy can run and change backups"
      And Ladderback warns "To turn sign-in on, run ladderback set-password"
      And Ladderback doesn't warn "reach localhost"
      And its log warns "sign-in is off"

    Scenario: An admin password file Ladderback cannot read is a refusal to start, never sign-in off
      Given the data folder holds an admin password file that is not a password hash
      When Ladderback is started with the arguments "--listen 127.0.0.1:0"
      Then Ladderback refuses to start
      And the refusal gives the platform's message

  @e2e
  Rule: With sign-in on, the API and the events stream answer a session only

    Background:
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"

    Scenario: The web UI's page is served without a session, so it can ask for the password
      Given a "GET" request for "/"
      When the request is sent
      Then the request isn't refused by a request check

    Scenario Outline: A <method> request for <path> without a session is refused
      Given a "<method>" request for "<path>"
      And the request has the header "Content-Type: application/json"
      When the request is sent
      Then the request is refused with status 401 and the error code "sign-in-required"

      Examples:
        | method | path                   |
        | GET    | /api/v1/plans          |
        | GET    | /api/v1/operations     |
        | POST   | /api/v1/operations     |
        | GET    | /api/v1/repositories   |
        | GET    | /api/v1                |
        | GET    | /api/v1/no-such-route  |

    Scenario: The events stream's handshake without a session is refused
      Given a "GET" request for "/api/v1/events"
      And the request asks to upgrade to a WebSocket
      When the request is sent
      Then the request is refused with status 401 and the error code "sign-in-required"

    Scenario: Signing in sets a session cookie that scripts can't read and other sites never send
      When the admin signs in over HTTP with "demo-admin-pass"
      Then the sign-in sets a session cookie with "HttpOnly", "SameSite=Strict" and "Path=/"
      And the session cookie is not marked "Secure"

    Scenario: Signing in from a page served over HTTPS by a proxy marks the cookie Secure
      When the admin signs in over HTTP with "demo-admin-pass" from the page "https://localhost:{port}"
      Then the sign-in sets a session cookie with "HttpOnly", "SameSite=Strict" and "Secure"

    Scenario: No cache keeps the answer to a sign-in
      When the admin signs in over HTTP with "demo-admin-pass"
      Then the response has the header "Cache-Control" with the value "no-store"

    Scenario: No cache keeps a signed-in session's answers
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And a "GET" request for "/api/v1/session" with the session cookie
      When the request is sent
      Then the response has the header "Cache-Control" with the value "no-store"

    Scenario: With the session cookie, the API answers
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And a "GET" request for "/api/v1/plans" with the session cookie
      When the request is sent
      Then the request is answered with status 200

    Scenario: With the session cookie, the events stream's handshake is accepted
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And a "GET" request for "/api/v1/events" with the session cookie
      And the request asks to upgrade to a WebSocket
      When the request is sent
      Then the WebSocket handshake is accepted

    Scenario: A changed request with the session cookie but without its CSRF token is refused
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And a "POST" request for "/api/v1/operations" with the session cookie
      And the request has the header "Content-Type: application/json"
      When the request is sent
      Then the request is refused with status 403 and the error code "csrf-token-invalid"

    Scenario: A changed request with the session cookie and its CSRF token reaches its route
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And a "POST" request for "/api/v1/operations" with the session cookie and its CSRF token
      And the request has the header "Content-Type: application/json"
      And the request has a JSON body of 20 bytes
      When the request is sent
      Then the route answers with status 404 and the error code "plan-unknown"

    Scenario: A wrong password is refused, and the next sign-in from that address waits
      When the admin signs in over HTTP with "not-the-password"
      Then the request is refused with status 401 and the error code "sign-in-failed"
      When the admin signs in over HTTP with "demo-admin-pass"
      Then the sign-in is refused with status 429, to be tried again after 1 second

    Scenario: After signing out, the session cookie no longer answers
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And the admin has signed out over HTTP
      And a "GET" request for "/api/v1/plans" with the session cookie
      When the request is sent
      Then the request is refused with status 401 and the error code "sign-in-required"

    Scenario: Signing out without the session's CSRF token is refused
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And a "DELETE" request for "/api/v1/session" with the session cookie
      And the request has the header "Content-Type: application/json"
      When the request is sent
      Then the request is refused with status 403 and the error code "csrf-token-invalid"

    Scenario: A session started before set-password sets the admin password anew is refused after it
      Given the admin has signed in over HTTP with "demo-admin-pass"
      And the admin password "another-admin-pass" is set
      And a "GET" request for "/api/v1/plans" with the session cookie
      When the request is sent
      Then the request is refused with status 401 and the error code "sign-in-required"

    Scenario: The admin password set anew signs in at once, without a restart
      Given the admin password "another-admin-pass" is set
      When the admin signs in over HTTP with "another-admin-pass"
      Then the sign-in sets a session cookie with "HttpOnly", "SameSite=Strict" and "Path=/"

  @ui
  Rule: The sign-in form works by keyboard alone, with paste and password managers

    Scenario: The sign-in page names its one field for a password manager and has no accessibility violations
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      When I open the web UI
      Then the page is titled "Sign in – Ladderback"
      And the main heading reads "Sign in to Ladderback"
      And the textbox "Admin password" has the autocomplete token "current-password"
      And the page has no accessibility violations

    Scenario: Signing in by keyboard alone opens the page that was asked for
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      When I open the address "/repositories"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      Then focus is on the heading "Repositories"
      And the page doesn't show "demo-admin-pass"
      And the page has no accessibility violations

    Scenario: A wrong password moves focus to an error summary that says how to fix it
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      When I open the web UI
      And I press Tab until focus is on the textbox "Admin password"
      And I type "not-the-password"
      And I press Tab until focus is on the button "Sign in"
      And I press Enter
      Then focus is on the heading "There is 1 problem with the form"
      And the page shows a message containing "That is not the admin password."
      And the field "Admin password" is marked invalid
      And the page doesn't show "not-the-password"
      And the page has no accessibility violations

    Scenario: An empty password is refused before it is sent, with a suggestion
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      When I open the web UI
      And I press Tab until focus is on the button "Sign in"
      And I press Enter
      Then focus is on the heading "There is 1 problem with the form"
      And the field "Admin password" shows the error "Enter the admin password."
      And the page has no accessibility violations

    Scenario: A session that ends mid-use asks to sign in again, keeps what was typed and does what was asked
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      When I open the address "/repositories"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      Then focus is on the heading "Repositories"
      When I press Tab until focus is on the textbox "Location"
      And I type "{repo}"
      And I press Tab until focus is on the textbox "Password"
      And I type "demo-password"
      And the admin password is set anew to "another-admin-pass"
      And I press Tab until focus is on the button "Test the repository"
      And I press Enter
      Then the dialog "Sign in again" opens with focus inside it
      And the page has no accessibility violations
      When I type "another-admin-pass"
      And I press Enter
      Then the dialog "Sign in again" is closed
      And the page shows a repository ID
      And the textbox "Location" still holds "{repo}"
      And the page has no accessibility violations

    Scenario: A page whose reading waited for a sign-in that never came says so once the dialog is closed
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      When I open the address "/no-such-page"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      Then focus is on the heading "Page not found"
      When the admin password is set anew to "another-admin-pass"
      And I press Tab until focus is on the link "Go to the start page"
      And I press Enter
      Then the dialog "Sign in again" opens with focus inside it
      When I press Escape
      Then the dialog "Sign in again" is closed
      And the main heading reads "Welcome to Ladderback"
      And the page shows a message containing "Sign in to Ladderback with the admin password to go on."
      And the page has no accessibility violations

  @ui
  Rule: Two minutes before a session would end unused, the web UI warns and offers to keep it

    Scenario: The warning takes focus and Stay signed in keeps the session, by keyboard alone
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the page's clock can be moved on
      When I open the address "/repositories"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      Then focus is on the heading "Repositories"
      When I press Tab until focus is on the textbox "Name"
      And the page is left unused for "11:58:00"
      Then the dialog "You will be signed out soon" opens with focus on the button "Stay signed in"
      And the dialog "You will be signed out soon" is described as saying "will sign you out in 2 minutes"
      And the page has no accessibility violations
      When I press Enter
      Then the dialog "You will be signed out soon" is closed
      And focus is on the textbox "Name"
      When the page is left unused for "11:57:00"
      Then the dialog "You will be signed out soon" is closed

    Scenario: A warning left alone gives way to signing in again, and what was typed is kept
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the page's clock can be moved on
      When I open the address "/repositories"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      Then focus is on the heading "Repositories"
      When I press Tab until focus is on the textbox "Name"
      And I type "demo"
      And the page is left unused for "11:58:00"
      And the page is left unused for "00:02:00"
      Then the dialog "Sign in again" opens with focus inside it
      And the dialog "You will be signed out soon" is closed
      And the textbox "Name" still holds "demo"
      And the page has no accessibility violations

    Scenario: An open Operations page that hears events still warns, and asks to sign in again 12 hours after its last use
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the daemon reports a running backup of "demo-home"
      And the page's clock can be moved on
      When I open the address "/operations"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      And the table "Operations" has a row "demo-home" that shows "Running"
      And the page is left unused for "06:00:00"
      And the daemon reports that the backup of "demo-home" has succeeded with 3 files
      Then the row "demo-home" of the table "Operations" lists "3 files" in its "Result" column
      And the page marked each later reading of the history as its own
      When the page is left unused for "05:58:00"
      Then the dialog "You will be signed out soon" opens with focus on the button "Stay signed in"
      When the page is left unused for "00:02:00"
      Then the dialog "Sign in again" opens with focus inside it

  @ui
  Rule: While sign-in is on, the navigation offers Sign out

    Scenario: Signing out by keyboard alone lands on the sign-in page with focus on its heading
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      When I open the address "/repositories"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      Then focus is on the heading "Repositories"
      When I press Shift+Tab until focus is on the button "Sign out"
      And I press Enter
      Then focus is on the heading "Sign in to Ladderback"
      And the page is titled "Sign in – Ladderback"
      And the page has no accessibility violations
      When I reload the page
      Then the main heading reads "Sign in to Ladderback"

  @ui
  Rule: Signing in again in one tab leaves the other tabs working

    # The tabs share the cookie, so a sign-in in one gives every tab the new
    # session, whose CSRF token the other tabs have not read yet.
    Scenario: A change made in one tab goes through after signing out and in again in another
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      When I open the address "/repositories"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      Then focus is on the heading "Repositories"
      When I open the address "/repositories" in a second tab
      And I press Shift+Tab until focus is on the button "Sign out"
      And I press Enter
      Then focus is on the heading "Sign in to Ladderback"
      When I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      Then focus is on the heading "Repositories"
      When I go back to the first tab
      And I press Tab until focus is on the textbox "Location"
      And I type "{repo}"
      And I press Tab until focus is on the textbox "Password"
      And I type "demo-password"
      And I press Tab until focus is on the button "Test the repository"
      And I press Enter
      Then the page shows a repository ID
      And the dialog "Sign in again" is closed

  @ui
  Rule: With sign-in off, the web UI says so, on every page when Ladderback listens on the network

    Scenario: The start page warns that sign-in is off and says how to turn it on
      When I open the web UI
      Then the page shows a message containing "Sign-in is off"
      And the page shows a message containing "ladderback set-password"
      And the page has no accessibility violations
      And the page doesn't show "answers on the network"

    Scenario: Listening on the network with sign-in off, every page warns that anyone who can reach Ladderback can use it
      Given a daemon with sign-in off listening on every address for the host name "backup.example"
      When I open the web UI
      Then the page shows a message containing "Sign-in is off, and Ladderback answers on the network"
      And the page's heading is described by "Sign-in is off, and Ladderback answers on the network"
      And the page shows a message containing "ladderback set-password"
      And the page doesn't show "anyone on this machine"
      And the page has no accessibility violations
      When I open the Repositories page
      Then the page shows a message containing "Sign-in is off, and Ladderback answers on the network"
      And the page's heading is described by "Sign-in is off, and Ladderback answers on the network"
      When I open the Operations page
      Then the page shows a message containing "Sign-in is off, and Ladderback answers on the network"
      And the page's heading is described by "Sign-in is off, and Ladderback answers on the network"
