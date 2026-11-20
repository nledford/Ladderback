Feature: The recent log
  Ladderback keeps the most recent lines of its own daemon log in memory, from
  the daemon's start on, so that the web UI can show what Operations doesn't:
  the start, the scheduler, notifications (#618). They are the lines the log
  writes, masked as the log masks them (I4). The recent log is bounded: at most
  1,000 lines and 1 MiB of text, each line's message cut to 4 KiB, and the
  oldest lines go first. Lines from before the daemon's start are not in it.

  Rule: The recent log keeps the newest lines within its caps

    Scenario: The lines are listed newest first
      Given the daemon has logged "starting", "listening" and "the scheduler is idle", in that order
      Then the recent log lists "the scheduler is idle", "listening" and "starting", in that order

    Scenario: Past 1,000 lines, the oldest are let go
      Given the daemon has logged 1005 lines of 10 bytes each, numbered from 1
      Then the recent log holds 1000 lines
      And its newest line is line 1005 and its oldest line 6

    Scenario: Past 1 MiB of text, the oldest lines are let go until the rest fit
      Given the daemon has logged 300 lines of 4 KiB each, numbered from 1
      Then the recent log holds no more than 1 MiB of text
      And its newest line is line 300
      And it doesn't hold line 1

    Scenario: A message longer than 4 KiB is kept cut at a character's boundary, ending in an ellipsis
      Given the daemon has logged a message of 5000 "é"
      Then the recent log's newest message is 4 KiB at most, ends in "…" and holds only "é" before it

  @e2e
  Rule: Over HTTP, a session or a read token reads the recent log

    Background:
      Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
      And the admin has signed in over HTTP with "demo-admin-pass"

    Scenario: A session reads the recent log, newest first, back to the daemon's start
      Given a "GET" request for "/api/v1/log" with the session cookie
      When the request is sent
      Then the request is answered with status 200
      And the answer lists the recent log newest first, its oldest line saying that Ladderback is starting

    Scenario: A token with Daemon log reads the recent log, without a session
      Given the admin has created an API token over HTTP with the permissions "daemon-log: read" and the label "reader"
      And a "GET" request for "/api/v1/log" with the API token
      When the request is sent
      Then the request is answered with status 200

    Scenario: A token without Daemon log is refused, and the refusal names it
      Given the admin has created an API token over HTTP with the permissions "backups: read" and the label "reader"
      And a "GET" request for "/api/v1/log" with the API token
      When the request is sent
      Then the route answers with status 403 and the error code "api-token-permission"
      And the refusal names the permission "daemon-log"

  @e2e
  Rule: The recent log holds the lines the log writes, and no secret

    Scenario: A warning the daemon gave as it started is in the recent log, without the password it warns of
      Given the daemon's config file holds:
        """
        schema = 1

        [repositories.nas]
        repository = "rest:https://demo:CANARY-location-password@backup.example.com/nas"
        password-env = "DEMO_PASSWORD"
        """
      And Ladderback is serving on loopback with that config file
      And a "GET" request for "/api/v1/log"
      When the request is sent
      Then the request is answered with status 200
      And the answer holds a warning saying "repository nas: The address in the repository setting holds a password as text"
      And the answer doesn't hold "CANARY-location-password"
