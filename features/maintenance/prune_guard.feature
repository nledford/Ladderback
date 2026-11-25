Feature: A prune waits for other machines' backups and deletes only while its machine may still maintain the repository
  A prune works out its plan first: the packs it will mark, delete and
  recover. It then warms up the packs it repacks when they are on cold
  storage, and repacks the blobs it keeps into new packs under a new index,
  which adds files and removes none, and can take hours. At its first delete
  it asks the daemon whether this machine may still maintain the repository,
  and the daemon answers as it did when the run started: a paired machine
  may only while it holds a live lease on the repository from its hub,
  judged on the hub's time, and a machine that isn't paired always may
  (technical design §7.3, I1; §16, decision 161). The lease is checked
  again, never renewed, and must be the one the run started under: a lease
  the hub granted again after the first lapsed, at a new epoch, may have
  let another machine maintain the repository meanwhile.

  The prune deletes only on yes, with the plan it worked out, never a new
  one. On no, it deletes nothing, and the run fails saying the lease lapsed
  before it deleted anything. A run cancelled before the answer is given
  tells the prune no. A deletion already begun finishes, and I2's delayed
  deletion protects the packs it marked.

  Before a prune starts, its hub must not hold it. The hub holds a prune of
  a repository while another machine it holds a connection of is backing up
  to that repository, or while it can't tell what such a machine runs: its
  heartbeat doesn't say, or a backup it runs doesn't know its repository's
  ID yet. Each heartbeat the machine sends, every 30 seconds, says the
  backups it runs, and the hub's answer says which prunes it holds. A hold
  ends when the backup finishes, when that machine goes offline (the hub
  heard nothing from it for 90 seconds), or when this machine's lease ends,
  after which it maintains nothing. A paired machine that hasn't heard its
  hub's answer for 90 seconds can't tell, and holds its prunes too. A held
  prune is refused before anything is stored or run, saying another
  machine may be backing up to the repository; the person starts it again
  once that backup has finished (technical design §7.3, layer 2; §16,
  decision 166). A machine that isn't paired has no hub to hold its prune.

  Scenario: A lease that lapses while the prune repacks deletes nothing
    Given this machine is paired with a hub and holds the lease on the repository
    And the lease runs out while the prune repacks, after it worked out its plan
    When this machine prunes the repository
    Then the prune deleted nothing
    And the prune failed because the lease lapsed before it deleted anything

  Scenario: A lease still held deletes as before
    Given this machine is paired with a hub and holds the lease on the repository
    When this machine prunes the repository
    Then the prune deleted what it planned

  Scenario: A machine that isn't paired prunes as before
    Given this machine isn't paired with a hub
    When this machine prunes the repository
    Then the prune deleted what it planned

  Scenario: A prune cancelled just as it asks deletes nothing
    Given this machine isn't paired with a hub
    And the prune is cancelled just as it asks whether it may delete
    When this machine prunes the repository
    Then the prune deleted nothing
    And the prune was cancelled

  Scenario: A lease that lapses and is granted again while the prune repacks deletes nothing
    Given this machine is paired with a hub and holds the lease on the repository
    And the lease runs out while the prune repacks, and the hub grants this machine a new one before the prune asks
    When this machine prunes the repository
    Then the prune deleted nothing
    And the prune failed because the lease lapsed before it deleted anything

  Scenario: A prune cancelled while the daemon checks whether it may delete deletes nothing
    Given this machine is paired with a hub and holds the lease on the repository
    And the prune is cancelled while the daemon checks whether it may delete
    When this machine prunes the repository
    Then the prune deleted nothing
    And the prune was cancelled

  Scenario: Another machine's backup to the repository holds the prune
    Given this machine is paired with a hub and holds the lease on the repository
    And another machine is backing up to the repository
    When this machine prunes the repository
    Then the prune is held because another machine may be backing up to the repository
    And nothing ran

  Scenario: A backup that finished no longer holds the prune
    Given this machine is paired with a hub and holds the lease on the repository
    And another machine was backing up to the repository, and its backup finished
    When this machine prunes the repository
    Then the prune deleted what it planned

  Scenario: A machine that went offline while it backed up no longer holds the prune
    Given this machine is paired with a hub and holds the lease on the repository
    And another machine was backing up to the repository, and went offline
    When this machine prunes the repository
    Then the prune deleted what it planned

  Scenario: A backup to another repository doesn't hold the prune
    Given this machine is paired with a hub and holds the lease on the repository
    And another machine is backing up to another repository
    When this machine prunes the repository
    Then the prune deleted what it planned

  Scenario: A machine whose heartbeat doesn't say what it runs holds the prune
    Given this machine is paired with a hub and holds the lease on the repository
    And another machine is online, and its heartbeat doesn't say what it runs
    When this machine prunes the repository
    Then the prune is held because another machine may be backing up to the repository
    And nothing ran

  Scenario: A backup that doesn't know its repository's ID yet holds the prune
    Given this machine is paired with a hub and holds the lease on the repository
    And another machine is backing up to a repository whose ID it doesn't know yet
    When this machine prunes the repository
    Then the prune is held because another machine may be backing up to the repository
    And nothing ran

  Scenario: A machine that hasn't heard from its hub for 90 seconds holds its prune
    Given this machine is paired with a hub and holds the lease on the repository
    And this machine last heard its hub answer a heartbeat 90 seconds ago
    When this machine prunes the repository
    Then the prune is held because another machine may be backing up to the repository
    And nothing ran

  Scenario: A backup that starts on another machine after the prune started doesn't stop it
    Given this machine is paired with a hub and holds the lease on the repository
    And another machine starts backing up to the repository while the prune repacks
    When this machine prunes the repository
    Then the prune deleted what it planned
