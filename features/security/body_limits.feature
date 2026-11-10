@e2e
Feature: Request body limits
  The daemon reads a request's body into memory before a route sees it, so a
  body of any size would let one request take that memory from the backups
  (§10.1). Each class of route has a limit of its own, the smallest that fits
  the largest body that class takes. The limit is checked before the route
  runs, and a body over it is refused with a typed code that names the limit.

  Background:
    Given Ladderback is serving on loopback, configured for the host name "nas.demo.test"

  Rule: A request to the API may carry up to 128 KiB (131,072 bytes)

    Scenario: A body of exactly the limit is read
      Given a "POST" request for "/api/v1/operations"
      And the request has the header "Content-Type: application/json"
      And the request has a JSON body of 131072 bytes
      When the request is sent
      Then the request isn't refused for its size

    Scenario Outline: A body of <size> bytes sent to <path> is refused
      Given a "POST" request for "<path>"
      And the request has the header "Content-Type: application/json"
      And the request has a JSON body of <size> bytes
      When the request is sent
      Then the request is refused with status 413 and the error code "body-too-large", naming the limit of 131072 bytes

      Examples:
        | path                                   | size    |
        | /api/v1/operations                     | 131073   |
        | /api/v1/operations                     | 262144 |
        | /api/v1/operations/some-id/cancel      | 131073   |
        | /api/v1/repositories                   | 131073   |
        | /api/v1/repositories/_test             | 131073   |
        | /api/v1/repositories/_init             | 131073   |
        | /api/v1/repositories/demo/maintenance  | 131073   |

  Rule: A request for the web UI takes no body

    Scenario: A request for a page with a body is refused
      Given a "GET" request for "/"
      And the request has a body of 1 byte
      When the request is sent
      Then the request is refused with status 413 and the error code "body-too-large", naming the limit of 0 bytes
