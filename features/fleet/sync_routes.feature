@e2e
Feature: The hub's sync routes
  Agents reach the hub's sync routes, every path under /sync/, by whatever
  name they know the hub by, and prove themselves with signatures (technical
  design §10.3), so the browser's request checks don't fit them (§10.1).
  The sync routes have a check of their own instead: an agent's WebSocket
  handshake carries no Origin and no body, while a web page's WebSocket
  always carries its Origin, and a page's other requests can't ask to
  upgrade. A request names the hub's own loopback address as its Host unless
  a step says otherwise.

  Background:
    Given Ladderback is serving as a hub on loopback

  Scenario Outline: A web page's handshake to <path> is refused before it starts
    Given a "GET" request for "<path>"
    And the request asks to upgrade to a WebSocket
    And the request has the header "Host: hub.other.example:8443"
    And the request has the header "Origin: <origin>"
    When the request is sent
    Then the request is refused with status 403 and the error code "origin-not-allowed"

    Examples:
      | path             | origin                        |
      | /sync/v1         | http://evil.example           |
      | /sync/v1         | http://hub.other.example:8443 |
      | /sync/v1?x=1     | null                          |
      | /sync/v1/connect | http://evil.example           |
      | /sync/v2         | http://evil.example           |

  Scenario Outline: The method <method> is refused at <path>
    Given a "<method>" request for "<path>"
    And the request asks to upgrade to a WebSocket
    When the request is sent
    Then the request is refused with status 405
    And the response has the header "Allow" with the value "GET"

    Examples:
      | method  | path             |
      | POST    | /sync/v1         |
      | HEAD    | /sync/v1         |
      | OPTIONS | /sync/v1         |
      | PUT     | /sync/v1/connect |
      | DELETE  | /sync/v2         |

  Scenario Outline: A request to <path> that isn't a WebSocket handshake is refused
    Given a "GET" request for "<path>"
    And the request has the header "Host: hub.other.example:8443"
    When the request is sent
    Then the request is refused with status 426
    And the response has the header "Upgrade" with the value "websocket"

    Examples:
      | path            |
      | /sync/v1        |
      | /sync/v1/status |
