Feature: A prune deletes only while its machine may still maintain the repository
  A prune works out its plan first: the packs it will mark, delete and
  recover. Before it deletes or rewrites anything, it asks the daemon whether
  this machine may still maintain the repository, and the daemon answers as
  it did when the run started: a paired machine may only while it holds a
  live lease on the repository from its hub, judged on the hub's time, and a
  machine that isn't paired always may (technical design §7.3, I1; §16,
  decision 161). The lease is checked again, never renewed.

  The prune deletes only on yes, with the plan it worked out, never a new
  one. On no, or with no answer in time, it deletes nothing, and the run
  fails saying the lease lapsed before it deleted anything. A deletion
  already begun finishes, and I2's delayed deletion protects the packs it
  marked.

  Scenario: A lease that lapses between planning and deletion deletes nothing
    Given this machine is paired with a hub and holds the lease on the repository
    And the lease runs out while the prune works out its plan
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
