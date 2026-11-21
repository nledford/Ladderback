Feature: The hub learns each machine's time zone
  A machine tells its hub its machine zone, the IANA time zone it runs in,
  when it pairs and each time it connects, so the hub can show the
  machine's times in the machine's own zone (technical design §7.1, §7.2).
  The hub keeps the zone the machine reported last, and holds it as unknown
  rather than guessing when the zone has no IANA name or the hub's copy of
  the zone database doesn't know it.

  Scenario: A machine's time zone reaches the hub when it pairs
    Given a hub with a pairing token
    When a machine in the time zone "America/Chicago" pairs with the token
    Then the hub holds the machine's time zone as "America/Chicago"

  Scenario: A machine that moved tells its hub at its next connection
    Given a machine paired with its hub from the time zone "America/Chicago"
    When the machine moves to the time zone "Europe/Berlin" and connects to its hub
    Then the hub holds the machine's time zone as "Europe/Berlin"

  Scenario: A zone with no IANA name is held as unknown
    Given a machine paired with its hub from the time zone "America/Chicago"
    When the machine's zone becomes a fixed offset of -5 hours and it connects to its hub
    Then the hub holds the machine's time zone as unknown

  Scenario: A zone the hub's copy of the zone database doesn't know is held as unknown
    Given a hub with a pairing token
    When a machine in a time zone named "America/Atlantis" that the hub's zone database doesn't know pairs with the token
    Then the hub holds the machine's time zone as unknown

  Scenario: A machine that moves to a zone the hub's copy of the zone database doesn't know is held as unknown
    Given a machine paired with its hub from the time zone "America/Chicago"
    When the machine moves to a time zone named "America/Atlantis" that the hub's zone database doesn't know and connects to its hub
    Then the hub holds the machine's time zone as unknown

  @e2e
  Scenario: A machine's time zone reaches the hub when it pairs from its terminal
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    When a machine named "e2e-laptop" in the time zone "America/Chicago" pairs with the token from its terminal
    Then once the hub stops, it holds "America/Chicago" as the machine's time zone

  @e2e
  Scenario: A machine that moved tells its hub when it checks its link
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" in the time zone "America/Chicago" paired with the token from its terminal
    When the machine, now in the time zone "Europe/Berlin", checks its link to the hub from its terminal
    Then once the hub stops, it holds "Europe/Berlin" as the machine's time zone
