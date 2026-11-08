@e2e
Feature: Request checks
  The web UI can set hooks and the password command, so an API that a web page
  can reach would let any site run commands on this machine, through DNS
  rebinding or a cross-site POST. Steading checks every request, whether or not
  sign-in is on (§10.1). A request names the daemon's own loopback address as
  its Host unless a step says otherwise. In the examples, "{port}" stands for
  the port the daemon is listening on.

  Background:
    Given Steading is serving on loopback, configured for the host name "nas.demo.test"

  Rule: The Host header must be a loopback address or a configured name

    Scenario Outline: A request for <host> is served
      Given a "GET" request for "/"
      And the request has the header "Host: <host>"
      When the request is sent
      Then the request isn't refused by a request check

      Examples:
        | host                  |
        | 127.0.0.1:{port}      |
        | 127.1.2.3:{port}      |
        | localhost:{port}      |
        | LocalHost:{port}      |
        | localhost             |
        | [::1]:{port}          |
        | nas.demo.test:{port}  |
        | NAS.Demo.Test         |

    Scenario Outline: A request for <host> is refused
      Given a "GET" request for "/"
      And the request has the header "Host: <host>"
      When the request is sent
      Then the request is refused with status 403 and the error code "host-not-allowed"

      Examples:
        | host                              |
        | evil.example:{port}               |
        | nas.demo.test.evil.example        |
        | evil.nas.demo.test:{port}         |
        | localhost.evil.example:{port}     |
        | localhost@evil.example            |
        | 192.168.1.5:{port}                |
        | 0.0.0.0:{port}                    |
        | [::2]:{port}                      |
        | 127.1                             |
        | localhost:                        |
        | localhost:99999                   |

    Scenario: A request with no Host header is refused
      Given a "GET" request for "/"
      And the request has no Host header
      When the request is sent
      Then the request is refused with status 403 and the error code "host-not-allowed"

    Scenario: A request with two Host headers is refused, even when both are allowed
      Given a "GET" request for "/"
      And the request has the header "Host: localhost:{port}"
      And the request has the header "Host: nas.demo.test"
      When the request is sent
      Then the request is refused with status 403 and the error code "host-not-allowed"

    Scenario Outline: A rebound name can't reach the API with a <method> request
      Given a "<method>" request for "/api/v1/plans"
      And the request has the header "Host: evil.example:{port}"
      And the request has the header "Origin: http://evil.example:{port}"
      And the request has the header "Content-Type: application/json"
      When the request is sent
      Then the request is refused with status 403 and the error code "host-not-allowed"

      Examples:
        | method |
        | GET    |
        | POST   |
        | DELETE |

  Rule: A request that carries an Origin header must come from the UI's own origin

    Scenario: A request without an Origin header isn't affected by the Origin rule
      Given a "POST" request for "/api/v1/plans"
      And the request has the header "Content-Type: application/json"
      When the request is sent
      Then the request isn't refused by a request check

    Scenario Outline: A request from <origin> to <host> is served
      Given a "POST" request for "/api/v1/plans"
      And the request has the header "Host: <host>"
      And the request has the header "Origin: <origin>"
      And the request has the header "Content-Type: application/json"
      When the request is sent
      Then the request isn't refused by a request check

      Examples:
        | host                 | origin                      |
        | localhost:{port}     | http://localhost:{port}     |
        | 127.0.0.1:{port}     | http://127.0.0.1:{port}     |
        | [::1]:{port}         | http://[::1]:{port}         |
        | nas.demo.test:{port} | http://NAS.demo.test:{port} |
        | nas.demo.test        | https://nas.demo.test       |

    Scenario Outline: A request from <origin> is refused
      Given a "POST" request for "/api/v1/plans"
      And the request has the header "Origin: <origin>"
      And the request has the header "Content-Type: application/json"
      When the request is sent
      Then the request is refused with status 403 and the error code "origin-not-allowed"

      Examples:
        | origin                                     |
        | http://evil.example                        |
        | https://evil.example:{port}                |
        | http://localhost:1                         |
        | http://localhost                           |
        | http://127.0.0.1:{port}                    |
        | http://nas.demo.test:{port}                |
        | http://localhost:{port}.evil.example       |
        | http://localhost:{port}@evil.example       |
        | http://localhost:{port}/                   |
        | http://localhost:{port}/path               |
        | ftp://localhost:{port}                     |
        | localhost:{port}                           |
        | null                                       |

    Scenario: A request with two Origin headers is refused, even when both match
      Given a "GET" request for "/"
      And the request has the header "Origin: http://localhost:{port}"
      And the request has the header "Origin: http://localhost:{port}"
      When the request is sent
      Then the request is refused with status 403 and the error code "origin-not-allowed"

    Scenario: A cross-site form post is refused
      Given a "POST" request for "/api/v1/plans"
      And the request has the header "Origin: http://evil.example"
      And the request has the header "Content-Type: application/x-www-form-urlencoded"
      When the request is sent
      Then the request is refused with status 403 and the error code "origin-not-allowed"

    Scenario: A cross-origin preflight is refused and grants nothing
      Given a "OPTIONS" request for "/api/v1/plans"
      And the request has the header "Origin: http://evil.example"
      And the request has the header "Access-Control-Request-Method: POST"
      And the request has the header "Access-Control-Request-Headers: content-type"
      When the request is sent
      Then the request is refused with status 403 and the error code "origin-not-allowed"
      And the response has no "Access-Control-Allow-Origin" header

    Scenario: The live-updates handshake from another origin is refused
      Given a "GET" request for "/api/v1/events"
      And the request has the header "Origin: http://evil.example"
      And the request asks to upgrade to a WebSocket
      When the request is sent
      Then the request is refused with status 403 and the error code "origin-not-allowed"

    Scenario: The live-updates handshake from the UI's own origin is served
      Given a "GET" request for "/api/v1/events"
      And the request has the header "Origin: http://localhost:{port}"
      And the request asks to upgrade to a WebSocket
      When the request is sent
      Then the request isn't refused by a request check

  Rule: A mutating request from the UI's page must use Content-Type: application/json

    Background:
      Given the request comes from the UI's page

    Scenario Outline: A <method> request as <type> is served
      Given a "<method>" request for "/api/v1/plans"
      And the request has the header "Content-Type: <type>"
      When the request is sent
      Then the request isn't refused by a request check

      Examples:
        | method | type                             |
        | POST   | application/json                 |
        | POST   | application/json; charset=utf-8  |
        | POST   | Application/JSON                 |
        | PUT    | application/json                 |
        | PATCH  | application/json; charset=UTF-8  |
        | DELETE | application/json                 |

    Scenario Outline: A <method> request as <type> is refused
      Given a "<method>" request for "/api/v1/plans"
      And the request has the header "Content-Type: <type>"
      When the request is sent
      Then the request is refused with status 415 and the error code "json-required"

      Examples:
        | method   | type                                |
        | POST     | text/plain                          |
        | POST     | text/plain;charset=UTF-8            |
        | POST     | application/x-www-form-urlencoded   |
        | POST     | multipart/form-data; boundary=x     |
        | POST     | application/octet-stream            |
        | POST     | application/jsonp                   |
        | POST     | application/vnd.api+json            |
        | POST     | application/json, text/plain        |
        | PUT      | text/plain                          |
        | PATCH    | text/plain                          |
        | DELETE   | text/plain                          |
        | PROPFIND | text/plain                          |

    Scenario Outline: A <method> request with no Content-Type is refused
      Given a "<method>" request for "/api/v1/plans"
      When the request is sent
      Then the request is refused with status 415 and the error code "json-required"

      Examples:
        | method   |
        | POST     |
        | PUT      |
        | PATCH    |
        | DELETE   |
        | TRACE    |
        | PROPFIND |

    Scenario: A request with two Content-Type headers is refused, even when one is JSON
      Given a "POST" request for "/api/v1/plans"
      And the request has the header "Content-Type: application/json"
      And the request has the header "Content-Type: text/plain"
      When the request is sent
      Then the request is refused with status 415 and the error code "json-required"

    Scenario Outline: A <method> request needs no Content-Type
      Given a "<method>" request for "/api/v1/plans"
      When the request is sent
      Then the request isn't refused by a request check

      Examples:
        | method  |
        | GET     |
        | HEAD    |
        | OPTIONS |

  Rule: A mutating request needs JSON even without an Origin header

    A browser or extension that strips Origin must not let a cross-site form
    post through, and curl and scripts send the header.

    Scenario Outline: A <method> request as <type> without an Origin is refused
      Given a "<method>" request for "/api/v1/plans"
      And the request has the header "Content-Type: <type>"
      When the request is sent
      Then the request is refused with status 415 and the error code "json-required"

      Examples:
        | method | type                              |
        | POST   | application/x-www-form-urlencoded |
        | POST   | text/plain                        |
        | POST   | multipart/form-data; boundary=x   |
        | DELETE | text/plain                        |

    Scenario Outline: A <method> request with no Content-Type and no Origin is refused
      Given a "<method>" request for "/api/v1/plans"
      When the request is sent
      Then the request is refused with status 415 and the error code "json-required"

      Examples:
        | method |
        | POST   |
        | PUT    |
        | PATCH  |
        | DELETE |

    Scenario Outline: A <method> request as <type> without an Origin is served
      Given a "<method>" request for "/api/v1/plans"
      And the request has the header "Content-Type: <type>"
      When the request is sent
      Then the request isn't refused by a request check

      Examples:
        | method | type                             |
        | POST   | application/json                 |
        | POST   | application/json; charset=utf-8  |
        | PUT    | application/json                 |
        | DELETE | application/json                 |

  Rule: Agents reach /sync/v1 by their own authentication, not the browser checks

    Scenario Outline: A <method> request to <path> from any host and origin is served
      Given a "<method>" request for "<path>"
      And the request has the header "Host: hub.other.example:8443"
      And the request has the header "Origin: http://evil.example"
      And the request has the header "Content-Type: text/plain"
      When the request is sent
      Then the request isn't refused by a request check

      Examples:
        | method | path             |
        | GET    | /sync/v1         |
        | GET    | /sync/v1?x=1     |
        | GET    | /sync/v1/connect |
        | POST   | /sync/v1/connect |

    Scenario Outline: Only /sync/v1 itself is exempt: <path> is refused
      Given a "GET" request for "<path>"
      And the request has the header "Host: hub.other.example:8443"
      When the request is sent
      Then the request is refused with status 403 and the error code "host-not-allowed"

      Examples:
        | path                        |
        | /sync/v10                   |
        | /sync/v1x/a                 |
        | /sync                       |
        | /api/v1/sync/v1             |
        | //sync/v1                   |
        | /api/v1/plans?next=/sync/v1 |

  Rule: Without sign-in, Steading only listens on loopback

    Scenario: Steading refuses to listen on every IPv4 address
      When Steading is started with the arguments "--listen 0.0.0.0:0"
      Then Steading refuses to start

    @needs-ipv6
    Scenario: Steading refuses to listen on every IPv6 address
      When Steading is started with the arguments "--listen [::]:0"
      Then Steading refuses to start

    Scenario: Steading refuses a host name that includes a port
      When Steading is started with the arguments "--allowed-host nas.demo.test:9898"
      Then Steading rejects the argument "nas.demo.test:9898"
