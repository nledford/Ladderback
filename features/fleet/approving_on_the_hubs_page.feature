Feature: Approving a machine on the running hub's page
  A machine that pairs is held pending until a person approves it on the
  hub. On the running hub's page, Settings → Machines lists the machines
  waiting for approval, and approving one names the agent ID and key the
  person compared with the machine's own screen: the hub approves that agent
  with that key, never a hostname alone. Approval is a high-impact action:
  it needs a fresh step-up every time, and beyond loopback it needs sign-in
  to be on. The machine is welcomed approved the next time it connects
  (technical design §7.1; §16, decisions 120 and 121, "Waiting for
  approval").

  Scenario: A waiting machine is approved with the admin password entered again
    Given a hub with sign-in on
    And an agent paired under the hostname "laptop"
    When the person approves "laptop" on the hub's page with the key it showed, confirming with the admin password
    Then the hub lists "laptop" as approved

  Scenario: A wrong password approves nothing
    Given a hub with sign-in on
    And an agent paired under the hostname "laptop"
    When the person approves "laptop" on the hub's page with the key it showed, confirming with a wrong password
    Then the hub refuses the approval as a step-up that failed
    And the hub lists "laptop" as waiting for approval

  Scenario: A remote page can't approve a machine while sign-in is off
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    When the person approves "laptop" from another computer with the key it showed, confirming with the hub's key ID
    Then the hub refuses the approval and says to set an admin password
    And the hub lists "laptop" as waiting for approval

  Scenario: With sign-in off, the hub's key ID confirms an approval on this machine
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    When the person approves "laptop" on the hub's page with the key it showed, confirming with the hub's key ID
    Then the hub lists "laptop" as approved

  Scenario: A key the machine doesn't hold approves nothing
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    When the person approves "laptop" on the hub's page with another key, confirming with the hub's key ID
    Then the hub refuses the approval because no such machine waits
    And the hub lists "laptop" as waiting for approval

  Scenario: A machine is approved once
    Given a hub with sign-in off
    And an agent paired under the hostname "laptop"
    And the person approved "laptop" on the hub's page
    When the person approves "laptop" on the hub's page with the key it showed, confirming with the hub's key ID
    Then the hub refuses the approval because "laptop" is approved already

  @e2e
  Scenario: A machine approved on the running hub's page is told it is approved
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    When the person reads the machines on the hub's page
    Then the page lists "e2e-laptop" as waiting with the key the machine showed
    When the person approves "e2e-laptop" on the hub's page, confirming with the hub's key ID
    Then the hub answers that "e2e-laptop" is approved
    When the machine checks its link to the hub from its terminal
    Then the machine says the hub approved it

  @e2e
  Scenario: The hub's page refuses to approve a machine twice
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    And the person approved "e2e-laptop" on the hub's page
    When the person approves "e2e-laptop" on the hub's page, confirming with the hub's key ID
    Then the hub refuses with the code "agent-approved-already"

  @e2e
  Scenario: The hub's page approves no machine that never paired
    Given the hub is serving on loopback
    When the person approves an agent ID that never paired on the hub's page, confirming with the hub's key ID
    Then the hub refuses with the code "agent-unknown"

  @e2e
  Scenario: The hub's log says when a person approves a machine and when it first connects approved
    Given a hub with a pairing token made at its terminal
    And the hub is serving on loopback
    And a machine named "e2e-laptop" paired with the token from its terminal
    When the person approves "e2e-laptop" on the hub's page, confirming with the hub's key ID
    And the machine checks its link to the hub from its terminal
    And the machine checks its link to the hub from its terminal
    Then the hub's log says in one line that "e2e-laptop" was approved, naming its agent ID and the key the machine showed
    And the hub's log says in one line that "e2e-laptop" connected for the first time since it was approved, naming its agent ID and the key the machine showed
    And the hub's log holds nothing of the token
