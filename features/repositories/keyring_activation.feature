@repositories
Feature: Saying when the keyring can't be started on demand
  On Linux the session bus starts the keyring service, the Secret Service, on
  demand: the first program of a session that asks for it has the bus start
  it. Where nothing can start it that way, as on machines a move from
  gnome-keyring to oo7 left without its activation file, it runs only once
  the desktop starts it, so a backup the schedule starts as the session
  begins may wait for it and then fail (#877). While the keyring runs but
  can't be started on demand, the Repositories page says so once, with the
  two ways out: repair the activation file, or keep the password in a file
  or a systemd credential. The agent asks the session bus only when the page
  asks, never before a run, and an answer it can't get notes nothing
  (security §10.2).

  Scenario: A keyring that runs now and can't be started on demand is noted
    Given a keyring service that runs now and that nothing can start on demand
    When the Repositories page asks for its notes
    Then the page notes that the keyring can't be started on demand

  Scenario Outline: Any other keyring is noted nothing: <keyring>
    Given <keyring>
    When the Repositories page asks for its notes
    Then the page notes nothing of the keyring

    Examples:
      | keyring                                                      |
      | a keyring service that can be started on demand              |
      | a machine with no keyring service                            |
      | a keyring service that can't be asked how it is started      |
