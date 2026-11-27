Feature: An agent's reports reach its hub once, in order
  An agent tells its hub how each of its operations started and ended. Each
  report enters the agent's outbox as it happens, numbered in order, and
  stays there, through a lost connection or a restart, until the hub
  acknowledges it. Only an approved agent's reports are sent. The hub takes a
  report it took before as a duplicate, so a report sent again is kept once
  (technical design §7.2; §16, decision 120, "Signed messages after the
  handshake"). Running the agent's operations never waits on the hub.

  Background:
    Given an agent paired with a hub
    And a person approved the agent on the hub

  Scenario: The hub is down during backups, and the reports reach it in order once it is back
    Given the hub can't be reached
    When the agent runs 3 backups
    And the agent tries to sync with its hub
    Then the 3 backups ran while the hub couldn't be reached
    And the agent's outbox holds 6 reports the hub hasn't acknowledged
    When the hub can be reached again
    And the agent syncs with its hub
    Then the hub holds the agent's 6 reports, each once, in the order they happened
    And the agent's outbox holds no report the hub hasn't acknowledged

  Scenario: A machine that can't reach its hub keeps backing up, and its reports reach the hub once it can
    Given the agent keeps syncing with its hub while it runs
    And the hub can't be reached
    When the agent runs 1 backup
    Then the backup's 2 reports were kept while the agent's sync couldn't reach the hub
    When the hub can be reached again
    Then the hub soon holds the agent's 2 reports, each once, in the order they happened

  Scenario: The agent restarts with reports it never sent
    Given the hub can't be reached
    And the agent ran 2 backups
    When the agent restarts
    And the hub can be reached again
    And the agent syncs with its hub
    Then the hub holds the agent's 4 reports, each once, in the order they happened
    And the agent's outbox holds no report the hub hasn't acknowledged

  Scenario: A report delivered twice is taken once
    Given the agent ran 1 backup
    And the hub's next acknowledgement is lost with its connection
    When the agent tries to sync with its hub
    And the agent syncs with its hub
    Then the hub received the agent's first report twice
    And the hub holds the agent's 2 reports, each once, in the order they happened
    And the agent's outbox holds no report the hub hasn't acknowledged

  Scenario: A pending agent's reports wait in its outbox
    Given another agent paired with the hub and waiting for approval
    And the other agent ran 1 backup
    When the other agent syncs with its hub
    Then the other agent is told it waits for approval
    And the hub holds no report of the other agent
    And the other agent's outbox holds 2 reports the hub hasn't acknowledged

  Rule: A machine restored from an older copy of its database starts a new epoch (decision 177)
    Its `Hello` says where its outbox stands. When the hub already took the
    place the outbox gives next, the machine numbers its reports again from
    an older place, so it starts a new epoch and sends its waiting reports
    under it; the hub keeps both epochs, and each report once.

    Scenario: A machine restored from an older copy of its database has every later report kept by the hub, and none twice
      Given the hub can't be reached
      And the agent ran 1 backup
      And a copy of the agent's database was kept
      And the hub can be reached again
      And the agent synced with its hub
      And the agent ran 2 backups
      And the agent synced with its hub
      When the agent's database is restored from the copy
      And the agent runs 1 backup
      And the agent syncs with its hub
      Then the agent's outbox has started a new epoch
      And the hub holds each of the agent's 8 reports once, in the order it took them
      And the agent's outbox holds no report the hub hasn't acknowledged

    Scenario: A restored machine whose connection drops under its new epoch sends each report once
      Given the agent ran 1 backup
      And a copy of the agent's database was kept
      And the agent synced with its hub
      And the agent ran 2 backups
      And the agent synced with its hub
      When the agent's database is restored from the copy
      And the agent runs 1 backup
      And the hub's next acknowledgement is lost with its connection
      And the agent tries to sync with its hub
      And the agent restarts
      And the agent syncs with its hub
      Then the agent's outbox has started a new epoch
      And the hub holds each of the agent's 8 reports once, in the order it took them
      And the agent's outbox holds no report the hub hasn't acknowledged

    Scenario: A machine that wasn't restored keeps its epoch
      Given the agent ran 1 backup
      And the agent synced with its hub
      And the agent ran 1 backup
      When the agent restarts
      And the agent syncs with its hub
      Then the agent's last Hello said its outbox gives place 5 next
      And the agent's outbox keeps its epoch
      And the hub holds the agent's 4 reports, each once, in the order they happened

    Scenario: A restored machine whose Hello says nothing of its outbox, as 0.6.0's doesn't, is taken as before
      Given the agent's Hello says nothing of its outbox, as a machine on 0.6.0 does
      And the agent ran 1 backup
      And a copy of the agent's database was kept
      And the agent synced with its hub
      And the agent ran 2 backups
      And the agent synced with its hub
      When the agent's database is restored from the copy
      And the agent runs 1 backup
      And the agent syncs with its hub
      Then the agent's outbox keeps its epoch
      And the hub holds the agent's first 6 reports only, as it did before decision 177
