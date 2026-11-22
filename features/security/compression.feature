@e2e
Feature: No answer of the API is compressed
  The daemon sends the web UI's files as copies a release build compressed,
  which hold nothing secret, and never compresses an answer of the API. A
  compressed answer's length can tell someone who watches the traffic and
  can make the browser send requests, guess by guess, a secret the answer
  holds beside text they chose (BREACH), and the live updates would wait in
  a compressor's buffer rather than arrive as they happen (§10.1).

  Background:
    Given Ladderback is serving on loopback, configured for the host name "nas.demo.test"

  Scenario Outline: The answer for <what> is sent as it is, whatever the request accepts
    Given a "GET" request for "<path>"
    And the request has the header "Accept-Encoding: br, gzip, *"
    When the request is sent
    Then the response has no "Content-Encoding" header

    Examples:
      | what                 | path            |
      | the session          | /api/v1/session |
      | the plans            | /api/v1/plans   |
      | an unknown API route | /api/v1/nope    |
