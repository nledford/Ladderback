Feature: The forget preview of a proposal
  Before approving a proposal, a person can see what it would change about
  the snapshots retention forgets (I6; decision 89, point 8). The forget
  preview covers every repository the proposal touches: for each, the
  snapshots each plan's retention would forget before the proposal and after
  it, and the difference. A retention change is only one way a proposal can
  forget more: a new plan that takes the ID of history no plan owns, or a
  plan moved to another repository, can too. The preview reads each
  repository the way a forget's own preview does: it takes no maintenance
  slot, writes nothing to the operation log, asks the engine to remove
  nothing and leaves the proposal pending.

  Scenario: A retention change shows the snapshots it would forget before and after
    Given a plan "home" keeping the last 3 snapshots
    And the repository holds 4 snapshots of the plan "home"
    And an AI agent has proposed keeping the last 1 snapshot of the plan "home"
    When a person previews what the proposal would forget
    Then before the proposal, retention would forget 1 of the 4 snapshots of the plan "home"
    And after the proposal, retention would forget 3 of the 4 snapshots of the plan "home"
    And the proposal would forget 2 snapshots of the plan "home" that nothing forgets now
    And the engine was never asked to remove anything
    And the proposal is pending

  Scenario: A new plan that takes the ID of history no plan owns would forget part of that history
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "photos"
    And an AI agent has proposed adding the plan "photos" keeping the last 1 snapshot
    When a person previews what the proposal would forget
    Then before the proposal, retention would forget nothing of the plan "photos"
    And after the proposal, retention would forget 2 of the 3 snapshots of the plan "photos"
    And the proposal would forget 2 snapshots of the plan "photos" that nothing forgets now
    And the engine was never asked to remove anything

  Scenario: A removed plan's history is left alone, so the proposal forgets less
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And an AI agent has proposed removing the plan "home"
    When a person previews what the proposal would forget
    Then before the proposal, retention would forget 2 of the 3 snapshots of the plan "home"
    And after the proposal, retention would forget nothing of the plan "home"
    And after the proposal, the repository leaves alone 1 group of orphaned history holding 3 snapshots
    And the proposal would no longer forget 2 snapshots of the plan "home"

  Scenario: A proposal that changes no retention would forget nothing new
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And an AI agent has proposed giving the plan "home" the label "Home folder"
    When a person previews what the proposal would forget
    Then the preview covers the repository "demo"
    And the proposal changes nothing retention forgets

  Scenario: A proposal that fixes a policy keeping nothing shows what retention would then forget
    Given a plan "home" keeping nothing
    And the repository holds 3 snapshots of the plan "home"
    And an AI agent has proposed keeping the last 1 snapshot of the plan "home"
    When a person previews what the proposal would forget
    Then before the proposal, a forget would be refused because the policy of the plan "home" keeps nothing
    And after the proposal, retention would forget 2 of the 3 snapshots of the plan "home"
    And the proposal would forget 2 snapshots of the plan "home" that nothing forgets now

  Scenario: A proposal whose policy would keep nothing shows that a forget would then be refused
    Given a plan "home" keeping the last 1 snapshot
    And the repository holds 3 snapshots of the plan "home"
    And an AI agent has proposed keeping the last 0 snapshots of the plan "home"
    When a person previews what the proposal would forget
    Then before the proposal, retention would forget 2 of the 3 snapshots of the plan "home"
    And after the proposal, a forget would be refused because the policy of the plan "home" keeps nothing
    And the proposal would no longer forget 2 snapshots of the plan "home"

  Scenario: Retention forgets nothing in a repository this agent doesn't maintain
    Given a plan "home" keeping the last 3 snapshots
    And the repository holds 4 snapshots of the plan "home"
    And the repository's maintenance is "hub"
    And an AI agent has proposed keeping the last 1 snapshot of the plan "home"
    When a person previews what the proposal would forget
    Then the preview says this agent doesn't maintain the repository "demo", so its retention forgets nothing there before or after the proposal
    And the engine was never asked to list the repository

  Scenario: A proposal that is no longer pending has no forget preview
    Given a plan "home" keeping the last 3 snapshots
    And the repository holds 4 snapshots of the plan "home"
    And an AI agent has proposed keeping the last 1 snapshot of the plan "home"
    When the AI agent withdraws the proposal
    And a person previews what the proposal would forget
    Then the forget preview is refused because the proposal is withdrawn
    And the engine was never asked to list the repository

  # Approving with the forget preview (decision 89, point 8). A proposal
  # that could delete snapshots is approved only with the forget preview the
  # person saw. The approval brings that preview's digest, which binds the
  # snapshots whose fate the proposal changes, as the difference shows them;
  # the daemon reads the preview again under the lock its apply holds, and
  # applies the proposal only if the two match, so nothing can change
  # between the check and the write.

  Scenario: A proposal that could delete snapshots is approved as its forget preview showed it
    Given a plan "home" keeping the last 3 snapshots
    And the repository holds 4 snapshots of the plan "home"
    And an AI agent has proposed keeping the last 1 snapshot of the plan "home"
    And a person has opened the proposal
    When a person previews what the proposal would forget
    And the person approves the proposal as its forget preview showed it
    Then the proposal is applied
    And the plan "home" keeps the last 1 snapshot
    And the engine was never asked to remove anything

  Scenario: An approval is refused when a backup added a snapshot since the forget preview
    Given a plan "home" keeping the last 3 snapshots
    And the repository holds 4 snapshots of the plan "home"
    And an AI agent has proposed keeping the last 1 snapshot of the plan "home"
    And a person has opened the proposal
    When a person previews what the proposal would forget
    And a backup of the plan "home" has since added 1 snapshot
    And the person approves the proposal as its forget preview showed it
    Then the approval is refused because what the proposal would forget has changed since the preview
    And the proposal is pending
    And the plan "home" keeps the last 3 snapshots

  Scenario: A backup of another plan in the same repository since the forget preview leaves the approval standing
    Given a plan "home" keeping the last 3 snapshots
    And a plan "photos" keeping the last 1 snapshot
    And the repository holds 4 snapshots of the plan "home"
    And the repository holds 2 snapshots of the plan "photos"
    And an AI agent has proposed keeping the last 1 snapshot of the plan "home"
    And a person has opened the proposal
    When a person previews what the proposal would forget
    And a backup of the plan "photos" has since added 1 snapshot
    And the person approves the proposal as its forget preview showed it
    Then the proposal is applied
    And the plan "home" keeps the last 1 snapshot
    And the engine was never asked to remove anything

  Scenario: An approval is refused when the repository holds other snapshots than the forget preview showed
    Given a plan "home" keeping the last 3 snapshots
    And the repository holds 4 snapshots of the plan "home"
    And an AI agent has proposed keeping the last 1 snapshot of the plan "home"
    And a person has opened the proposal
    When a person previews what the proposal would forget
    And the repository has since been replaced by one holding 4 other snapshots of the plan "home"
    And the person approves the proposal as its forget preview showed it
    Then the approval is refused because what the proposal would forget has changed since the preview
    And the proposal is pending
    And the plan "home" keeps the last 3 snapshots

  Scenario: An approval that brings another proposal's forget preview is refused
    Given a plan "home" keeping the last 3 snapshots
    And the repository holds 4 snapshots of the plan "home"
    And an AI agent has proposed keeping the last 1 snapshot of the plan "home"
    And a person has opened the proposal
    When a person previews what the proposal would forget
    And the person approves the proposal with the forget preview of another proposal that would forget the same
    Then the approval is refused because what the proposal would forget has changed since the preview
    And the proposal is pending

  Scenario: An approval is refused when a repository it touches can no longer be read
    Given a plan "home" keeping the last 3 snapshots
    And a second repository "usb" whose password is in the environment variable "DEMO_USB_PASSWORD"
    And the repository holds 4 snapshots of the plan "home"
    And an AI agent has proposed moving the plan "home" to the repository "usb"
    And a person has opened the proposal
    When a person previews what the proposal would forget
    And the environment variable "DEMO_USB_PASSWORD" has since been unset
    And the person approves the proposal as its forget preview showed it
    Then the approval is refused because what the proposal would forget has changed since the preview
    And the proposal is pending
    And the plan "home" backs up to the repository "demo"
    When a person previews what the proposal would forget
    Then the preview says the repository "usb" couldn't be read, so it offers no approval

  Scenario: A plan moved to another repository is approved as its forget preview showed it
    Given a plan "home" keeping the last 3 snapshots
    And a second repository "usb"
    And the repository holds 4 snapshots of the plan "home"
    And an AI agent has proposed moving the plan "home" to the repository "usb"
    And a person has opened the proposal
    When a person previews what the proposal would forget
    Then the preview covers the repositories "demo" and "usb"
    And the forget preview of the 2 repositories offers an approval
    When the person approves the proposal as its forget preview showed it
    Then the proposal is applied
    And the plan "home" backs up to the repository "usb"
    And the engine was never asked to remove anything
