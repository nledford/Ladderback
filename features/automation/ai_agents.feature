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
