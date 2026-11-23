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
