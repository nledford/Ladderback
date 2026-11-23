Feature: Removing or rejecting a machine on the hub, and a waiting machine lapsing
  A person rejects a machine that waits for approval, or removes one the hub
  approved, on the running hub's page. Either only takes authority away, so
  it needs a signed-in session and no step-up, as revoking a token does. The
  hub names the agent ID under the key it listed, so a machine that paired
  again with another key in between is left alone. The hub keeps a
  tombstone of the machine: its agent ID, hostname and key. The removed
  machine's next connection is answered, signed, that the hub removed it,
  and a pairing under its agent ID with an ordinary token is refused the
  same way. A machine nobody approves within 7 days lapses: the hub drops
  it, frees its hostname and keeps no tombstone, since nobody judged it
  (technical design §7.1; §16, decision 120, "Pending approval", "Removing,
  re-keying and recovering an agent").

  Scenario: A waiting machine is rejected
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    When the person rejects "laptop" on the hub's page
    Then the hub lists no agent
    And the hub keeps a tombstone of "laptop"
    When the agent connects to its hub
    Then the hub tells the agent it removed it

  Scenario: An approved machine is removed, and its next connection is refused
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    And the person approved "laptop" on the hub's page
    When the person removes "laptop" on the hub's page
    Then the hub lists no agent
    And the hub keeps a tombstone of "laptop"
    When the agent connects to its hub
    Then the hub tells the agent it removed it

  Scenario: A removed machine can't pair again with an ordinary token
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    And the person removed "laptop" on the hub's page
    When the agent pairs again with a new token
    Then the pairing is refused because the hub removed the agent
    And the hub lists no agent

  Scenario: A stranger under a removed machine's ID learns nothing
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    And the person removed "laptop" on the hub's page
    When a machine with another key connects under the agent's ID
    Then the hub refuses the machine's proof

  Scenario: A removed machine's hostname is free for another machine
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    And the person removed "laptop" on the hub's page
    When another agent pairs as "laptop"
    Then the hub lists it as waiting for approval

  Scenario: The page removes no machine under a key it doesn't hold
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    When the person removes "laptop" on the hub's page with another key
    Then the hub refuses the removal because no such machine is registered
    And the hub lists "laptop" as waiting for approval

  Scenario: A waiting machine nobody approved lapses after 7 days, and its hostname is free
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    When 6 days and 23 hours pass on the hub's clock
    Then the hub lists "laptop" as waiting for approval
    When 1 hour passes on the hub's clock
    Then the hub lists no agent
    And the hub keeps no tombstone of "laptop"
    When another agent pairs as "laptop"
    Then the hub lists it as waiting for approval

  Scenario: An approved machine never lapses, while one waiting beside it does
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    And the person approved "laptop" on the hub's page
    When another agent pairs as "desk"
    And 8 days pass on the hub's clock
    Then the hub still lists "laptop" as approved
    And the hub lists only "laptop"
