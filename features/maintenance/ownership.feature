Feature: The hub gives each repository at most one maintenance owner
  Among the machines paired to a hub, each repository, known by its rustic
  repository ID, has at most one maintenance owner (I1, technical design
  §7.3). A machine claims a repository by reporting it with the request
  `local`. The hub grants the first claim a lease of 7 days, numbered by an
  epoch that only rises, and renews it each time its holder reports the
  repository again. Another machine's claim is refused while the lease
  holds. A lease nobody renews runs out, and an hour later the hub may grant
  the repository to another machine, so a holder whose clock drifted a
  little has stopped first. A lease moves sooner only when its holder
  releases it. A machine maintains only a repository no other machine uses:
  once a second machine reports it, the holder's lease isn't renewed, and
  nobody maintains it until a person makes the hub its owner. Every time is
  the hub's. A machine waiting for approval is granted nothing and keeps no
  other machine from a lease (§16, decision 120, "What pending means").

  A paired machine maintains a repository (a forget, a prune, a check or an
  adoption) only while it holds the hub's lease on it, and judges when the
  lease runs out on the hub's time, which each grant carries, never on its
  own clock. A paired machine that holds no lease, because the hub hasn't
  granted one yet or can't be reached, maintains nothing and says why; its
  backups and previews run all the same. A machine that isn't paired
  maintains what it asks for, as before. A machine that lost its hub and
  pairs with a new one forgets the leases and the highest grants the old
  hub gave it, whose epochs the new hub's don't follow; one that pairs
  again with the hub it pins keeps them.

  After any start, a restart or a restore of its database from a backup,
  the hub moves no repository to a new owner until every machine that used
  it has reported since the start, or 7 days and an hour have passed since
  the start, counted on a clock that a change of the hub's own time doesn't
  move (§16, decisions 137 and 157). A hub restored from a backup can't see
  a lease it granted after the backup until its holder reports, so it
  grants nobody else the repository meanwhile, and the holder's own report
  gives the lease back to it. Its holder's lease is renewed all the while.

  Background:
    Given a hub that approved the machines "laptop" and "nas"

  Scenario: The first claim for a repository is granted a lease of 7 days
    When "laptop" claims the repository "photos"
    Then "laptop" holds the lease on "photos" at epoch 1, for 7 days from now

  Scenario: Two machines claim one repository at once, and only one is granted it
    When "laptop" and "nas" claim the repository "photos" at the same moment
    Then exactly one of them holds the lease on "photos"
    And the other's claim was refused because that one holds the lease

  Scenario: The holder's report renews its lease
    Given "laptop" holds the lease on "photos"
    When 3 days pass on the hub
    And "laptop" claims the repository "photos"
    Then "laptop" holds the lease on "photos" at epoch 1, for 7 days from now

  Scenario: An owner that disappears loses the lease an hour after it runs out
    Given "laptop" holds the lease on "photos"
    And "laptop" can't reach the hub any more
    When 7 days pass on the hub
    And "nas" claims the repository "photos"
    Then the claim of "nas" was refused because "laptop" holds the lease
    When 1 hour passes on the hub
    And "nas" claims the repository "photos"
    Then "nas" holds the lease on "photos" at epoch 2, for 7 days from now

  Scenario: A hub clock set back never shortens a lease
    Given "laptop" holds the lease on "photos"
    When the hub's clock is set back 2 days
    And "laptop" claims the repository "photos"
    Then the lease of "laptop" on "photos" still runs out 7 days after it was granted

  Scenario: A machine that maintained a repository on its own joins a hub where another holds it
    Given "laptop" holds the lease on "photos"
    And the hub approved the machine "desktop"
    When "desktop" claims the repository "photos"
    Then the claim of "desktop" was refused because "laptop" holds the lease
    When "laptop" claims the repository "photos"
    Then the lease of "laptop" on "photos" wasn't renewed, because another machine uses the repository
    When 7 days and 1 hour pass while "laptop" and "desktop" keep claiming "photos"
    Then nobody holds the lease on "photos"
    And the claim of "desktop" was refused because another machine uses the repository

  Scenario: A repository that holds another machine's snapshots is granted to nobody
    When "laptop" claims the repository "photos", which holds another machine's snapshots
    Then the claim of "laptop" was refused because another machine uses the repository
    And nobody holds the lease on "photos"

  Scenario: A lease moves sooner only when its holder releases it
    Given "laptop" holds the lease on "photos"
    When "laptop" stops using "photos"
    And "nas" claims the repository "photos"
    Then the claim of "nas" was refused because "laptop" holds the lease
    When "laptop" releases its lease on "photos" at epoch 1
    And "nas" claims the repository "photos"
    Then "nas" holds the lease on "photos" at epoch 2, for 7 days from now

  Scenario Outline: A release of a lease the machine doesn't hold changes nothing
    Given "laptop" holds the lease on "photos"
    When "<machine>" releases its lease on "photos" at epoch <epoch>
    Then the release was refused
    And "laptop" holds the lease on "photos" at epoch 1

    Examples:
      | machine | epoch |
      | nas     | 1     |
      | laptop  | 2     |

  Scenario: A machine waiting for approval is granted nothing and keeps nobody from a lease
    Given the machine "desktop" is waiting for the hub's approval
    When "desktop" claims the repository "photos"
    Then the hub decided nothing of the report of "desktop"
    When "laptop" claims the repository "photos"
    Then "laptop" holds the lease on "photos" at epoch 1, for 7 days from now

  Scenario: A machine the hub doesn't know is granted nothing
    Given the machine "desktop" isn't known to the hub
    When "desktop" claims the repository "photos"
    Then the hub decided nothing of the report of "desktop"
    And nobody holds the lease on "photos"

  Scenario: A paired machine the hub hasn't granted a lease maintains nothing
    Given "laptop" is paired with the hub and has heard no grant from it
    Then "laptop" may not maintain "photos", because it holds no lease on it

  Scenario: A machine that isn't paired maintains what it asks for
    Given "laptop" isn't paired with a hub
    Then "laptop" may maintain "photos"

  Scenario: A machine maintains a repository while the hub's lease holds
    When "laptop" claims the repository "photos" and hears the hub's grants
    Then "laptop" may maintain "photos"

  Scenario: A lease runs out on the hub's time, whatever the machine's clock says
    Given the clock of "laptop" runs 3 days behind the hub's
    And "laptop" claims the repository "photos" and hears the hub's grants
    And "laptop" can't reach the hub any more
    When 7 days pass on the hub
    Then "laptop" may not maintain "photos", because its lease ran out

  Scenario: Two paired machines never both maintain one repository, across a lost connection and skewed clocks
    Given the clock of "laptop" runs 3 days behind the hub's
    And the clock of "nas" runs 2 days ahead of the hub's
    And "laptop" claims the repository "photos" and hears the hub's grants
    And "laptop" can't reach the hub any more
    When "nas" claims the repository "photos" and hears the hub's grants every hour for 8 days
    Then "laptop" and "nas" never both may maintain "photos"
    And "nas" may maintain "photos"

  Scenario: A machine refuses a grant below the highest it has seen
    Given "laptop" heard a grant of "photos" at epoch 3
    When the hub grants "laptop" the repository "photos" at epoch 2
    Then "laptop" refused the grant of "photos" as below the highest it has seen
    And "laptop" may not maintain "photos", because it holds no lease on it

  Scenario: A machine paired with a new hub forgets what its old hub granted
    Given "laptop" heard a grant of "photos" at epoch 3
    And "laptop" lost its hub and holds no pin
    When "laptop" pairs with a new hub
    Then "laptop" may not maintain "photos", because it holds no lease on it
    When the new hub grants "laptop" the repository "photos" at epoch 1
    Then "laptop" may maintain "photos"

  Scenario: A machine that pairs again with the hub it pins keeps the highest grant it has seen
    Given "laptop" paired with a hub from its token
    And "laptop" heard a grant of "photos" at epoch 3
    When "laptop" pairs again with that hub from a new token
    And the hub grants "laptop" the repository "photos" at epoch 2
    Then "laptop" refused the grant of "photos" as below the highest it has seen

  Scenario: A machine that stops using a repository releases its lease, and another is granted it
    Given "laptop" claims the repository "photos" and hears the hub's grants
    When "laptop" stops using "photos" and the hub acknowledges its release
    And "nas" claims the repository "photos" and hears the hub's grants
    Then "nas" may maintain "photos"
    And "laptop" may not maintain "photos", because it holds no lease on it

  Scenario: A hub restored from a backup grants no second owner while the machine that used the repository hasn't reported
    Given "laptop" reported the repository "photos" without claiming it
    And 8 days passed on the hub
    And the hub's database is backed up
    And "laptop" claims the repository "photos" and hears the hub's grants
    When the hub is restored from that backup and starts
    And "nas" claims the repository "photos"
    Then the claim of "nas" was refused because the hub has just started
    And nobody holds the lease on "photos"
    And "laptop" may maintain "photos"

  Scenario: A restored hub gives the lease back to the machine that reports it
    Given "laptop" reported the repository "photos" without claiming it
    And the hub's database is backed up
    And "laptop" claims the repository "photos" and hears the hub's grants
    When the hub is restored from that backup and starts
    And "laptop" claims the repository "photos"
    Then "laptop" holds the lease on "photos" at epoch 1, for 7 days from now
    When "laptop" claims the repository "photos" and hears the hub's grants
    Then "laptop" may maintain "photos"

  Scenario: A restored hub grants a repository nobody had reported to nobody until every machine that had reported one has reported again
    Given "laptop" reported the repository "music" without claiming it
    And "nas" reported the repository "music" without claiming it
    And the hub's database is backed up
    And "laptop" claims the repository "photos" and hears the hub's grants
    When the hub is restored from that backup and starts
    And "nas" claims the repository "photos"
    Then the claim of "nas" was refused because the hub has just started
    When "laptop" stops using "photos"
    And "nas" claims the repository "photos"
    Then "nas" holds the lease on "photos" at epoch 1, for 7 days from now

  Scenario: The start window ends for a repository once every machine that used it has reported
    Given "laptop" reported the repository "photos" without claiming it
    And 8 days passed on the hub
    When the hub restarts
    And "nas" claims the repository "photos"
    Then the claim of "nas" was refused because the hub has just started
    When "laptop" stops using "photos"
    And "nas" claims the repository "photos"
    Then "nas" holds the lease on "photos" at epoch 1, for 7 days from now

  Scenario: The holder's lease is renewed while the hub waits for another machine
    Given "nas" reported the repository "photos" without claiming it
    When 7 days pass on the hub
    And "laptop" claims the repository "photos"
    And the hub restarts
    And 3 days pass on the hub
    And "laptop" claims the repository "photos"
    Then "laptop" holds the lease on "photos" at epoch 1, for 7 days from now

  Scenario: A machine that never reports keeps a repository from moving for 7 days and an hour, whatever the hub's clock says
    Given 8 days passed on the hub
    And "laptop" holds the lease on "photos"
    And "laptop" can't reach the hub any more
    When the hub restarts
    And the hub's clock is set forward 8 days
    And "nas" claims the repository "photos"
    Then the claim of "nas" was refused because the hub has just started
    When 7 days pass on the hub
    And "nas" claims the repository "photos"
    Then the claim of "nas" was refused because the hub has just started
    When 1 hour passes on the hub
    And "nas" claims the repository "photos"
    Then "nas" holds the lease on "photos" at epoch 2, for 7 days from now
