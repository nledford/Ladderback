@e2e
Feature: API paths no route serves
  The daemon serves its API under /api, and every other path belongs to the
  web UI, whose client-side routes all get its page (§4.2). A program that
  asks the API for a route this Ladderback doesn't have, one from a newer or
  older version or a typo, is refused with an error it can read, never sent
  the page as a success, so it can tell "no such route" from a broken answer
  (#1063).

  Background:
    Given Ladderback is serving on loopback, configured for the host name "nas.demo.test"

  Scenario Outline: A <method> request for <path>, which no route serves, is refused
    Given a "<method>" request for "<path>"
    And the request has the header "Content-Type: application/json"
    When the request is sent
    Then the request is refused with status 404 and the error code "route-unknown"

    Examples:
      | method | path                   |
      | GET    | /api/v1/no-such-route  |
      | POST   | /api/v1/no-such-route  |
      | PUT    | /api/v1/no-such-route  |
      | DELETE | /api/v1/no-such-route  |
      | GET    | /api/v2/plans          |
      | GET    | /api                   |

  Scenario: A HEAD request for an API route no route serves is refused
    Given a "HEAD" request for "/api/v1/no-such-route"
    When the request is sent
    Then the request is answered with status 404
    And the response has the header "Content-Type" with the value "application/json"
