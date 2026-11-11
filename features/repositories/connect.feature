@repositories
Feature: Connecting a repository
  The agent initialises a repository where nothing is yet, tests one that is
  there, and connects one by saving it into its configuration once its test
  passed. A repository is never overwritten, an alias is never reused, a
  repository has one alias, and what the configuration keeps is the settings
  as written: a secret stays a reference, never the value it resolved to. A
  repository created here is maintained by this agent; one that is connected
  is maintained by it only when the person chose so, for only one party may
  maintain a repository and another may already do it (I1, §7.3).

  Scenario: Testing a repository reports its ID and saves nothing
    Given a repository the engine identifies
    When the agent tests the repository
    Then the test reports the repository's ID
    And the agent has no repository configured

  Scenario: Connecting a repository saves it once its test passed
    Given a repository the engine identifies
    When the agent connects it as "demo"
    Then the agent has the repository "demo" configured with its settings

  Scenario: Connecting with an alias already used is refused
    Given a repository the engine identifies
    And the agent has the repository "demo" configured with its settings
    When the agent connects it as "demo"
    Then the connection is refused because the alias is taken
    And the agent has one repository configured

  Scenario: Connecting a repository the agent has under another name is refused
    Given a repository the engine identifies
    And the agent has the repository "demo" configured with its settings
    When the agent connects it as "other"
    Then the request is refused because the repository is configured as "demo"
    And the agent has one repository configured

  Scenario: Connecting a repository by another location is refused when its ID is configured under another name
    Given a repository the engine identifies
    And the agent connected it as "demo"
    When the agent connects it at another location as "other"
    Then the request is refused because the repository is configured as "demo"
    And the agent has one repository configured

  Scenario: Connecting a repository keeps its ID in the configuration
    Given a repository the engine identifies
    When the agent connects it as "demo"
    Then the agent has the repository "demo" configured with the repository's ID

  Scenario: Initialising where the agent has a repository under another name is refused
    Given a repository the engine identifies
    And the agent has the repository "demo" configured with its settings
    When the agent initialises the repository as "other"
    Then the request is refused because the repository is configured as "demo"
    And the agent has one repository configured

  Scenario: Initialising keeps the new repository's ID in the configuration
    Given a repository the engine identifies
    When the agent initialises the repository as "demo"
    Then the agent has the repository "demo" configured with the repository's ID

  Scenario: Connecting a repository whose test fails leaves the config unchanged
    Given a repository the engine cannot open
    When the agent connects it as "demo"
    Then the connection fails
    And the agent has no repository configured

  Scenario: Initialising saves the repository only when an alias is given
    Given a repository the engine identifies
    When the agent initialises the repository
    Then the initialisation reports the repository's ID
    And the agent has no repository configured
    When the agent initialises the repository as "demo"
    Then the agent has the repository "demo" configured with its settings

  Scenario: Creating a repository under a name asks this agent to maintain it
    Given a repository the engine identifies
    When the agent initialises the repository as "demo"
    Then the repository "demo" asks this agent to maintain it

  Scenario: Connecting a repository asks this agent to maintain it only when the person chose so
    Given a repository the engine identifies
    When the agent connects it as "demo"
    Then the repository "demo" asks nobody to maintain it
    When the agent connects it as "other" to maintain it from this agent
    Then the repository "other" asks this agent to maintain it

  Scenario: Testing a repository that never answers is refused once its time is up
    Given a repository the engine never answers
    When the agent tests the repository
    Then the request is refused because the repository did not answer in time
    And the request gave up when the repository job's time was up
    And the repository's worker was stopped
    And the agent has no repository configured

  Scenario: Connecting a repository that never answers is refused and saves nothing
    Given a repository the engine never answers
    When the agent connects it as "demo"
    Then the request is refused because the repository did not answer in time
    And the repository's worker was stopped
    And the agent has no repository configured

  Scenario: Initialising a repository that never answers is refused and saves nothing
    Given a repository the engine never answers
    When the agent initialises the repository as "demo"
    Then the request is refused because the repository did not answer in time
    And the repository's worker was stopped
    And the agent has no repository configured

  @e2e
  Scenario: Initialising a new local repository gives it an ID
    Given an empty repository location
    When the agent initialises the repository
    Then the initialisation reports the repository's ID
    And the repository at the location opens with the password

  @e2e
  Scenario: Initialising where a repository already exists is refused, and the repository is unchanged
    Given an initialised repository
    When the agent initialises the repository
    Then the initialisation is refused because a repository already exists
    And the repository is unchanged

  @e2e
  Scenario: A new hot/cold repository is initialised in both halves
    Given an empty hot/cold repository location
    When the agent initialises the repository
    Then both halves hold the repository's config

  @e2e
  Scenario: Testing a repository with the right password reports its ID and saves nothing
    Given an initialised repository
    When the agent tests the repository
    Then the test reports the repository's ID
    And the agent has no repository configured

  @e2e
  Scenario: Testing a repository with a wrong password fails and saves nothing
    Given an initialised repository
    And the agent uses the wrong password
    When the agent tests the repository
    Then the test fails
    And the agent has no repository configured

  @e2e
  Scenario: Testing a location with no repository fails and saves nothing
    Given an empty repository location
    When the agent tests the repository
    Then the test fails
    And the agent has no repository configured

  @e2e
  Scenario: Connecting a repository adds it to the config, and the config file lists it
    Given an initialised repository
    When the agent connects it as "demo"
    Then the saved config file gives the repository "demo" the "repository" "{scratch}/repo"
    And the agent has the repository "demo" configured with its settings

  @e2e
  Scenario: Secrets stay references in the saved config
    Given an initialised repository
    And the repository's password is in a file
    When the agent connects it as "demo"
    Then the saved config file doesn't hold "demo-password"
    And the saved config file gives the repository "demo" the "password-file" "{scratch}/password.txt"
