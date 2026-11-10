@e2e
Feature: Live updates
  A page keeps its view of the machine current over one WebSocket per tab
  (technical design §4.4). The daemon forwards what a run reports unchanged:
  coalescing is the worker's, once (§4.1). A connection carries nothing until
  an event arrives; the first state comes over REST. The events of the
  examples are JSON that names its kind in a "type".

  Background:
    Given Ladderback is serving on loopback with the plan "home"

  Scenario: A connected page learns the events of a run, in order
    Given a live-updates connection from the UI's own origin
    When the daemon runs a backup of the plan "home"
    Then the connected page learns the operation started
    And the connected page learns the operation's progress
    And the connected page learns the operation finished

  Scenario: Two connected pages each learn the same events
    Given a live-updates connection from the UI's own origin
    And a second live-updates connection from the UI's own origin
    When the daemon runs a backup of the plan "home"
    Then each connected page learns the operation started
    And each connected page learns the operation finished

  Scenario: A page that stops reading doesn't hold up the run or the other page
    Given a live-updates connection that stops reading
    And a live-updates connection from the UI's own origin
    When the daemon runs a backup of the plan "home"
    Then the connected page learns the operation finished
    And the daemon is still serving

  Scenario: The daemon refuses to start on a config file from a newer Ladderback
    Given the daemon is killed
    And the config folder holds a config.toml from a newer schema
    When Ladderback is asked to serve
    Then Ladderback refuses to start, naming the newer schema
