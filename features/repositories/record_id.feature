@repositories
Feature: Recording a saved repository's ID
  An entry written by hand or imported from a profile records no repository
  ID, since Ladderback never opened the repository it names. The first job
  that opens it, of whatever kind, has the agent record the ID of the
  repository it opened in that entry, and in nothing else, so the agent
  knows from then on that a repository was reached there and never offers
  to create one in its place. A record that can't be made changes nothing
  about the job, and the next job that opens the repository tries again.

  Rule: The first job that opens a saved repository records its ID in the entry

    Scenario Outline: A <job> of a repository whose entry records no ID records the ID it opened
      Given the agent has the repository "demo" configured with its settings
      When a <job> of the repository "demo" opens it
      Then the agent has the repository "demo" configured with the repository's ID
      And the <job> ended as the engine reported it

      Examples:
        | job              |
        | backup           |
        | check            |
        | prune            |
        | snapshot listing |

  Rule: An entry that records an ID is never written

    Scenario: A check of a repository whose entry records an ID leaves the entry as it was
      Given the agent has the repository "demo" configured with another repository's ID
      When a check of the repository "demo" opens it
      Then the agent has the repository "demo" configured with another repository's ID

  Rule: An entry changed after the job read it records nothing until the next job opens it

    Scenario: An entry edited while a check ran records the ID only when the next check opens it
      Given the agent has the repository "demo" configured with its settings
      And a check of the repository "demo" read its entry
      And the repository "demo" was set by hand to be maintained from this agent
      When that check opens the repository
      Then the agent has the repository "demo" configured with no repository ID
      And the repository "demo" asks this agent to maintain it
      When a check of the repository "demo" opens it
      Then the agent has the repository "demo" configured with the repository's ID

  Rule: Two entries never record the same repository

    Scenario: A second entry for a repository another entry records is left without an ID
      Given the agent has the repository "demo" configured with its settings, and "mounted" at another location
      And a check of the repository "demo" opened it
      When a check of the repository "mounted" opens it
      Then the agent has the repository "mounted" configured with no repository ID

  Rule: A saved repository a job has opened is never offered to be created

    Scenario: A repository a check opened is not offered to be created after a backup found nothing there
      Given the agent has the repository "demo" configured with its settings, backed up to by the plan "demo-home"
      And a check of the repository "demo" opened it
      And the latest operation of the plan "demo-home" failed because no repository is there
      When the agent decides which saved repositories to offer to create
      Then the repository "demo" is not offered to be created
