Feature: API tokens
  With sign-in on, a program other than a browser, such as MCP connect mode,
  uses the API with an API token (§10.1, decisions 93 and 127). The admin
  creates one in a session, with a label and its permissions, and sees its
  secret once. The token opens only the routes its permissions allow; a
  failed one counts as a failed sign-in; it lives until it is revoked or the
  admin password is set anew.

  Rule: An API token is created with a label and at least one permission, and its secret is shown only then

    Background:
      Given the admin password is "demo-admin-pass"

    Scenario: A created token is listed by its label and permissions, never with its secret
      When the admin creates an API token "nightly-mcp" with the permissions "backup-runs: start-and-cancel, daemon-log: read"
      Then the token's secret starts with "ladderback_"
      And the API tokens are listed as "nightly-mcp" with the permissions "backup-runs: start-and-cancel, daemon-log: read", never used
      And no listed API token holds the token's secret

    Scenario: A token needs a label
      When the admin creates an API token "" with the permissions "backups: read"
      Then the token is refused as having no label of 1 to 64 characters

    Scenario: A label longer than 64 characters is refused
      When the admin creates an API token with a label of 65 characters and the permissions "backups: read"
      Then the token is refused as having no label of 1 to 64 characters

    Scenario: A token needs a permission
      When the admin creates an API token "nightly-mcp" with no permission
      Then the token is refused as having no permission

    Scenario: At most 100 tokens are kept
      Given the admin has created 100 API tokens
      When the admin creates an API token "one-more" with the permissions "backups: read"
      Then the token is refused as one over the 100 kept

  Rule: A token is accepted with its own secret, and a wrong one counts as a failed sign-in

    Background:
      Given the admin password is "demo-admin-pass"
      And the admin has created an API token "nightly-mcp" with the permissions "backups: read"

    Scenario: A program presents the token and is let in with its permissions
      When a program presents the token from 192.0.2.10
      Then the program is let in with the permissions "backups: read"

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
      And the admin has created an API token "nightly-mcp" with the permissions "backups: read"

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
      Then the program is let in with the permissions "backups: read"

  Rule: Settings shows when each token was last used, written at most once a minute

    Background:
      Given the admin password is "demo-admin-pass"
      And the admin has created an API token "nightly-mcp" with the permissions "backups: read"

    Scenario: A use is recorded, and uses within the minute after it are not
      When a program presents the token from 192.0.2.10
      And 30 seconds pass
      And a program presents the token from 192.0.2.10
      Then the token was last used 30 seconds ago
      When 30 seconds pass
      And a program presents the token from 192.0.2.10
      Then the token was last used just now

  @e2e
  Rule: Over HTTP, a token opens only the routes of its permissions, and its secret is never shown again

    Background:
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"

    Scenario: A token with Backups reads, without a session or a CSRF token
      Given the admin has created an API token over HTTP with the permissions "backups: read" and the label "reader"
      And a "GET" request for "/api/v1/operations" with the API token
      When the request is sent
      Then the request is answered with status 200

    Scenario: A token without Backup runs can't start a plan, and the refusal names the permission it needs
      Given the admin has created an API token over HTTP with the permissions "backups: read" and the label "reader"
      And a "POST" request for "/api/v1/operations" with the API token
      And the request has the header "Content-Type: application/json"
      And the request has the body "{\"plan\":\"home\"}"
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-permission"
      And the refusal names the permission "backup-runs"

    Scenario: A token with only Backup runs reads nothing
      Given the admin has created an API token over HTTP with the permissions "backup-runs: start-and-cancel" and the label "scheduler"
      And a "GET" request for "/api/v1/operations" with the API token
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-permission"
      And the refusal names the permission "backups"

    Scenario: A token without Snapshot contents can't see the names inside a snapshot
      Given the admin has created an API token over HTTP with the permissions "backups: read" and the label "reader"
      And a "GET" request for "/api/v1/repositories/nas/snapshots/0123abcd/folder" with the API token
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-permission"
      And the refusal names the permission "snapshot-contents"

    Scenario: A token asks what it may do
      Given the admin has created an API token over HTTP with the permissions "backups: read, daemon-log: read" and the label "reader"
      And a "GET" request for "/api/v1/permissions" with the API token
      When the request is sent
      Then the request is answered with status 200
      And the answer lists the permissions "backups: read, daemon-log: read"

    Scenario Outline: A token never opens a route for a session only: <method> <path>
      Given the admin has created an API token over HTTP with the permissions "backups: read, backup-runs: start-and-cancel, previews: preview, configuration: propose, snapshot-contents: read, daemon-log: read, repository-check: run" and the label "every-permission"
      And a "<method>" request for "<path>" with the API token
      And the request has the header "Content-Type: application/json"
      And the request has the body "{}"
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-permission"
      And the refusal names the permission "session"

      Examples:
        | method | path                                    |
        | POST   | /api/v1/session                         |
        | GET    | /api/v1/tokens                          |
        | POST   | /api/v1/tokens                          |
        | PUT    | /api/v1/tokens/abcdefghijkl/permissions |
        | POST   | /api/v1/repositories                    |
        | POST   | /api/v1/repositories/_test              |
        | POST   | /api/v1/repositories/home/maintenance   |
        | GET    | /api/v1/no-such-route                   |

    Scenario: A token with a wrong secret is refused, and the cookie beside it doesn't help
      Given a "GET" request for "/api/v1/operations" with a token nobody created
      And the request has the session cookie
      When the request is sent
      Then the route answers with status 401 and the error code "api-token-invalid"

    Scenario: A revoked token is refused
      Given the admin has created an API token over HTTP with the permissions "backups: read" and the label "reader"
      And the admin has revoked that API token over HTTP
      And a "GET" request for "/api/v1/operations" with the API token
      When the request is sent
      Then the route answers with status 401 and the error code "api-token-invalid"
      And its log says "revoked"

    Scenario: The secret is in no file Ladderback keeps, no log line and no answer after the one that created it
      Given the admin has created an API token over HTTP with the permissions "backups: read" and the label "reader"
      And a "GET" request for "/api/v1/tokens" with the session cookie
      When the request is sent
      Then the request is answered with status 200
      And the answer doesn't hold the API token's secret
      And no file Ladderback keeps holds the API token's secret
      And the daemon's log doesn't hold the API token's secret
      And its log says "(reader) created, with backups: read"

  @e2e
  Rule: Only a person signed in with the current admin password changes a token's permissions, and the log says so

    Background:
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"
      And the admin has created an API token over HTTP with the permissions "backups: read" and the label "reader"

    Scenario: With the current admin password, the change is kept, answered and logged
      Given a request in the admin's session that changes the API token's permissions to "backups: read, daemon-log: read" with the admin password "demo-admin-pass"
      When the request is sent
      Then the request is answered with status 200
      And the answer lists the permissions "backups: read, daemon-log: read"
      And its log says "now has backups: read, daemon-log: read"
      And the daemon's log doesn't hold "demo-admin-pass"
      Given a "GET" request for "/api/v1/log" with the API token
      When the request is sent
      Then the request is answered with status 200

    Scenario: A wrong admin password changes nothing
      Given a request in the admin's session that changes the API token's permissions to "backups: read, daemon-log: read" with the admin password "not-the-password"
      When the request is sent
      Then the route answers with status 403 and the error code "current-password-wrong"
      And the daemon's log doesn't hold "not-the-password"
      And the admin's session lists the API token with the permissions "backups: read"

    Scenario: A token can't change its own permissions, even with the admin password
      Given a request with the API token that changes its own permissions to "backups: read, daemon-log: read" with the admin password "demo-admin-pass"
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-permission"
      And the refusal names the permission "session"

  @e2e
  Rule: With sign-in off, there are no API tokens

    Scenario: The token routes say that sign-in is off
      Given Ladderback is serving on loopback
      And a "GET" request for "/api/v1/tokens"
      When the request is sent
      Then the route answers with status 409 and the error code "sign-in-off"

  @ui
  Rule: In Settings, the admin creates, lists, changes and revokes API tokens by keyboard alone, and sees a secret only once

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
      And I press Tab until focus is in the group "Snapshot contents"
      And I press the down arrow until focus is on the choice "Read" of the group "Snapshot contents"
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
      And the row "nightly-mcp" of the table "API tokens" lists "Backups: Read" in its "Permissions" column
      And the row "nightly-mcp" of the table "API tokens" lists "Backup runs: Start and cancel" in its "Permissions" column
      And the row "nightly-mcp" of the table "API tokens" lists "Snapshot contents: Read" in its "Permissions" column
      And the row "nightly-mcp" of the table "API tokens" shows "Never" in its "Last used" column
      And nothing on the page holds the token the dialog showed
      And the page has no accessibility violations

    # A new token starts from the safe defaults (decision 127): it sees and
    # starts backups, previews and proposes; what can show a file's name or a
    # log line to whatever model an AI agent uses is off until chosen.
    Scenario: The form offers each permission as a group of its own, starting at the defaults of a new token
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      When I open the address "/settings#api-tokens"
      Then the group "Backups" has "Read" chosen
      And the group "Backup runs" has "Start and cancel" chosen
      And the group "Previews" has "Preview" chosen
      And the group "Configuration" has "Propose" chosen
      And the group "Snapshot contents" has "No access" chosen
      And the group "Daemon log" has "No access" chosen
      And the group "Repository check" has "No access" chosen
      And the group "Backups" is described as saying "Not the files inside a snapshot, and not the daemon's log"
      And the page has no accessibility violations

    Scenario: Escape leaves the token's dialog and takes its secret off the page
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      When I open the address "/settings#api-tokens"
      And I skip to the content
      And I press Tab until focus is on the textbox "Label"
      And I type "nightly-mcp"
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
      And the admin has created the API token "nightly-mcp" with the permissions "backups: read"
      And a program has used the API token "nightly-mcp"
      When I open the address "/settings#api-tokens"
      Then the row "nightly-mcp" of the table "API tokens" says "Backups: Read" in its "Permissions" column, not as a list
      And the row "nightly-mcp" of the table "API tokens" shows "Less than a minute ago" in its "Last used" column
      And the page has no accessibility violations

    # 1a's Scopes column showed a token with only Daemon log as "Read" and one
    # with only Repository check as nothing.
    Scenario: A token's row names each permission it holds, whichever they are
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "log-reader" with the permissions "daemon-log: read"
      And the admin has created the API token "checker" with the permissions "repository-check: run"
      When I open the address "/settings#api-tokens"
      Then the row "log-reader" of the table "API tokens" says "Daemon log: Read" in its "Permissions" column, not as a list
      And the row "checker" of the table "API tokens" says "Repository check: Run" in its "Permissions" column, not as a list

    Scenario: Changing a token's permissions by keyboard alone takes the admin password
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the permissions "backups: read"
      When I open the address "/settings#api-tokens"
      And I skip to the content
      And I press Tab until focus is on the button "Change permissions of token nightly-mcp"
      And I press Enter
      Then the dialog "Permissions of token nightly-mcp" opens with focus on the button "Cancel"
      And the group "Backups" has "Read" chosen
      And the group "Daemon log" has "No access" chosen
      And the page has no accessibility violations
      When I press Tab until focus is in the group "Daemon log"
      And I press the down arrow until focus is on the choice "Read" of the group "Daemon log"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "demo-admin-pass"
      And I press Tab until focus is on the button "Save the permissions"
      And I press Enter
      Then the dialog "Permissions of token nightly-mcp" is closed
      And focus is on the button "Change permissions of token nightly-mcp"
      And the toast reads "Token nightly-mcp has its new permissions." after its icon "Success"
      And the row "nightly-mcp" of the table "API tokens" lists "Daemon log: Read" in its "Permissions" column
      And the page has no accessibility violations

    Scenario: A wrong admin password keeps a token's permissions, and the dialog says why
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the permissions "backups: read"
      When I open the address "/settings#api-tokens"
      And I skip to the content
      And I press Tab until focus is on the button "Change permissions of token nightly-mcp"
      And I press Enter
      And I press Tab until focus is in the group "Daemon log"
      And I press the down arrow until focus is on the choice "Read" of the group "Daemon log"
      And I press Tab until focus is on the textbox "Admin password"
      And I type "not-the-password"
      And I press Tab until focus is on the button "Save the permissions"
      And I press Enter
      Then the field "Admin password" shows the error "That is not the current admin password"
      And the page has no accessibility violations
      When I press Escape
      Then the row "nightly-mcp" of the table "API tokens" says "Backups: Read" in its "Permissions" column, not as a list

    Scenario: Revoking a token asks first, and Escape keeps it
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the permissions "backups: read, backup-runs: start-and-cancel"
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

    Scenario: A revoked token is answered in a toast, said once, and focus goes to the section's heading as its row goes
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the permissions "backups: read, backup-runs: start-and-cancel"
      When I open the address "/settings#api-tokens"
      And I skip to the content
      And I press Tab until focus is on the button "Revoke token nightly-mcp"
      And I press Enter
      And I press Tab until focus is on the button "Revoke the token"
      And I press Enter
      Then the toast reads "Token nightly-mcp revoked." after its icon "Success"
      And the toast is no live region, nor holds one
      And focus is on the heading "API tokens"
      And the polite region reads "Token nightly-mcp revoked."
      And the polite region keeps reading "Token nightly-mcp revoked."
      And the page has no accessibility violations

    Scenario: A form without a label or a permission moves focus to an error summary that names both
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      When I open the address "/settings#api-tokens"
      And I skip to the content
      And I press Tab until focus is in the group "Backups"
      And I press the down arrow until focus is on the choice "No access" of the group "Backups"
      And I press Tab until focus is in the group "Backup runs"
      And I press the down arrow until focus is on the choice "No access" of the group "Backup runs"
      And I press Tab until focus is in the group "Previews"
      And I press the down arrow until focus is on the choice "No access" of the group "Previews"
      And I press Tab until focus is in the group "Configuration"
      And I press the down arrow until focus is on the choice "No access" of the group "Configuration"
      And I press Tab until focus is on the button "Create the token"
      And I press Enter
      Then focus is on the heading "There are 2 problems with the form"
      And the field "Label" shows the error "so you can tell it from the others"
      And the field "Label" is marked invalid
      And the page shows a message containing "Choose at least one permission for the token"
      And the page has no accessibility violations

    Scenario: A token over the 100 kept is refused, and the error summary says so
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created 100 API tokens
      When I open the address "/settings#api-tokens"
      And I press Shift+Tab until focus is on the textbox "Label"
      And I type "one-more"
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

    Scenario: Tokens Ladderback can't read are said calmly in an error alert in the list's place
      Given the daemon can't read its API tokens
      When I open the address "/settings#api-tokens"
      Then the page shows an error alert containing "could not read or keep its API tokens"
      And the page has no accessibility violations

    Scenario: The API tokens fit a small screen
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the permissions "backups: read, backup-runs: start-and-cancel, previews: preview"
      And a viewport of 320 by 256 pixels
      When I open the address "/settings#api-tokens"
      Then the page shows the row "nightly-mcp"
      And the page doesn't scroll sideways
      And no text is clipped
      And the page has no accessibility violations

    Scenario Outline: The API tokens are in the language of the page
      Given a daemon with sign-in on and the admin password "demo-admin-pass"
      And the admin is signed in with "demo-admin-pass"
      And the admin has created the API token "nightly-mcp" with the permissions "backups: read"
      When I open the address "/settings?locale=<locale>#api-tokens"
      Then the page shows the row "nightly-mcp"
      And every visible text but the data of the tables and lists is transformed
      And the page has no accessibility violations

      Examples:
        | locale |
        | en-XA  |
        | ar-XB  |
