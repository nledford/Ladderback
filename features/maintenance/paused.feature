Feature: A paired machine says when its maintenance is paused for want of a lease
  A paired machine maintains a repository only while it holds a live lease
  on it from its hub (technical design §7.3, I1). While it holds none on a
  repository it asks to maintain, its maintenance of that repository is
  paused, and the repository's state says since when: since its lease ran
  out, or since the machine first saw it held none: from the moment it
  turns maintenance on, so a repository its hub leases to another machine
  shows paused at once, not only once the next watch notes it (#1671), or
  else from the watch that first sees it. Once a pause has lasted
  14 days the machine tells its notification rules, once, with
  `repo.maintenance_paused`. The next grant clears it. A run asked for by
  hand is refused with `maintenance-no-lease`, as before.

  Background:
    Given a hub that approved the machines "laptop" and "nas"

  Scenario: A lease that runs out shows maintenance paused since it ran out
    Given "laptop" claims the repository "photos" and hears the hub's grants
    And "laptop" can't reach the hub any more
    When 9 days pass on the hub
    Then maintenance of "photos" on "laptop" is paused since its lease ran out 2 days ago

  Scenario: A renewed lease clears the paused state
    Given "laptop" claims the repository "photos" and hears the hub's grants
    And "laptop" can't reach the hub any more
    When 9 days pass on the hub
    And "laptop" claims the repository "photos" and hears the hub's grants
    Then maintenance of "photos" on "laptop" isn't paused

  Scenario: A run asked for by hand still answers maintenance-no-lease while maintenance is paused
    Given "laptop" claims the repository "photos" and hears the hub's grants
    And "laptop" can't reach the hub any more
    When 9 days pass on the hub
    Then "laptop" may not maintain "photos", because its lease ran out
    And maintenance of "photos" on "laptop" is paused since its lease ran out 2 days ago

  Scenario: A machine that never heard a grant is paused from the moment it sees it holds none
    Given "laptop" asks to maintain "photos"
    And "laptop" is paired with the hub and has heard no grant from it
    When "laptop" watches its maintenance
    And 2 days pass on the hub
    Then maintenance of "photos" on "laptop" is paused since 2 days ago

  Scenario: A machine that turns maintenance on for a repository its hub leases to another machine is paused at once, before any watch
    Given "nas" claims the repository "photos" and hears the hub's grants
    And "laptop" keeps "photos" without maintaining it
    And "laptop" claims the repository "photos" and hears the hub's grants
    When "laptop" turns on maintenance of "photos"
    Then "laptop" may not maintain "photos", because it holds no lease on it
    And maintenance of "photos" on "laptop" is paused since now
    When 2 days pass on the hub
    And "laptop" watches its maintenance
    Then maintenance of "photos" on "laptop" is paused since 2 days ago

  Scenario: A pause is told through the notification rules once it has lasted 14 days, and only once
    Given "laptop" asks to maintain "photos", with a rule that sends "repo.maintenance_paused" to the webhook channel "ops"
    And "laptop" claims the repository "photos" and hears the hub's grants
    And "laptop" can't reach the hub any more
    When 20 days pass on the hub
    And "laptop" watches its maintenance
    Then "laptop" sent no notification
    When a day passes on the hub
    And "laptop" watches its maintenance
    Then "ops" was sent "repo.maintenance_paused" for the repository "photos", paused since 14 days ago
    When a day passes on the hub
    And "laptop" watches its maintenance
    Then "ops" was sent "repo.maintenance_paused" only once
