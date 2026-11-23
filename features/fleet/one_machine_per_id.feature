Feature: The hub takes one machine under each machine ID
  A data folder copied to a second computer, by a restored home folder or a
  cloned disk, brings the first computer's machine ID and agent key with it,
  so both computers can prove themselves to the hub as one machine. Their
  reports would interleave under one ID and both could hold its leases (I1,
  I6). So each machine's `Hello` carries its machine fingerprint, a digest
  of the machine identifier the operating system gives its computer, never
  the identifier itself. The hub keeps the first fingerprint it hears under
  each machine ID, and refuses a machine whose fingerprint is another as
  `identity-conflict`: it takes none of its reports, says so in its log and
  in its list of machines, and leaves the first machine's connection alone.
  The same computer connecting again has the same fingerprint and is taken
  as before (technical design §7.1, §7.2; §16, decision 150; #296).

  Background:
    Given an agent paired with a hub
    And a person approved the agent on the hub
    And the agent connected to its hub

  Scenario: A copy of the data folder on a second computer is refused, and the first machine syncs on
    Given a copy of the agent's data folder on another computer
    And the copy ran 1 backup
    When the copy syncs with its hub
    Then the copy is refused because another machine holds its machine ID
    And the hub holds no report of the copy
    And the copy's outbox holds 2 reports the hub hasn't acknowledged
    And the hub lists the agent as having refused a second machine
    When the agent runs 1 backup
    And the agent syncs with its hub
    Then the hub holds the agent's 2 reports, each once, in the order they happened

  Scenario: The same computer connecting again is taken
    When the agent connects to its hub
    Then the agent is told the hub approved it
    And the hub lists no second machine under the agent

  Scenario: A machine that can't tell its computer's identifier is taken
    An identifier that can't be read stops nothing, as on the machine itself
    (decision 144): the hub can't tell such a machine from the first.
    Given a copy of the agent's data folder on a computer whose identifier can't be read
    When the copy connects to its hub
    Then the copy is told the hub approved it
    And the hub lists no second machine under the agent

  Scenario: Once a person at the hub forgets the computer it took, a second computer is taken in its place
    The way back for a machine refused as a second computer, when it is the
    one that should keep the machine ID: the person stops the other computer,
    or gives it a new machine ID, then forgets on the hub the computer the
    hub took. The next computer to connect is taken from then on.
    Given a copy of the agent's data folder on another computer
    And the copy ran 1 backup
    When the copy syncs with its hub
    Then the copy is refused because another machine holds its machine ID
    When a person at the hub forgets the computer it took for the agent
    And the copy syncs with its hub
    Then the hub holds the copy's 2 reports, each once, in the order they happened
    And the hub lists no second machine under the agent
