Feature: A maintenance preview tells what a run would do before it does anything
  A person reviews a forget or an adoption before it runs (§7.3). A preview
  reads the repository the way a snapshot listing does: it takes no
  maintenance slot, writes nothing to the operation log and asks the engine
  to change nothing. It names the snapshots a run would act on and the
  history the run leaves alone (I6), and gives a selection: a run started
  with that selection is refused, before anything is removed or rewritten,
  when the snapshots it would act on are not the ones the preview showed
  (I1). The groups say who owns each snapshot of a repository and which
  plans could take the history no plan owns.

  Scenario: A forget preview names what the plan's retention would remove
    Given a plan "home" keeping the last 2 snapshots
    And the repository holds 4 snapshots of the plan "home"
    When the owner previews forgetting the repository
    Then the preview says the plan "home" would lose 2 of its 4 snapshots
    And the preview names the 2 oldest snapshots of the plan "home"
    And the engine was never asked to remove anything

  Scenario: Each plan's forget preview follows its own policy
    Given a plan "home" keeping the last 2 snapshots
    And a plan "photos" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 3 snapshots of the plan "photos"
    When the owner previews forgetting the repository
    Then the preview says the plan "home" would lose 1 of its 3 snapshots
    And the preview says the plan "photos" would lose 2 of its 3 snapshots
    And the preview lists the plans "home, photos" in that order

  Scenario: A forget preview names the history it leaves alone
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository holds 2 snapshots another agent's plan "home" holds
    And the repository holds 2 snapshots no plan owns
    When the owner previews forgetting the repository
    Then the preview says it leaves alone 1 group of orphaned history holding 2 snapshots
    And the preview says it leaves alone 1 group of unmanaged history holding 2 snapshots
    And the engine was never asked to remove anything

  Scenario: A forget preview of history every plan owns leaves nothing alone
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner previews forgetting the repository
    Then the preview says it leaves nothing alone

  Scenario: A forget preview refuses a policy that keeps nothing
    Given a plan "home" keeping nothing
    And the repository holds 3 snapshots of the plan "home"
    When the owner previews forgetting the repository
    Then the preview is refused because the plan's policy keeps nothing
    And the engine was never asked to remove anything

  Scenario: A forget preview ignores a plan that keeps nothing and has no history
    Given a plan "home" keeping the last 1 snapshot
    And a plan "empty" keeping nothing that has never backed up
    And the repository holds 3 snapshots of the plan "home"
    When the owner previews forgetting the repository
    Then the preview says the plan "home" would lose 2 of its 3 snapshots
    And the preview lists the plans "home" in that order

  Scenario: A forget preview takes no slot and logs nothing
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And a maintenance operation is already running on the repository
    When the owner previews forgetting the repository
    Then the preview says the plan "home" would lose 2 of its 3 snapshots
    And the log holds no operation of this preview
    And the log holds only the operation that was already running

  Scenario: A forget preview of a repository that is not configured is refused
    When the owner previews forgetting the repository
    Then the preview is refused because the repository is not configured

  Scenario: A forget preview of a repository that cannot be reached is refused
    Given a plan "home" keeping the last 1 snapshot
    And the engine cannot be reached
    When the owner previews forgetting the repository
    Then the preview is refused because the engine cannot be reached

  Scenario: A forget preview of a repository that never answers is refused
    Given a plan "home" keeping the last 1 snapshot
    And the repository never answers
    When the owner previews forgetting the repository
    Then the preview is refused because the repository did not answer in time

  Scenario: A forget confirmed with its preview's selection removes exactly what the preview named
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner previews forgetting the repository
    And the owner forgets the repository with the preview's selection
    Then the operation ends as succeeded
    And the forget removed exactly the snapshots the preview named
    And the log shows the forget removed 2 snapshots

  Scenario: A forget still runs when only history no plan owns changed since its preview
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner previews forgetting the repository
    And the repository gains 2 snapshots another agent's plan "home" holds
    And the owner forgets the repository with the preview's selection
    Then the operation ends as succeeded
    And the forget removed exactly the snapshots the preview named

  Scenario: A forget confirmed after its plan's snapshots changed is refused and removes nothing
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner previews forgetting the repository
    And the repository gains 1 snapshot of the plan "home"
    And the owner forgets the repository with the preview's selection
    Then the run is refused because the snapshots changed since the preview
    And the engine was never asked to remove anything
    And the log holds no operation of this forget

  Scenario: An adoption preview names the orphaned history it would rewrite
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots another agent's plan "home" holds
    When the owner previews adopting the other agent's "home" history into the plan "home"
    Then the preview says 3 snapshots would be rewritten into the plan "home"
    And the preview names the snapshots the other agent held
    And the engine was never asked to rewrite anything
    And the log holds no operation of this preview

  Scenario: An adoption preview names the unmanaged history it would rewrite
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 2 snapshots no plan owns that match the plan "home"
    When the owner previews adopting the unmanaged history into the plan "home"
    Then the preview says 2 snapshots would be rewritten into the plan "home"
    And the preview names the snapshots no plan owned
    And the engine was never asked to rewrite anything

  Scenario: An adoption preview refuses an orphaned group nobody holds
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    When the owner previews adopting the other agent's "home" history into the plan "home"
    Then the preview is refused because no plan matches that history

  Scenario: An adoption preview refuses history no plan matches
    Given a plan "photos" keeping the last 1 snapshot
    And the repository holds 2 snapshots no plan owns of another folder
    When the owner previews adopting the unmanaged history into the plan "photos"
    Then the preview is refused because no plan matches that history
    And the engine was never asked to rewrite anything

  Scenario: An adoption preview refuses history two plans match
    Given a plan "home" keeping the last 1 snapshot
    And a plan "mirror" keeping the last 1 snapshot that backs up the same sources
    And the repository holds 2 snapshots no plan owns that match the plan "home"
    When the owner previews adopting the unmanaged history into the plan "home"
    Then the preview is refused because two plans match that history

  Scenario: An adoption preview refuses a plan that is not configured
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 2 snapshots another agent's plan "home" holds
    When the owner previews adopting the other agent's "home" history into the plan "ghost"
    Then the preview is refused because the plan "ghost" is not configured
    And the engine was never asked to rewrite anything

  Scenario: An adoption confirmed with its preview's selection rewrites exactly what the preview named
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots another agent's plan "home" holds
    When the owner previews adopting the other agent's "home" history into the plan "home"
    And the owner adopts the other agent's "home" history into the plan "home" with the preview's selection
    Then the operation ends as succeeded
    And the 3 snapshots are rewritten with the tags of the plan "home"
    And the snapshots the other agent held are gone

  Scenario: An adoption confirmed after its history changed is refused and rewrites nothing
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots another agent's plan "home" holds
    When the owner previews adopting the other agent's "home" history into the plan "home"
    And the repository gains 1 snapshot another agent's plan "home" holds
    And the owner adopts the other agent's "home" history into the plan "home" with the preview's selection
    Then the run is refused because the snapshots changed since the preview
    And the engine was never asked to rewrite anything
    And the log holds no operation of this adoption

  Scenario: A check preview reads nothing and changes nothing
    Given a plan "home" keeping the last 1 snapshot
    When the owner previews checking the repository
    Then the preview says the check does not read the repository's data
    And the engine was never given a job

  Scenario: A prune preview reports what the engine's plan would do and runs no prune
    Given a plan "home" keeping the last 1 snapshot
    When the owner previews pruning the repository
    Then the preview says the prune would mark 2 packs, delete 1 and bring back 3, freeing 4096 bytes
    And the preview says marked packs wait the default time before a later prune deletes them
    And the engine was only asked to plan a prune
    And the log holds no operation of this preview

  Scenario: A prune preview takes no slot
    Given a plan "home" keeping the last 1 snapshot
    And a maintenance operation is already running on the repository
    When the owner previews pruning the repository
    Then the preview says the prune would mark 2 packs, delete 1 and bring back 3, freeing 4096 bytes
    And the log holds only the operation that was already running

  Scenario: A prune preview of a repository that is not configured is refused
    When the owner previews pruning the repository
    Then the preview is refused because the repository is not configured
    And the engine was never given a job

  Scenario: The groups list every plan of the repository, with history or without
    Given a plan "home" keeping the last 1 snapshot
    And a plan "photos" keeping the last 1 snapshot of the folder "/srv/photos"
    And the repository holds 3 snapshots of the plan "home"
    When the owner lists the repository's groups
    Then the groups list the plans "home, photos" in that order
    And the groups list the plan "home" with 3 snapshots
    And the groups list the plan "photos" with no snapshots
    And the groups list no orphaned history
    And the groups list no unmanaged history

  Scenario: The groups list a plan's snapshots newest first
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 4 snapshots of the plan "home"
    When the owner lists the repository's groups
    Then the groups list the plan "home" with 4 snapshots, newest first

  Scenario: The groups list the history no plan of this agent owns
    Given a plan "home" keeping the last 1 snapshot
    And a plan "photos" keeping the last 1 snapshot of the folder "/srv/photos"
    And the repository holds 2 snapshots of the plan "home"
    And the repository holds 3 snapshots another agent's plan "home" holds
    When the owner lists the repository's groups
    Then the groups list an orphaned group of 3 snapshots that can be adopted into "home, photos"
    And the groups list the plan "home" with 2 snapshots

  Scenario: The groups name the plan that would adopt unmanaged history
    Given a plan "home" keeping the last 1 snapshot
    And a plan "photos" keeping the last 1 snapshot of the folder "/srv/photos"
    And the repository holds 2 snapshots no plan owns that match the plan "home"
    When the owner lists the repository's groups
    Then the groups list an unmanaged group of 2 snapshots that can be adopted into "home"

  Scenario: The groups name every plan that matches unmanaged history
    Given a plan "home" keeping the last 1 snapshot
    And a plan "mirror" keeping the last 1 snapshot of the folder "/home/demo"
    And the repository holds 2 snapshots no plan owns that match the plan "home"
    When the owner lists the repository's groups
    Then the groups list an unmanaged group of 2 snapshots that can be adopted into "home, mirror"

  Scenario: The groups name no plan for unmanaged history no plan matches
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 2 snapshots no plan owns of another folder
    When the owner lists the repository's groups
    Then the groups list an unmanaged group of 2 snapshots that no plan would adopt

  Scenario: The groups of a repository that is not configured are refused
    When the owner lists the repository's groups
    Then the groups are refused because the repository is not configured

  Scenario: The groups of a repository that cannot be reached are refused
    Given a plan "home" keeping the last 1 snapshot
    And the engine cannot be reached
    When the owner lists the repository's groups
    Then the groups are refused because the engine cannot be reached

  Scenario: The groups of a repository that never answers are refused
    Given a plan "home" keeping the last 1 snapshot
    And the repository never answers
    When the owner lists the repository's groups
    Then the groups are refused because the repository did not answer in time

  Scenario: A forget preview of a repository that lists its snapshots and then stalls is refused
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository lists its snapshots but never answers a read of them for retention
    When the owner previews forgetting the repository
    Then the preview is refused because the repository did not answer in time
    And the engine was never asked to remove anything

  Scenario: A forget of a repository that lists its snapshots and then stalls is refused
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And the repository lists its snapshots but never answers a read of them for retention
    When the owner forgets the repository
    Then the forget is refused because the repository did not answer in time
    And the log holds no operation of this forget

  Scenario: The groups still list when the sources of one plan cannot be read
    Given a plan "home" keeping the last 1 snapshot
    And a plan "photos" keeping the last 1 snapshot of the folder "/mnt/gone"
    And the sources of the folder "/mnt/gone" cannot be read
    And the repository holds 2 snapshots of the plan "home"
    And the repository holds 3 snapshots another agent's plan "home" holds
    And the repository holds 2 snapshots no plan owns that match the plan "home"
    When the owner lists the repository's groups
    Then the groups list the plans "home, photos" in that order
    And the groups list the plan "home" with 2 snapshots
    And the groups list an orphaned group of 3 snapshots that can be adopted into "home, photos"
    And the groups list an unmanaged group of 2 snapshots that can be adopted into "home"

  Scenario: An adoption preview is not blocked by another plan whose sources cannot be read
    Given a plan "home" keeping the last 1 snapshot
    And a plan "photos" keeping the last 1 snapshot of the folder "/mnt/gone"
    And the sources of the folder "/mnt/gone" cannot be read
    And the repository holds 2 snapshots no plan owns that match the plan "home"
    When the owner previews adopting the unmanaged history into the plan "home"
    Then the preview says 2 snapshots would be rewritten into the plan "home"

  Scenario: An adoption preview into a plan whose sources cannot be read is refused
    Given a plan "home" keeping the last 1 snapshot
    And a plan "photos" keeping the last 1 snapshot of the folder "/mnt/gone"
    And the sources of the folder "/mnt/gone" cannot be read
    And the repository holds 2 snapshots no plan owns that match the plan "home"
    When the owner previews adopting the unmanaged history into the plan "photos"
    Then the preview is refused because the engine cannot be reached

  Scenario: The groups offer no plan for orphaned history that only an agent tag marks
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 2 snapshots only another agent's tag marks
    When the owner lists the repository's groups
    Then the groups list an orphaned group of 2 snapshots that no plan could adopt

  Scenario: The groups offer no plan for orphaned history that only a plan tag marks
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 2 snapshots only the plan tag "home" marks
    When the owner lists the repository's groups
    Then the groups list an orphaned group of 2 snapshots that no plan could adopt

  Scenario: An adoption confirmed into another plan than the one previewed is refused and rewrites nothing
    Given a plan "home" keeping the last 1 snapshot
    And a plan "photos" keeping the last 1 snapshot of the folder "/srv/photos"
    And the repository holds 3 snapshots another agent's plan "home" holds
    When the owner previews adopting the other agent's "home" history into the plan "home"
    And the owner adopts the other agent's "home" history into the plan "photos" with the preview's selection
    Then the run is refused because the snapshots changed since the preview
    And the engine was never asked to rewrite anything
    And the log holds no operation of this adoption
