Feature: API tokens
  With sign-in on, a program other than a browser, such as MCP connect mode,
  uses the API with an API token (§10.1, decision 93). The admin creates one
  in a session, with a label and its scopes, and sees its secret once. The
  token opens only the routes its scopes name; a failed one counts as a failed
  sign-in; it lives until it is revoked or the admin password is set anew.

  Rule: An API token is created with a label and at least one scope, and its secret is shown only then

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario: A created token is listed by its label and scopes, never with its secret
      When the admin creates an API token "nightly-mcp" with the scopes "read, run"
      Then the token's secret starts with "ladderback_"
      And the API tokens are listed as "nightly-mcp" with the scopes "read, run", never used
      And no listed API token holds the token's secret

    Scenario: A token needs a label
      When the admin creates an API token "" with the scopes "read"
      Then the token is refused as having no label of 1 to 64 characters

    Scenario: A label longer than 64 characters is refused
      When the admin creates an API token with a label of 65 characters and the scopes "read"
      Then the token is refused as having no label of 1 to 64 characters

    Scenario: A token needs a scope
      When the admin creates an API token "nightly-mcp" with no scopes
      Then the token is refused as having no scope

    Scenario: At most 100 tokens are kept
      Given the admin has created 100 API tokens
      When the admin creates an API token "one-more" with the scopes "read"
      Then the token is refused as one over the 100 kept

  Rule: A token is accepted with its own secret, and a wrong one counts as a failed sign-in

    Background:
      Given the admin password is "demo-admin-pass"
      And the admin has created an API token "nightly-mcp" with the scopes "read"

    Scenario: A program presents the token and is let in with its scopes
      When a program presents the token from 192.0.2.10
      Then the program is let in with the scopes "read"

    Scenario: A wrong secret and an unknown token are refused alike
      When a program presents the token with a wrong secret from 192.0.2.10
      Then the token is refused as invalid
      When a program presents a token nobody created from 192.0.2.20
      Then the token is refused as invalid

    Scenario: After a wrong token the next one from that address waits, unchecked
      When a program presents the token with a wrong secret from 192.0.2.10
      And a program presents the token from 192.0.2.10
      Then the token is refused with a wait of 1 second

    Scenario: A wrong token makes the admin's next sign-in from that address wait
      When a program presents the token with a wrong secret from 192.0.2.10
      And the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the sign-in is refused, to be tried again in 1 second

    Scenario: A good token doesn't start the waits over
      When the admin signs in with "not-the-password" from 192.0.2.10
      And 1 second passes
      And a program presents the token from 192.0.2.10
      And the admin signs in with "not-the-password" from 192.0.2.10
      Then the sign-in is refused as a wrong password
      When the admin signs in with "demo-admin-pass" from 192.0.2.10
      Then the sign-in is refused, to be tried again in 2 seconds

  Rule: A token lives until it is revoked or the admin password is set anew

    Background:
      Given the admin password is "demo-admin-pass"
      And the admin has created an API token "nightly-mcp" with the scopes "read"

    Scenario: A revoked token is refused and no longer listed
      When the admin revokes the token
      And a program presents the token from 192.0.2.10
      Then the token is refused as invalid
      And no API token is listed

    Scenario: A new admin password revokes every token
      When the admin password is set to "demo-admin-pass"
      And a program presents the token from 192.0.2.10
      Then the token is refused as invalid
      And no API token is listed

    Scenario: A token is never too old: it is accepted a year later
      When 8760 hours pass
      And a program presents the token from 192.0.2.10
      Then the program is let in with the scopes "read"

  Rule: Settings shows when each token was last used, written at most once a minute

    Background:
      Given the admin password is "demo-admin-pass"
      And the admin has created an API token "nightly-mcp" with the scopes "read"

    Scenario: A use is recorded, and uses within the minute after it are not
      When a program presents the token from 192.0.2.10
      And 30 seconds pass
      And a program presents the token from 192.0.2.10
      Then the token was last used 30 seconds ago
      When 30 seconds pass
      And a program presents the token from 192.0.2.10
      Then the token was last used just now

  @e2e
  Rule: Over HTTP, a token opens only the routes of its scopes, and its secret is never shown again

    Background:
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"

    Scenario: A read token reads, without a session or a CSRF token
      Given the admin has created an API token over HTTP with the scopes "read" and the label "reader"
      And a "GET" request for "/api/v1/operations" with the API token
      When the request is sent
      Then the request is answered with status 200

    Scenario: A read token can't start a plan, and the refusal names the scope it needs
      Given the admin has created an API token over HTTP with the scopes "read" and the label "reader"
      And a "POST" request for "/api/v1/operations" with the API token
      And the request has the header "Content-Type: application/json"
      And the request has the body "{\"plan\":\"home\"}"
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-scope"
      And the refusal names the scope "run"

    Scenario Outline: A token never opens a route for a session only: <method> <path>
      Given the admin has created an API token over HTTP with the scopes "read, run, preview, propose" and the label "every-scope"
      And a "<method>" request for "<path>" with the API token
      And the request has the header "Content-Type: application/json"
      And the request has the body "{}"
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-scope"
      And the refusal names the scope "session"

      Examples:
        | method | path                                  |
        | POST   | /api/v1/session                       |
        | GET    | /api/v1/tokens                        |
        | POST   | /api/v1/tokens                        |
        | POST   | /api/v1/repositories                  |
        | POST   | /api/v1/repositories/_test            |
        | POST   | /api/v1/repositories/home/maintenance |
        | GET    | /api/v1/no-such-route                 |

    Scenario: A token with a wrong secret is refused, and the cookie beside it doesn't help
      Given a "GET" request for "/api/v1/operations" with a token nobody created
      And the request has the session cookie
      When the request is sent
      Then the route answers with status 401 and the error code "api-token-invalid"

    Scenario: A revoked token is refused
      Given the admin has created an API token over HTTP with the scopes "read" and the label "reader"
      And the admin has revoked that API token over HTTP
      And a "GET" request for "/api/v1/operations" with the API token
      When the request is sent
      Then the route answers with status 401 and the error code "api-token-invalid"

    Scenario: The secret is in no file Ladderback keeps, no log line and no answer after the one that created it
      Given the admin has created an API token over HTTP with the scopes "read" and the label "reader"
      And a "GET" request for "/api/v1/tokens" with the session cookie
      When the request is sent
      Then the request is answered with status 200
      And the answer doesn't hold the API token's secret
      And no file Ladderback keeps holds the API token's secret
      And the daemon's log doesn't hold the API token's secret

  @e2e
  Rule: With sign-in off, there are no API tokens

    Scenario: The token routes say that sign-in is off
      Given Ladderback is serving on loopback
      And a "GET" request for "/api/v1/tokens"
      When the request is sent
      Then the route answers with status 409 and the error code "sign-in-off"

  @ui
  Rule: In Settings, the admin creates, lists and revokes API tokens by keyboard alone, and sees a secret only once

    Scenario: The Settings link in the navigation moves focus to the page's heading
      When I open the web UI
      And I press Tab until focus is on the link "Settings"
      And I press Enter
      Then focus is on the main heading "Settings"
      And the page is titled "Settings – Ladderback"

    Scenario: Creating a token by keyboard alone shows its secret once, with a button that copies it
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the page may use the clipboard
      When I open the address "/settings#api-tokens"
      And I skip to the content
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Enter
      Then focus is on the heading "Settings"
      And the page shows the paragraph "No API tokens yet."
      When I press Tab until focus is on the textbox "Label"
      And I type "nightly-mcp"
      And I press Tab until focus is on the checkbox "Read"
      And I press Space
      And I press Tab until focus is on the checkbox "Run"
      And I press Space
      And I press Tab until focus is on the button "Create the token"
      And I press Enter
      Then the dialog "Copy the new API token" opens with focus on the button "Copy the token"
      And the dialog "Copy the new API token" is described as saying "won't show it again"
      And the dialog "Copy the new API token" shows a token that starts with "ladderback_"
      And the page has no accessibility violations
      When I press Enter
      Then the clipboard holds the token the dialog showed
      And the dialog "Copy the new API token" says "Copied."
      When I press Tab until focus is on the button "Close"
      And I press Enter
      Then the dialog "Copy the new API token" is closed
      And focus is on the button "Create the token"
      And the row "nightly-mcp" of the table "API tokens" lists "Read" in its "Scopes" column
      And the row "nightly-mcp" of the table "API tokens" lists "Run" in its "Scopes" column
      And the row "nightly-mcp" of the table "API tokens" shows "Never" in its "Last used" column
      And nothing on the page holds the token the dialog showed
      And the page has no accessibility violations

    Scenario: Escape leaves the token's dialog and takes its secret off the page
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      When I open the address "/settings#api-tokens"
      And I skip to the content
      And I press Tab until focus is on the textbox "Label"
      And I type "nightly-mcp"
      And I press Tab until focus is on the checkbox "Read"
      And I press Space
      And I press Tab until focus is on the button "Create the token"
      And I press Enter
      Then the dialog "Copy the new API token" shows a token that starts with "ladderback_"
      When I press Escape
      Then the dialog "Copy the new API token" is closed
      And focus is on the button "Create the token"
      And nothing on the page holds the token the dialog showed

    Scenario: A token's row says in words when a program last used it
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the scopes "read"
      And a program has used the API token "nightly-mcp"
      When I open the address "/settings#api-tokens"
      Then the row "nightly-mcp" of the table "API tokens" says "Read" in its "Scopes" column, not as a list
      And the row "nightly-mcp" of the table "API tokens" shows "Less than a minute ago" in its "Last used" column
      And the page has no accessibility violations

    Scenario: Revoking a token asks first, and Escape keeps it
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the scopes "read, run"
      When I open the address "/settings#api-tokens"
      And I skip to the content
      And I press Tab until focus is on the button "Revoke token nightly-mcp"
      And I press Enter
      Then the dialog "Revoke token nightly-mcp" opens with focus on the button "Cancel"
      And the dialog "Revoke token nightly-mcp" is described as saying "refused from then on"
      And the page has no accessibility violations
      When I press Escape
      Then the dialog "Revoke token nightly-mcp" is closed
      And focus is on the button "Revoke token nightly-mcp"
      When I press Enter
      And I press Tab until focus is on the button "Revoke the token"
      And I press Enter
      Then the dialog "Revoke token nightly-mcp" is closed
      And focus is on the heading "API tokens"
      And the polite region comes to contain "Token nightly-mcp revoked."
      And the page shows the paragraph "No API tokens yet."
      And the page has no accessibility violations

    Scenario: A form without a label or a scope moves focus to an error summary that names both
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      When I open the address "/settings#api-tokens"
      And I skip to the content
      And I press Tab until focus is on the button "Create the token"
      And I press Enter
      Then focus is on the heading "There are 2 problems with the form"
      And the field "Label" shows the error "so you can tell it from the others"
      And the field "Label" is marked invalid
      And the page shows a message containing "Choose at least one scope for the token"
      And the page has no accessibility violations

    Scenario: A token over the 100 kept is refused, and the error summary says so
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created 100 API tokens
      When I open the address "/settings#api-tokens"
      And I press Shift+Tab until focus is on the textbox "Label"
      And I type "one-more"
      And I press Tab until focus is on the checkbox "Read"
      And I press Space
      And I press Tab until focus is on the button "Create the token"
      And I press Enter
      Then focus is on the heading "There is 1 problem with the form"
      And the page shows a message containing "keeps at most 100 API tokens"
      And the page has no accessibility violations

    Scenario: With sign-in off, Settings says why there are no API tokens
      When I open the address "/settings#api-tokens"
      Then the page shows a message containing "Sign-in is off, so there are no API tokens."
      And the page offers no button "Create the token"
      And the page has no accessibility violations

    Scenario: Tokens Ladderback can't read are said calmly in the list's place
      Given the daemon can't read its API tokens
      When I open the address "/settings#api-tokens"
      Then the page shows a message containing "could not read or keep its API tokens"
      And the page has no accessibility violations

    Scenario: The API tokens fit a small screen
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the scopes "read, run, preview"
      And a viewport of 320 by 256 pixels
      When I open the address "/settings#api-tokens"
      Then the page shows the row "nightly-mcp"
      And the page doesn't scroll sideways
      And no text is clipped
      And the page has no accessibility violations

    Scenario Outline: The API tokens are in the language of the page
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the scopes "read"
      When I open the address "/settings?locale=<locale>#api-tokens"
      Then the page shows the row "nightly-mcp"
      And every visible text but the data of the tables and lists is transformed
      And the page has no accessibility violations

      Examples:
        | locale |
        | en-XA  |
        | ar-XB  |
