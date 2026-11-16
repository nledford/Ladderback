@repositories
Feature: Keeping a repository's password in the system keyring
  When the person chooses the system keyring for the password they typed,
  the agent keeps it in the keyring entry named "ladderback/" and the
  repository's name, and the configuration keeps a reference to that entry,
  never the password (§10.2). An entry is never overwritten: one that holds
  another password is refused and left as it is, and one that already holds
  the typed password is used as it is. Where there is no keyring to keep it
  in, nothing is saved and nothing is created. A request that fails after it
  added an entry removes it again, unless a repository was created with that
  password, whose only copy the entry may then be.

  Scenario: Connecting with the password kept in the keyring saves a reference to the entry, never the password
    Given a repository the engine identifies
    When the agent connects it as "demo", keeping the password in the keyring
    Then the keyring entry "ladderback/demo" holds the typed password
    And the agent has the repository "demo" configured with its password read from the keyring entry "ladderback/demo"

  Scenario: Creating a repository under a name with the password kept in the keyring saves a reference to the entry
    Given a repository the engine identifies
    When the agent initialises the repository as "demo", keeping the password in the keyring
    Then the engine initialised the repository with the typed password
    And the keyring entry "ladderback/demo" holds the typed password
    And the agent has the repository "demo" configured with its password read from the keyring entry "ladderback/demo"

  Scenario: Where there is no keyring, connecting with the password kept in it is refused, and nothing is saved
    Given a repository the engine identifies
    And a machine with no keyring service
    When the agent connects it as "demo", keeping the password in the keyring
    Then the request is refused because there is no keyring to keep the password in
    And the agent has no repository configured

  Scenario: Where there is no keyring, creating with the password kept in it is refused before anything is created
    Given a repository the engine identifies
    And a machine with no keyring service
    When the agent initialises the repository as "demo", keeping the password in the keyring
    Then the request is refused because there is no keyring to keep the password in
    And the engine ran no job
    And the agent has no repository configured

  Scenario: A keyring entry that holds another password is refused and left as it is
    Given a repository the engine identifies
    And the keyring entry "ladderback/demo" holds "another-password"
    When the agent connects it as "demo", keeping the password in the keyring
    Then the request is refused because the keyring entry holds another password
    And the keyring entry "ladderback/demo" holds "another-password"
    And the agent has no repository configured

  Scenario: A keyring entry that already holds the typed password is used as it is
    Given a repository the engine identifies
    And the keyring entry "ladderback/demo" holds the typed password
    When the agent connects it as "demo", keeping the password in the keyring
    Then the agent has the repository "demo" configured with its password read from the keyring entry "ladderback/demo"

  Scenario: A connection whose test fails keeps nothing in the keyring
    Given a repository the engine cannot open
    When the agent connects it as "demo", keeping the password in the keyring
    Then the connection fails
    And the keyring has no entry "ladderback/demo"

  Scenario: Connecting under a name already used keeps nothing in the keyring
    Given a repository the engine identifies
    And the agent has the repository "demo" configured with its settings
    When the agent connects it as "demo", keeping the password in the keyring
    Then the connection is refused because the alias is taken
    And the keyring has no entry "ladderback/demo"

  Scenario: A creation that fails removes the keyring entry it added
    Given a location that already holds a repository
    When the agent initialises the repository as "demo", keeping the password in the keyring
    Then the initialisation is refused because a repository already exists
    And the keyring has no entry "ladderback/demo"
    And the agent has no repository configured

  Scenario: A creation that fails leaves a keyring entry that was there before
    Given a location that already holds a repository
    And the keyring entry "ladderback/demo" holds the typed password
    When the agent initialises the repository as "demo", keeping the password in the keyring
    Then the initialisation is refused because a repository already exists
    And the keyring entry "ladderback/demo" holds the typed password

  Scenario: A creation whose settings can't be resolved keeps nothing in the keyring
    Given a repository the engine identifies
    And its location names a variable that is not set
    When the agent initialises the repository as "demo", keeping the password in the keyring
    Then the request is refused because a secret can't be read
    And the engine ran no job
    And the keyring has no entry "ladderback/demo"
