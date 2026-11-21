@e2e
Feature: What AI agents may do without a token
  With sign-in off, no API token says who is asking, so any program on the
  network may use the API. A request `ladderback mcp` marks as an AI agent's
  is held to what AI agents may do without a token (§10.1, decision 127): the
  `[without-token]` table of `config.toml`, with the same permissions and the
  same defaults as a new token. A permission it leaves at no access refuses
  what that permission covers, naming it, so the agent can tell the person
  which one to turn on.

  Rule: With sign-in off, a request marked as an AI agent's may do only what AI agents may do without a token

    Scenario: A configuration without the table gets the defaults of a new token
      Given the daemon's config file holds:
        """
        schema = 1
        """
      And Ladderback is serving on loopback with that config file
      And a "GET" request for "/api/v1/permissions"
      And the request has the header "x-ladderback-ai-agent: 1"
      When the request is sent
      Then the request is answered with status 200
      And the answer lists the permissions "backups: read, backup-runs: start-and-cancel, previews: preview, configuration: propose"

    Scenario: A permission the table turns off is refused, and the refusal names it
      Given the daemon's config file holds:
        """
        schema = 1

        [without-token]
        backup-runs = "none"
        """
      And Ladderback is serving on loopback with that config file
      And a "POST" request for "/api/v1/operations"
      And the request has the header "x-ladderback-ai-agent: 1"
      And the request has the header "Content-Type: application/json"
      And the request has the body "{\"plan\":\"home\"}"
      When the request is sent
      Then the route answers with status 403 and the error code "without-token-permission"
      And the refusal names the permission "backup-runs"

    Scenario: The daemon's log stays closed to AI agents until the table opens it
      Given the daemon's config file holds:
        """
        schema = 1
        """
      And Ladderback is serving on loopback with that config file
      And a "GET" request for "/api/v1/log"
      And the request has the header "x-ladderback-ai-agent: 1"
      When the request is sent
      Then the route answers with status 403 and the error code "without-token-permission"
      And the refusal names the permission "daemon-log"

    Scenario: The table opens the daemon's log to AI agents
      Given the daemon's config file holds:
        """
        schema = 1

        [without-token]
        daemon-log = "read"
        """
      And Ladderback is serving on loopback with that config file
      And a "GET" request for "/api/v1/log"
      And the request has the header "x-ladderback-ai-agent: 1"
      When the request is sent
      Then the request is answered with status 200

    Scenario: A request not marked as an AI agent's may do everything, as before
      Given the daemon's config file holds:
        """
        schema = 1

        [without-token]
        backup-runs = "none"
        """
      And Ladderback is serving on loopback with that config file
      And a "GET" request for "/api/v1/permissions"
      When the request is sent
      Then the request is answered with status 200
      And the answer lists the permissions "backups: read, backup-runs: start-and-cancel, previews: preview, configuration: propose, snapshot-contents: read, daemon-log: read, repository-check: run"

  Rule: A change made elsewhere in the configuration keeps what AI agents may do without a token

    Scenario: A changed plan keeps a permission the table turns off
      Given Ladderback is serving on loopback with no repositories
      And the daemon's config file is edited by hand to hold:
        """
        schema = 1

        [repositories.demo]
        repository = "/srv/demo-repo"
        password-env = "DEMO_PASSWORD"

        [[plans]]
        id = "home"
        repository = "demo"
        sources = ["/home/demo"]
        schedule = { cron = "0 3 * * *" }

        [without-token]
        backup-runs = "none"
        """
      When the client changes the schedule of the plan "home" to "30 2 * * *"
      Then the daemon's config file reads:
        """
        schema = 1

        [repositories.demo]
        repository = "/srv/demo-repo"
        password-env = "DEMO_PASSWORD"

        [[plans]]
        id = "home"
        repository = "demo"
        sources = ["/home/demo"]
        schedule = { cron = "30 2 * * *" }

        [without-token]
        backup-runs = "none"
        """

  Rule: Only a person changes what AI agents may do without a token, with the current admin password once sign-in is on, and the log says so

    Scenario: With sign-in off, a person's change is kept and logged, and AI agents are held to it
      Given the daemon's config file holds:
        """
        schema = 1
        """
      And Ladderback is serving on loopback with that config file
      When a person allows AI agents without a token "backups: read, daemon-log: read"
      Then the change is kept, and AI agents without a token may do "backups: read, daemon-log: read"
      And its log says "AI agents without a token now have backups: read, daemon-log: read"
      And an AI agent without a token may read the daemon's log

    Scenario: With sign-in off, an AI agent can't change what it may do
      Given Ladderback is serving on loopback
      And a "PUT" request for "/api/v1/settings/without-token"
      And the request has the header "x-ladderback-ai-agent: 1"
      And the request has the header "Content-Type: application/json"
      And the request has the body "{\"permissions\":{\"daemon-log\":\"read\"}}"
      When the request is sent
      Then the route answers with status 403 and the error code "without-token-permission"
      And the refusal names the permission "session"
      Given a "GET" request for "/api/v1/settings/without-token"
      When the request is sent
      Then the answer lists the permissions "backups: read, backup-runs: start-and-cancel, previews: preview, configuration: propose"

    Scenario: With sign-in on, a change in the admin's session with the current admin password is kept and logged
      Given the daemon's config file holds:
        """
        schema = 1
        """
      And Ladderback is serving on loopback with that config file, sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"
      And a "PUT" request for "/api/v1/settings/without-token" with the session cookie and its CSRF token
      And the request has the header "Content-Type: application/json"
      And the request has the body "{\"permissions\":{\"backups\":\"read\"},\"current-password\":\"demo-admin-pass\"}"
      When the request is sent
      Then the request is answered with status 200
      And the answer lists the permissions "backups: read"
      And its log says "AI agents without a token now have backups: read"
      And the daemon's log doesn't hold "demo-admin-pass"

    Scenario: With sign-in on, a wrong admin password changes nothing
      Given the daemon's config file holds:
        """
        schema = 1
        """
      And Ladderback is serving on loopback with that config file, sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"
      And a "PUT" request for "/api/v1/settings/without-token" with the session cookie and its CSRF token
      And the request has the header "Content-Type: application/json"
      And the request has the body "{\"permissions\":{\"backups\":\"read\"},\"current-password\":\"not-the-password\"}"
      When the request is sent
      Then the route answers with status 403 and the error code "current-password-wrong"
      And the daemon's log doesn't hold "not-the-password"
      Given a "GET" request for "/api/v1/settings/without-token" with the session cookie
      When the request is sent
      Then the answer lists the permissions "backups: read, backup-runs: start-and-cancel, previews: preview, configuration: propose"

    Scenario: An API token can't change what AI agents may do without one, even with the admin password
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"
      And the admin has created an API token over HTTP with the permissions "configuration: propose" and the label "assistant"
      And a "PUT" request for "/api/v1/settings/without-token" with the API token
      And the request has the header "Content-Type: application/json"
      And the request has the body "{\"permissions\":{\"backups\":\"read\"},\"current-password\":\"demo-admin-pass\"}"
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-permission"
      And the refusal names the permission "session"
      Given a "GET" request for "/api/v1/settings/without-token" with the session cookie
      When the request is sent
      Then the answer lists the permissions "backups: read, backup-runs: start-and-cancel, previews: preview, configuration: propose"

  Rule: `ladderback mcp` without a token offers only the tools what AI agents may do without a token opens, and its requests say they are an AI agent's

    Scenario: Connected with sign-in off, the server lists only the tools the table opens, and the daemon refuses the rest
      Given the daemon's config file holds:
        """
        schema = 1

        [without-token]
        backup-runs = "none"
        """
      And Ladderback is serving on loopback with that config file
      And an MCP server connected to it
      When the MCP client lists the tools
      Then the tools listed are "status, list_plans, list_repositories, list_snapshots, list_operations, get_operation, wait_operation, preview_forget, preview_import, export_profile, propose_setup, get_proposal, withdraw_proposal, dry_run_plan"
      When the MCP client calls the tool "run_plan" with the arguments '{"plan": "home"}'
      Then the result is the error "without-token-permission"
      And the error names the permission "backup-runs"

    Scenario: A session of its own lists only the tools the table opens, and refuses the rest
      Given the daemon's config file holds:
        """
        schema = 1

        [without-token]
        previews = "none"
        """
      And an MCP server running headless on that configuration
      When the MCP client lists the tools
      Then the tools listed are "status, list_plans, list_repositories, list_snapshots, list_operations, get_operation, run_plan, wait_operation, cancel_operation, test_repository, export_profile"
      When the MCP client calls the tool "preview_forget" with the arguments '{"repository": "demo"}'
      Then the result is the error "without-token-permission"
      And the error names the permission "previews"
