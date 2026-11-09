@repositories
Feature: Connecting a repository
  The agent initialises a repository where nothing is yet, tests one that is
  there, and connects one by saving it into its configuration once its test
  passed. A repository is never overwritten, an alias is never reused, and
  what the configuration keeps is the settings as written: a secret stays a
  reference, never the value it resolved to.

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
