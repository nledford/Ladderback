Feature: Only the maintenance owner forgets, prunes, checks and adopts
  Exactly one party maintains a repository (I1). An agent with no hub is
  that party only for a repository whose entry sets `maintenance = "local"`
  (§7.3): `none` leaves maintenance to something outside Ladderback, such as
  a cron job, and `hub` to a hub this agent doesn't have. A forget, a prune,
  a check and an adoption of any other repository, and the preview of each,
  are refused before the engine is asked anything, and nothing is written
  to the operation log. Its history is a read, and is listed all the same.

  Scenario Outline: <asked> of a repository whose maintenance is "<maintenance>" is refused
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 2 snapshots another agent's plan "home" holds
    And the repository's maintenance is "<maintenance>"
    When this agent is asked for <asked> of the repository
    Then it is refused because this agent does not maintain the repository
    And the engine was never given a job
    And the operation log holds no operation

    Examples:
      | maintenance | asked                    |
      | none        | a forget                 |
      | none        | a prune                  |
      | none        | a check                  |
      | none        | an adoption              |
      | none        | a preview of a forget    |
      | none        | a preview of a prune     |
      | none        | a preview of a check     |
      | none        | a preview of an adoption |
      | hub         | a forget                 |
      | hub         | a prune                  |
      | hub         | a check                  |
      | hub         | an adoption              |
      | hub         | a preview of a forget    |
      | hub         | a preview of a prune     |
      | hub         | a preview of a check     |
      | hub         | a preview of an adoption |

  Scenario Outline: The history of a repository whose maintenance is "<maintenance>" is still listed
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 2 snapshots another agent's plan "home" holds
    And the repository's maintenance is "<maintenance>"
    When this agent lists the repository's groups
    Then the groups list the plan "home" with 3 snapshots
    And the groups list an orphaned group of 2 snapshots that can be adopted into "home"

    Examples:
      | maintenance |
      | none        |
      | hub         |
