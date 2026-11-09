Feature: One maintenance operation at a time on a repository
  Prune and check work on a repository's packs, and rustic doesn't promise
  that they cope with each other, so a repository runs one maintenance
  operation at a time (invariant I7). The operation log decides when an
  operation starts, in the one step that records it, so two starts that race
  for a repository can't both win. An operation that is refused isn't
  recorded as having run.

  Scenario Outline: A <next> is refused while a <running> runs on its repository
    Given a <running> of the repository "nas" is running
    When a <next> of the repository "nas" tries to start
    Then the start is refused
    And the refusal names the running operation
    And the history doesn't list the operation that tried to start

    Examples:
      | running | next  |
      | prune   | prune |
      | prune   | check |
      | check   | prune |
      | check   | check |

  Scenario: A refused operation stays queued
    Given a prune of the repository "nas" is running
    And a check of the repository "nas" is queued
    When the queued check tries to start
    Then the start is refused
    And the history still shows the queued check as queued
    And the history still shows the running prune as running

  Scenario: Maintenance on another repository isn't held up
    Given a prune of the repository "nas" is running
    When a prune of the repository "laptop" tries to start
    Then the start is allowed

  Scenario Outline: A <other> isn't held up by maintenance on its repository
    Given a prune of the repository "nas" is running
    When a <other> of the repository "nas" tries to start
    Then the start is allowed

    Examples:
      | other    |
      | backup   |
      | identify |

  Scenario Outline: Maintenance isn't held up by a <other> on its repository
    Given a <other> of the repository "nas" is running
    When a prune of the repository "nas" tries to start
    Then the start is allowed

    Examples:
      | other    |
      | backup   |
      | identify |

  Scenario Outline: A repository is free again once its maintenance operation <ending>
    Given a prune of the repository "nas" is running
    And the running operation <ending>
    When a check of the repository "nas" tries to start
    Then the start is allowed

    Examples:
      | ending                 |
      | succeeds               |
      | succeeds with warnings |
      | fails                  |
      | is cancelled           |

  Scenario: A queued maintenance operation doesn't hold its repository
    Given a prune of the repository "nas" is queued
    When a check of the repository "nas" tries to start
    Then the start is allowed

  Scenario: A repeated start of the running operation isn't refused
    Given a prune of the repository "nas" is running
    When the running prune tries to start again
    Then the start is allowed

  Scenario: Of two starts at the same moment only one wins
    When a prune and a check of the repository "nas" try to start at the same moment
    Then exactly one of them is started
    And the other is refused, naming the one that started
