@e2e @repositories
Feature: Repositories over HTTP
  The Repositories page tests a location, connects it by saving it into the
  configuration, and initialises a repository where nothing is yet, all through
  the daemon's API (components.md §4.4). A request carries the password a
  person typed: no answer carries it back, and what the configuration keeps is
  the settings as written, a secret reference included (I4). A repository's
  location is answered with any password in it masked.

  Background:
    Given Ladderback is serving on loopback with no repositories

  Scenario: Testing a repository over HTTP reports its ID and saves nothing
    Given an initialised repository
    When the client tests the repository
    Then the answer has status 200 and the repository's ID
    And the daemon lists no repositories

  Scenario: Testing with a wrong password is refused with the engine's words, and saves nothing
    Given an initialised repository
    And the client uses the wrong password
    When the client tests the repository
    Then the refusal has status 422 and the typed code "operation-failed"
    And the refusal carries the engine's detail
    And no answer holds the password
    And the daemon lists no repositories

  Scenario: Testing a location with no repository is refused, and saves nothing
    Given an empty repository location
    When the client tests the repository
    Then the refusal has status 422 and the typed code "operation-failed"
    And the refusal carries the engine's detail
    And the daemon lists no repositories

  Scenario: Connecting a repository adds it to the list, without its password
    Given an initialised repository
    When the client connects it as "demo"
    Then the answer has status 201 and the repository's ID
    And the daemon lists the repository "demo" at the location, its password stored inline
    And no answer holds the password

  Scenario: A password stored in the config is listed with a warning that names the setting, not the value
    Given an initialised repository
    When the client connects it as "demo"
    Then the daemon lists the repository "demo" with a warning that its password is written in the config
    And no answer holds the password

  Scenario: Connecting over HTTP with an alias already used is refused
    Given an initialised repository
    And the client has connected it as "demo"
    When the client connects it as "demo"
    Then the refusal has status 409 and the typed code "repository-alias-taken"
    And the daemon lists 1 repository

  Scenario: Connecting under an alias that is not a name is refused
    Given an initialised repository
    When the client connects it as "Not A Name"
    Then the refusal has status 422 and the typed code "repository-alias-invalid"
    And the daemon lists no repositories

  Scenario: Connecting a repository whose test fails saves nothing
    Given an initialised repository
    And the client uses the wrong password
    When the client connects it as "demo"
    Then the refusal has status 422 and the typed code "operation-failed"
    And the daemon lists no repositories

  Scenario: A password file that is not there is refused with a code of its own
    Given an initialised repository
    And the client's password is in a file that is not there
    When the client connects it as "demo"
    Then the refusal has status 422 and the typed code "secret-missing"
    And the refusal names the setting "password" and the file the password is read from
    And the daemon lists no repositories

  Scenario: Two repositories connected at the same moment are both kept
    Given two initialised repositories
    When the client connects one as "first" and the other as "second" at the same moment
    Then the daemon lists 2 repositories

  Scenario: A password file stays a reference, in the saved config and in the list
    Given an initialised repository
    And the client's password is in a file
    When the client connects it as "demo"
    Then the daemon's config file doesn't hold "demo-password"
    And the daemon lists the repository "demo" at the location, its password in the file
    And the daemon lists the repository "demo" with no warnings

  Scenario: Initialising a new local repository gives it an ID and saves nothing
    Given an empty repository location
    When the client initialises the repository
    Then the answer has status 201 and the repository's ID, not saved
    And the repository at the location opens with the password
    And the daemon lists no repositories

  Scenario: Initialising as an alias saves the repository too
    Given an empty repository location
    When the client initialises the repository as "fresh"
    Then the answer has status 201 and the repository's ID, saved
    And the daemon lists the repository "fresh" at the location, its password stored inline

  Scenario: Initialising under an alias already used is refused and creates nothing
    Given an initialised repository
    And the client has connected it as "demo"
    And the client's repository is at another, empty location
    When the client initialises the repository as "demo"
    Then the refusal has status 409 and the typed code "repository-alias-taken"
    And the repository is unchanged
    And the daemon lists 1 repository

  Scenario: Initialising over HTTP where a repository already exists is refused, and the repository is unchanged
    Given an initialised repository
    When the client initialises the repository
    Then the refusal has status 409 and the typed code "repository-exists"
    And the repository is unchanged
    And no answer holds the password

  Scenario: A new hot/cold repository is initialised in both halves over HTTP
    Given an empty hot/cold repository location
    When the client initialises the repository
    Then the answer has status 201 and the repository's ID, not saved
    And the hot and the cold half each hold the repository's config

  Scenario Outline: A body that is not the JSON the route takes is refused: <what>
    When the client sends <body> to "<route>"
    Then the refusal has status 400 and the typed code "invalid-body"
    And the daemon lists no repositories

    Examples:
      | what                     | route                      | body                                                                                     |
      | not JSON                 | /api/v1/repositories/_test | not json                                                                                 |
      | no settings              | /api/v1/repositories/_test | {}                                                                                       |
      | no alias                 | /api/v1/repositories       | {"settings":{"repository":"/x","password":{"source":"inline","value":"p"}}}              |
      | a source the API refuses | /api/v1/repositories/_test | {"settings":{"repository":"/x","password":{"source":"command","command":"pass show x"}}} |
      | an unknown field         | /api/v1/repositories/_init | {"settings":{"repository":"/x","password":{"source":"inline","value":"p"},"repo_hot":"/y"}} |
      | a relative password file | /api/v1/repositories/_test | {"settings":{"repository":"/x","password":{"source":"file","path":"password.txt"}}}      |
      | an empty password        | /api/v1/repositories/_test | {"settings":{"repository":"/x","password":{"source":"inline","value":""}}}               |
      | an empty location        | /api/v1/repositories/_test | {"settings":{"repository":"","password":{"source":"inline","value":"p"}}}                |
