Feature: A repository maintained here warns of snapshots from other hosts
  Without a hub, a repository whose entry asks for `local` maintenance is
  forgotten, pruned and adopted from this agent directly, and a second
  machine that maintains it too can't be detected (I1, §7.3). What can be
  seen is who else writes to it: a snapshot this agent didn't tag, whose
  host is not one this agent records, neither the machine's own name, nor
  the agent's hostname, nor a plan's host, comes from another host, which
  may maintain the repository too. So does a snapshot another agent tagged
  in the last 30 days, under any host name: two machines can share one
  name. One it tagged longer ago than that may be this machine's own from
  before it was reinstalled, so its host name alone decides. A snapshot
  this agent tagged is this host's under any name, as one made before the
  machine was renamed. So the repository's groups, and the review of a
  forget, an adoption or a prune, name those other hosts, and a run of one
  ends with them as a warning. A check only reads, so it warns of nothing.
  The groups of a repository this agent doesn't maintain, the one thing it
  may still ask of one, warn of nothing, whoever writes to it.

  Scenario: The groups of a repository maintained here name the other host plain rustic wrote from
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 2 snapshots of the plan "home"
    And the repository holds 2 snapshots plain rustic made on the host "demo-desktop"
    When the owner lists the repository's groups
    Then the groups warn of snapshots from the other hosts "demo-desktop"

  Scenario: The groups of a repository maintained here whose snapshots all come from this host warn of nothing
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 2 snapshots of the plan "home"
    And the repository holds 2 snapshots no plan owns
    When the owner lists the repository's groups
    Then the groups warn of no other host

  Scenario: Snapshots under the agent's hostname or a plan's host are this host's
    Given this agent's hostname is "demo-laptop"
    And a plan "home" keeping the last 1 snapshot that records the host "demo-old-laptop"
    And the repository is maintained by this agent
    And the repository holds 2 snapshots no plan owns
    And the repository holds 2 snapshots plain rustic made on the host "demo-laptop"
    And the repository holds 2 snapshots plain rustic made on the host "demo-old-laptop"
    When the owner lists the repository's groups
    Then the groups warn of no other host

  Scenario: Snapshots this agent made under a host name it no longer records are this host's
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 2 snapshots this agent's plan "home" made on the host "demo-old-name"
    And the repository holds 1 snapshot another agent's plan "home" made on the host "demo-nas"
    When the owner lists the repository's groups
    Then the groups warn of snapshots from the other hosts "demo-nas"

  Scenario: A repository this agent doesn't maintain warns of no other host
    Given a plan "home" keeping the last 1 snapshot
    And the repository's maintenance is "none"
    And the repository holds 2 snapshots plain rustic made on the host "demo-desktop"
    When this agent lists the repository's groups
    Then the groups warn of no other host

  Scenario: A recent snapshot another agent made under this host's name comes from another machine
    Given this agent's hostname is "demo-pi"
    And a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 2 snapshots this agent's plan "home" made on the host "demo-pi"
    And the repository holds 1 snapshot another agent's plan "home" made on the host "demo-pi" 29 days ago
    When the owner lists the repository's groups
    Then the groups warn of snapshots from the other hosts "demo-pi"

  Scenario: A snapshot another agent made under this host's name more than 30 days ago is this host's
    Given this agent's hostname is "demo-pi"
    And a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 2 snapshots this agent's plan "home" made on the host "demo-pi"
    And the repository holds 1 snapshot another agent's plan "home" made on the host "demo-pi" 31 days ago
    When the owner lists the repository's groups
    Then the groups warn of no other host

  Scenario: A snapshot another agent made exactly 30 days ago is still recent
    Given this agent's hostname is "demo-pi"
    And a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 1 snapshot another agent's plan "home" made on the host "demo-pi" 30 days ago
    When the owner lists the repository's groups
    Then the groups warn of snapshots from the other hosts "demo-pi"

  Scenario: A snapshot another agent made 30 days and 1 second ago is no longer recent
    Given this agent's hostname is "demo-pi"
    And a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 1 snapshot another agent's plan "home" made on the host "demo-pi" 30 days and 1 second ago
    When the owner lists the repository's groups
    Then the groups warn of no other host

  Scenario: An old snapshot another agent made under another host's name still comes from another host
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 1 snapshot another agent's plan "home" made on the host "demo-nas" 400 days ago
    When the owner lists the repository's groups
    Then the groups warn of snapshots from the other hosts "demo-nas"

  Scenario: This agent's own recent snapshots under another host's name are this host's
    Given this agent's hostname is "demo-pi"
    And a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 1 snapshot this agent's plan "home" made on the host "demo-old-pi" 1 day ago
    When the owner lists the repository's groups
    Then the groups warn of no other host

  Scenario: A prune ends warning of another agent's recent snapshots under this host's name
    Given this agent's hostname is "demo-pi"
    And a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 1 snapshot another agent's plan "home" made on the host "demo-pi" 2 days ago
    When the owner prunes the repository
    Then the operation ends as succeeded with warnings
    And the operation warns that its prune ran beside snapshots from the other hosts "demo-pi"

  Scenario: The review of a forget names every other host once, in order
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 2 snapshots plain rustic made on the host "demo-nas"
    And the repository holds 1 snapshot plain rustic made on the host "demo-desktop"
    When the owner previews forgetting the repository
    Then the preview warns of snapshots from the other hosts "demo-desktop, demo-nas"
    And the preview says the plan "home" would lose 2 of its 3 snapshots

  Scenario: The review of an adoption names the other hosts, the recent history's own among them
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 3 snapshots another agent's plan "home" holds
    And the repository holds 1 snapshot plain rustic made on the host "demo-desktop"
    When the owner previews adopting the other agent's "home" history into the plan "home"
    Then the preview warns of snapshots from the other hosts "demo-desktop, demo-host"

  Scenario: The review of a prune names the other hosts
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 1 snapshot plain rustic made on the host "demo-desktop"
    When the owner previews pruning the repository
    Then the preview warns of snapshots from the other hosts "demo-desktop"
    And the preview says the prune would mark 2 packs, delete 1 and bring back 3, freeing 4096 bytes

  Scenario: The review of a check reads nothing and warns of nothing
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 1 snapshot plain rustic made on the host "demo-desktop"
    When the owner previews checking the repository
    Then the preview warns of no other host
    And the engine was never given a job

  Scenario: A forget of a repository maintained here ends warning of the other hosts
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 1 snapshot plain rustic made on the host "demo-desktop"
    When the owner forgets the repository
    Then the operation ends as succeeded with warnings
    And the operation warns that its forget ran beside snapshots from the other hosts "demo-desktop"
    And the repository keeps the 1 newest snapshots of the plan "home"

  Scenario: A prune of a repository maintained here ends warning of the other hosts
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 1 snapshot plain rustic made on the host "demo-desktop"
    When the owner prunes the repository
    Then the operation ends as succeeded with warnings
    And the operation warns that its prune ran beside snapshots from the other hosts "demo-desktop"

  Scenario: An adoption in a repository maintained here ends warning of the other hosts
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 3 snapshots another agent's plan "home" holds
    And the repository holds 1 snapshot plain rustic made on the host "demo-desktop"
    When the owner adopts the other agent's "home" history into the plan "home"
    Then the operation ends as succeeded with warnings
    And the operation warns that its adoption ran beside snapshots from the other hosts "demo-desktop, demo-host"

  Scenario: A forget of a repository whose snapshots all come from this host ends without a warning
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 3 snapshots of the plan "home"
    When the owner forgets the repository
    Then the operation ends as succeeded
    And the operation lists 0 warnings

  Scenario: A check of a repository maintained here warns of nothing
    Given a plan "home" keeping the last 1 snapshot
    And the repository is maintained by this agent
    And the repository holds 1 snapshot plain rustic made on the host "demo-desktop"
    When the owner checks the repository
    Then the operation ends as succeeded
    And the operation lists 0 warnings
