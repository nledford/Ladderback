Feature: The hub shows each machine's version of Ladderback
  Each machine gives its hub the version of Ladderback it runs in every
  `Hello`, as `--version` prints it (technical design §7.2; §16, decision
  90). The hub keeps the one each machine gave last, and its dashboard shows
  it on the machine's card. A version older than the hub's own says so in a
  quiet note, and so does a newer one, since then the hub is the one to
  upgrade; neither counts as needing attention. Versions compare by SemVer
  precedence with build metadata ignored, so two builds of one release are
  equal, and a version that doesn't read as SemVer shows as written, with no
  note (#1707).

  Background:
    Given a hub running Ladderback "0.7.0"
    And an agent paired with a hub
    And a person approved the agent on the hub

  Scenario: A machine on an older release shows its version and that it is older than the hub
    When the agent, running Ladderback "0.6.0", connects to its hub
    Then the hub shows the agent's version "0.6.0", older than the hub's

  Scenario: A machine on the hub's release shows its version with no note, whatever its build
    When the agent, running Ladderback "0.7.0+3f937cf", connects to its hub
    Then the hub shows the agent's version "0.7.0+3f937cf" with no note

  Scenario: A machine on a newer release shows its version and that it is newer than the hub
    When the agent, running Ladderback "0.8.0", connects to its hub
    Then the hub shows the agent's version "0.8.0", newer than the hub's

  Scenario: A version that doesn't read as SemVer shows as written, with no note
    When the agent, running Ladderback "nightly", connects to its hub
    Then the hub shows the agent's version "nightly" with no note

  Scenario: A machine shows no version until it connects, then the one it gave last
    Then the hub shows no version of the agent yet
    When the agent, running Ladderback "0.6.0", connects to its hub
    Then the hub shows the agent's version "0.6.0", older than the hub's
    When the agent, running Ladderback "0.7.0", connects to its hub
    Then the hub shows the agent's version "0.7.0" with no note
