Feature: The release check
  While the agent serves, it asks for Ladderback's newest release at most
  once a day, at a time spread by up to an hour of jitter, so that agents
  started together don't all ask at once (decision NN). A full release newer
  than the version the agent runs is the available update: the header shows
  it, and the log says so once for each new version. A prerelease, an equal
  or older version, or an answer that fails or can't be read shows nothing,
  and a failure is noted only for debugging. The agent keeps its last check
  between its starts, so a restart neither asks again within the day nor
  hides the update it found. With `check-for-updates = false` in the
  `[agent]` table of its configuration, it never asks. Times here are
  written with their offset from UTC.

  Background:
    Given the agent runs version "0.2.0"
    And the agent's clock reads "2026-06-10 00:00+00:00"

  Scenario: A newer release is the available update, and the log says so once
    Given the newest release is "v0.3.0"
    When the agent serves for 3 days
    Then the header shows that version "0.3.0" is available
    And the log said once that version "0.3.0" is available

  Scenario Outline: An equal or older release, or a prerelease, is no update
    Given the newest release is "<tag>"<marked>
    When the agent serves for 2 days
    Then the header shows no available update
    And the log said nothing of an update

    Examples:
      | tag         | marked                 |
      | v0.2.0      |                        |
      | v0.1.9      |                        |
      | v0.3.0-rc.1 |                        |
      | v0.3.0      | , marked a prerelease  |

  Scenario: The agent asks at most once a day
    Given the newest release is "v0.3.0"
    When the agent serves for 3 days
    Then the agent asked for the newest release 3 times
    And each time at least a day after the one before
    And the agent first asked within 65 minutes of starting

  Scenario: A release published later is found at the next check and said once
    Given the newest release is "v0.3.0"
    When the agent serves for 1 day
    And the newest release becomes "v0.4.0"
    And the agent serves for 2 days
    Then the header shows that version "0.4.0" is available
    And the log said once that version "0.3.0" is available
    And the log said once that version "0.4.0" is available

  Scenario: An agent that restarts within a day of its last check doesn't ask again, and shows what it found
    Given the agent last checked at "2026-06-09 12:00+00:00", when version "0.3.0" was available
    And the newest release is "v0.3.0"
    When the agent serves until its clock reads "2026-06-10 11:59+00:00"
    Then the agent never asked for the newest release
    And the header shows that version "0.3.0" is available
    And the log said nothing of an update

  Scenario: The update the agent found before stops showing once it runs that version
    Given the agent runs version "0.3.0"
    And the agent last checked at "2026-06-09 12:00+00:00", when version "0.3.0" was available
    When the agent serves until its clock reads "2026-06-10 11:59+00:00"
    Then the header shows no available update

  Scenario: With the release check off, the agent never asks
    Given the agent's configuration turns the release check off
    And the newest release is "v0.3.0"
    When the agent serves for 3 days
    Then the agent never asked for the newest release
    And the header shows no available update

  Scenario: Turning the release check off hides the update it found
    Given the agent last checked at "2026-06-09 12:00+00:00", when version "0.3.0" was available
    And the agent's configuration turns the release check off
    When the agent serves for 2 days
    Then the agent never asked for the newest release
    And the header shows no available update

  Scenario Outline: A check that fails shows nothing, and is noted only for debugging
    Given the newest release can't be read: <failure>
    When the agent serves for 2 days
    Then the header shows no available update
    And the log said nothing of an update
    And the log noted the failed checks only for debugging

    Examples:
      | failure     |
      | unreachable |
      | timed out   |
      | rejected    |
      | unreadable  |

  Scenario: A check that fails keeps the update found before
    Given the agent last checked at "2026-06-09 00:00+00:00", when version "0.3.0" was available
    And the newest release can't be read: unreachable
    When the agent serves for 1 day
    Then the agent asked for the newest release 1 time
    And the header shows that version "0.3.0" is available
    And the log said nothing of an update
