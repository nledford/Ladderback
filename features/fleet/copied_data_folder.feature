Feature: A copied data folder keeps backing up and stops maintaining
  A machine's ID lives in its data folder (§7.1). A copy of that folder on
  another computer, by a restored backup of a home folder or a cloned disk
  or virtual machine, runs under the same machine ID, and its forget would
  take the first computer's snapshots for its own (I1, I6). So the machine
  identifier the operating system gives the computer (`/etc/machine-id` on
  Linux, `IOPlatformUUID` on macOS) is recorded beside the machine ID and
  compared at each start. A copy keeps backing up, since its backups lose
  nothing, and refuses every forget, prune and adoption, and the preview of
  each, saying why; a check only reads, and still runs. The person can say
  the copy is a new computer: it gives up maintaining every repository, so
  the two computers never both maintain one, and takes a new machine ID,
  used from its next start. A data folder from before the identifier was
  kept records this computer's silently: that is an upgrade, not a copy.
  An identifier that can't be read blocks nothing, and the log says so.

  Background:
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository's maintenance is "local"

  Scenario Outline: A copied data folder refuses <asked>, saying why
    Given the data folder was copied from another computer
    When the agent starts on this computer
    And this agent is asked for <asked> of the repository
    Then it is refused because the data folder was copied from another computer
    And the engine was never given a job
    And the operation log holds no operation

    Examples:
      | asked                       |
      | a forget                    |
      | a prune                     |
      | an adoption                 |
      | a preview of a forget       |
      | a preview of a prune        |
      | a preview of an adoption    |
      | a proposal's forget preview |

  Scenario: A copied data folder is told at start
    Given the data folder was copied from another computer
    When the agent starts on this computer
    Then the agent finds its data folder was copied from another computer
    And the agent doesn't maintain the repositories its configuration asks it to
    And the machine ID is unchanged

  Scenario: Taking a new ID makes it a new machine, and maintenance can be set up again
    Given the data folder was copied from another computer
    When the agent starts on this computer
    And the person takes a new machine ID
    Then the machine has a new machine ID
    And the machine holds a new agent key
    And the data folder records this computer's machine identifier
    And the repository's maintenance is now "none"
    And maintenance stays refused until the agent starts again
    When the agent starts on this computer again
    Then the agent finds nothing new
    And the agent maintains the repositories its configuration asks it to

  Scenario: A running check keeps the machine ID and the maintenance
    A check still runs on a copy, and while it runs this machine maintains
    the repository: giving it up then would let the other computer start a
    prune while the check still reads (I1).
    Given the data folder was copied from another computer
    And a check of the repository is running here
    When the agent starts on this computer
    And the person takes a new machine ID
    Then taking a new machine ID is refused because that check is running
    And the machine ID is unchanged
    And the machine's agent key is unchanged
    And the repository's maintenance is now "local"

  Scenario: An unchanged machine sees nothing new
    Given the data folder was made on this computer
    When the agent starts on this computer
    Then the agent finds nothing new
    And the agent maintains the repositories its configuration asks it to
    And the machine ID is unchanged

  Scenario: An unchanged machine can't take a new ID
    Given the data folder was made on this computer
    When the agent starts on this computer
    And the person takes a new machine ID
    Then taking a new machine ID is refused, as the data folder isn't a copy
    And the machine ID is unchanged
    And the repository's maintenance is now "local"

  Scenario: A data folder from before the identifier was kept records it silently
    Given the data folder was made before the machine identifier was kept
    When the agent starts on this computer
    Then the agent finds nothing new
    And the data folder records this computer's machine identifier
    And the agent maintains the repositories its configuration asks it to

  Scenario: An identifier that can't be read blocks nothing, and the log says so
    Given this computer's machine identifier can't be read
    When the agent starts on this computer
    Then the agent notes that this computer's machine identifier couldn't be read
    And the agent maintains the repositories its configuration asks it to
