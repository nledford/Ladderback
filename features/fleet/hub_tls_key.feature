Feature: The hub's TLS key
  With built-in TLS, the hub serves its certificate from a key of its own,
  the hub's TLS key, kept in its data folder, and every agent pins that
  key's fingerprint when it pairs. The hub makes the key on its first start
  and never makes a replacement by itself: a new key would lock out every
  agent that pinned the old one (technical design §7.1; §16, decision 120,
  "The hub key").

  Scenario: A hub's first start makes its TLS key
    Given a hub that has never started with built-in TLS
    When the hub starts with built-in TLS
    Then the hub keeps a new TLS key in its data folder
    And the hub serves the TLS key it keeps, under that key's fingerprint

  Scenario: A later start serves the same TLS key, so pinned agents still match
    Given a hub that made its TLS key on its first start
    When the hub starts with built-in TLS again
    Then the hub serves the TLS key under the fingerprint it recorded at its first start
    And no new TLS key was made

  Scenario: A hub whose TLS key is gone makes no new one
    Given a hub that made its TLS key on its first start
    And the hub's TLS key is gone from its data folder
    When the hub starts with built-in TLS again
    Then the start is refused because the hub's TLS key is missing
    And no new TLS key was made

  Scenario: A hub whose data folder holds another TLS key refuses it
    Given a hub that made its TLS key on its first start
    And the hub's data folder holds another TLS key in its place
    When the hub starts with built-in TLS again
    Then the start is refused because the TLS key isn't the one the hub recorded
    And no new TLS key was made

  Scenario: A TLS key file that holds no key is refused and left as it is
    Given a hub that has never started with built-in TLS
    And the hub's TLS key file holds something that isn't a key
    When the hub starts with built-in TLS
    Then the start is refused because the TLS key file holds no key
    And no new TLS key was made
    And the TLS key file still holds what it held
