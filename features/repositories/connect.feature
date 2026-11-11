@repositories
Feature: Connecting a repository
  The agent initialises a repository where nothing is yet, tests one that is
  there, and connects one by saving it into its configuration once its test
  passed. A repository the configuration already names, such as one an
  imported profile added, is created where its entry says, with the entry's
  own password reference, and only its new ID is recorded in the entry. It
  is created only once a job found no repository where its entry says and
  none ever reached it, and only where the engine, looking just before,
  finds a folder with no repository in it: what the log says may be about
  another location, or a drive since unplugged. One whose entry records an
  ID was reached before and is never created again.
  A repository is never overwritten, an alias is never reused, a repository
  has one alias, and what the configuration keeps is the settings as
  written: a secret stays a reference, never the value it resolved to. A
  repository created here under a name is maintained by this agent; one that
  is connected is maintained by it only when the person chose so, for only
  one party may maintain a repository and another may already do it (I1,
  §7.3).

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

  Scenario: Creating a saved repository initialises it where its entry says, and records only its new ID in the entry
    Given a location the engine finds empty
    And the agent has the repository "demo" configured with its settings, backed up to by the plan "demo-home"
    And the latest operation of the plan "demo-home" failed because no repository is there
    When the agent creates the saved repository "demo"
    Then the creation reports the repository's ID
    And the engine looked at the location, then initialised the repository there, with its saved settings
    And the agent has the repository "demo" configured with the repository's ID
    And the agent has the repository "demo" configured with its settings
    And the repository "demo" asks nobody to maintain it

  Scenario: Creating a saved repository whose folder is not there is refused as not offered, and nothing is created
    Given a location whose folder is not there
    And the agent has the repository "demo" configured with its settings, backed up to by the plan "demo-home"
    And the latest operation of the plan "demo-home" failed because no repository is there
    When the agent creates the saved repository "demo"
    Then the creation is refused because its row doesn't offer it
    And the engine initialised no repository
    And the agent has the repository "demo" configured with no repository ID

  Scenario: Creating a saved repository the engine finds there now is refused as already there, and nothing is created
    Given a repository the engine identifies
    And the agent has the repository "demo" configured with its settings, backed up to by the plan "demo-home"
    And the latest operation of the plan "demo-home" failed because no repository is there
    When the agent creates the saved repository "demo"
    Then the creation is refused because a repository already exists
    And the engine initialised no repository
    And the agent has the repository "demo" configured with no repository ID

  Scenario: Creating a saved repository whose entry records a repository ID is refused, and nothing runs
    Given a repository the engine identifies
    And the agent has the repository "demo" configured with another repository's ID
    When the agent creates the saved repository "demo"
    Then the creation is refused because a repository already exists
    And the engine ran no job
    And the agent has the repository "demo" configured with another repository's ID

  Scenario Outline: Creating a saved repository no job found missing is refused as not offered, and nothing runs
    Given a repository the engine identifies
    And the agent has the repository "demo" configured with its settings, backed up to by the plan "demo-home"
    And <history>
    When the agent creates the saved repository "demo"
    Then the creation is refused because its row doesn't offer it
    And the engine ran no job
    And the agent has the repository "demo" configured with no repository ID

    Examples:
      | history                                                                                           |
      | no operation of the plan "demo-home" is on record                                                 |
      | a backup of the plan "demo-home" succeeded, then the latest failed because no repository is there |
      | the latest operation of the plan "demo-home" succeeded                                            |

  Scenario: Creating a repository the agent has not saved is refused, and nothing runs
    Given a repository the engine identifies
    When the agent creates the saved repository "demo"
    Then the creation is refused because the agent has no repository "demo"
    And the engine ran no job

  Scenario Outline: A saved repository is offered to be created only once a job found no repository there and none reached it
    Given the agent has the repository "demo" configured with its settings, backed up to by the plan "demo-home"
    And <history>
    When the agent decides which saved repositories to offer to create
    Then the repository "demo" <offer>

    Examples:
      | history                                                                                           | offer                        |
      | no operation of the plan "demo-home" is on record                                                 | is not offered to be created |
      | the latest operation of the plan "demo-home" failed because no repository is there                | is offered to be created     |
      | a backup of the plan "demo-home" succeeded, then the latest failed because no repository is there | is not offered to be created |
      | the latest operation of the plan "demo-home" succeeded                                            | is not offered to be created |
      | the latest operation of the plan "demo-home" failed because the daemon stopped                    | is not offered to be created |
      | the latest operation of the plan "demo-home" is still running                                     | is not offered to be created |

  Scenario Outline: A saved repository whose entry records a repository ID is never offered to be created
    Given the agent has the repository "demo" configured with another repository's ID, backed up to by the plan "demo-home"
    And <history>
    When the agent decides which saved repositories to offer to create
    Then the repository "demo" is not offered to be created

    Examples:
      | history                                                                            |
      | no operation of the plan "demo-home" is on record                                  |
      | the latest operation of the plan "demo-home" failed because no repository is there |

  Scenario: Connecting a repository asks nobody to maintain it unless the person chose so
    Given a repository the engine identifies
    When the agent connects it as "demo"
    Then the repository "demo" asks nobody to maintain it

  Scenario: Connecting a repository the person chose to maintain from this agent asks this agent to maintain it
    Given a repository the engine identifies
    When the agent connects it as "demo" to maintain it from this agent
    Then the repository "demo" asks this agent to maintain it

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
