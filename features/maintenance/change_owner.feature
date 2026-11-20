Feature: Turning maintenance from this agent on or off for a saved repository
  Exactly one party maintains a repository (I1). A person turns on
  maintenance from this agent for a repository already saved, as connecting
  it with "Maintain this repository from this machine" would, or turns it off,
  and only that one setting of the entry changes. Without a hub nothing can
  tell whether another agent maintains it, so the person turns it on knowing
  only one place may (§7.3). Turned off, this agent forgets, prunes, checks
  and adopts nothing of the repository, so it is never turned off while a
  forget, a prune, a check or an adoption of it is running here: the person
  could start one on another machine while this one still runs.

  Scenario Outline: Turning maintenance on asks this agent to maintain a repository that asks for "<maintenance>"
    Given the repository "demo" is saved asking for "<maintenance>" maintenance
    When maintenance of "demo" from this agent is turned on
    Then the repository "demo" asks this agent to maintain it
    And the rest of the entry "demo" is as it was saved

    Examples:
      | maintenance |
      | none        |
      | hub         |

  Scenario: Turning maintenance off asks nobody to maintain the repository
    Given the repository "demo" is saved asking for "local" maintenance
    When maintenance of "demo" from this agent is turned off
    Then the repository "demo" asks nobody to maintain it
    And the rest of the entry "demo" is as it was saved

  Scenario Outline: Turning maintenance <turned> when it already is changes nothing
    Given the repository "demo" is saved asking for "<maintenance>" maintenance
    When maintenance of "demo" from this agent is turned <turned>
    Then the repository "demo" asks for "<maintenance>" maintenance
    And the configuration was not written again

    Examples:
      | maintenance | turned |
      | local       | on     |
      | none        | off    |
      | hub         | off    |

  Scenario Outline: Turning maintenance off while <running> of the repository is running is refused
    Given the repository "demo" is saved asking for "local" maintenance
    And <running> of the repository "demo" is running here
    When maintenance of "demo" from this agent is turned off
    Then it is refused because that maintenance operation is running
    And the repository "demo" asks this agent to maintain it

    Examples:
      | running     |
      | a forget    |
      | a prune     |
      | a check     |
      | an adoption |

  Scenario: A maintenance operation running on another repository doesn't stop maintenance from being turned off
    Given the repository "demo" is saved asking for "local" maintenance
    And a prune of another repository is running
    When maintenance of "demo" from this agent is turned off
    Then the repository "demo" asks nobody to maintain it

  Scenario: Turning maintenance off for a repository whose ID was never recorded is refused while any maintenance operation runs here
    Given the repository "demo" is saved asking for "local" maintenance, with no ID recorded
    And a prune of another repository is running
    When maintenance of "demo" from this agent is turned off
    Then it is refused because that maintenance operation is running
    And the repository "demo" asks this agent to maintain it

  Scenario: A backup running on the repository doesn't stop maintenance from being turned off
    Given the repository "demo" is saved asking for "local" maintenance
    And a backup to the repository "demo" is running
    When maintenance of "demo" from this agent is turned off
    Then the repository "demo" asks nobody to maintain it

  Scenario Outline: Turning maintenance <turned> for a repository the configuration doesn't hold is refused
    Given the repository "demo" is saved asking for "local" maintenance
    When maintenance of "other" from this agent is turned <turned>
    Then it is refused because the configuration holds no repository "other"
    And the configuration was not written again

    Examples:
      | turned |
      | on     |
      | off    |
