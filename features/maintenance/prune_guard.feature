Feature: A prune deletes only while its machine may still maintain the repository
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
