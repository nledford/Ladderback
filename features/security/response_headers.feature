@e2e
Feature: Security response headers
  With sign-in off, the web UI is a local admin console that can set hooks, so
  a page that frames it, or a browser that guesses what a response is, can do
  harm even on loopback. Every response says what a browser may do with it
  (§10.1). The daemon serves plain HTTP on loopback, so it sends no
  Strict-Transport-Security header: a browser would hold on to it for the
  address of any other service that later listens there.

  Background:
    Given Ladderback is serving on loopback, configured for the host name "nas.demo.test"

  Rule: Every response carries the headers

    Scenario Outline: The response for <what> carries the security headers
      Given a "GET" request for "<path>"
      When the request is sent
      Then the response has the header "X-Content-Type-Options" with the value "nosniff"
      And the response has the header "Referrer-Policy" with the value "no-referrer"
      And the response has a "Content-Security-Policy" header

      Examples:
        | what                  | path           |
        | the web UI            | /              |
        | a client-side route   | /plans         |
        | a missing file        | /missing.js    |
        | the API               | /api/v1/plans  |
        | an unknown API route  | /api/v1/nope   |

    Scenario: A refused request's response carries the security headers
      Given a "GET" request for "/"
      And the request has the header "Host: evil.example:{port}"
      When the request is sent
      Then the request is refused with status 403 and the error code "host-not-allowed"
      And the response has the header "X-Content-Type-Options" with the value "nosniff"
      And the response has the header "Referrer-Policy" with the value "no-referrer"
      And the response has a "Content-Security-Policy" header

  Rule: The content security policy lets the page run its own code and nothing else

    Scenario Outline: The policy has the directive <directive>
      Given a "GET" request for "/"
      When the request is sent
      Then the response's "Content-Security-Policy" has the directive "<directive>"

      Examples:
        | directive                                     |
        | default-src 'self'                            |
        | script-src 'self' 'wasm-unsafe-eval'          |
        | frame-ancestors 'none'                        |
