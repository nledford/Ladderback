@ui
Feature: Pending changes page
  The "Pending changes" page lists the proposals an AI agent made through
  the API, and a person approves or rejects each as a whole, with the
  keyboard alone (A1, A2; decision 89, point 6). An open proposal shows what
  it changes in Ladderback's words, the AI agent's words apart from them,
  and the exact changes to the configuration file, each secret the file
  holds masked (I4). Approving asks for step-up confirmation: with sign-in
  off, the proposal's short ID, and the page says that approval is then no
  safety boundary. A proposal that could delete snapshots shows, on request,
  its forget preview: what retention would forget before and after it, and
  the difference (decision 89, point 8), and is approved only from that
  preview; until then it is only rejected, and the page says why. Rejecting takes a reason for
  each change, which the AI agent reads. Whatever the daemon refuses, the
  page shows in an error alert where the reader is looking, and says it
  assertively once (§4.5, #929).

  Scenario: Approving a proposal by keyboard alone applies it
    Given a demo plan "demo-home" with a scratch repository, and a proposal to add the plan "demo-docs"
    When I open the web UI
    And I press Tab until focus is on the link "Pending changes"
    And I press Enter
    Then focus is on the main heading "Pending changes"
    And the page is titled "Pending changes – Ladderback"
    And the page says "Sign-in is off, so approving here is a convenience, not a safety boundary: a program that can reach Ladderback could approve its own proposal. To make approval yours alone, turn sign-in on with ladderback set-password and start Ladderback again."
    When I press Tab until focus is on the proposal's Review button
    And I press Enter
    Then focus is on the proposal's heading
    And the proposal says "Adds the plan demo-docs."
    And the proposal says "Tidy the demo plans."
    And the page has no accessibility violations
    When I press Tab until focus is on the textbox "Proposal ID"
    And I type the proposal's short ID
    And I press Tab until focus is on the button "Approve the proposal"
    And I press Enter
    Then the polite region comes to contain "is applied, and the configuration file is saved."
    And focus is on the main heading "Pending changes"
    And the page says "No proposal is pending."
    And the daemon lists the plan "demo-docs"
    And the page has no accessibility violations

  Scenario: A proposal that removes a plan is only rejected, with a reason for its change
    Given a demo plan "demo-home" with a scratch repository, and a proposal to remove it
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    Then focus is on the proposal's heading
    And the proposal says "Removes the plan demo-home. Its snapshots stay in the repository."
    And the changes to the configuration file show the repository's password masked
    And the proposal says "This proposal could delete snapshots. Show what it would forget first: you can approve it once the forget preview shows which ones would go."
    And the page offers no button "Approve the proposal"
    And the page has no accessibility violations
    When I press Tab until focus is on the textbox "Reason for change 1 (optional)"
    And I type "Keep demo-home: it backs up the home folder."
    And I press Tab until focus is on the button "Reject the proposal"
    And I press Enter
    Then the polite region comes to contain "is rejected. The AI agent can read your reasons."
    And focus is on the main heading "Pending changes"
    And the page says "No proposal is pending."
    And the daemon lists the plan "demo-home"

  Scenario: A proposal that could delete snapshots is approved from its forget preview
    Given a demo plan "demo-home" with a tiny scratch repository
    And the plan "demo-home" has been backed up 3 times
    And the plan "demo-home" keeps the last 1 snapshots
    And a proposal to remove the plan "demo-home"
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    Then the page offers no button "Approve the proposal"
    When I press Tab until focus is on the button "Show what it would forget"
    And I press Enter
    Then the proposal says "After the proposal, retention would no longer forget 2 snapshots of the plan demo-home"
    And the proposal says "Now, retention forgets 2 of the 3 snapshots of the plan demo-home."
    And the proposal says "After the proposal, no plan would own any snapshots in this repository, so retention would forget nothing there."
    And the page has no accessibility violations
    When I press Tab until focus is on the textbox "Proposal ID"
    And I type the proposal's short ID
    And I press Tab until focus is on the button "Approve the proposal"
    And I press Enter
    Then the polite region comes to contain "is applied, and the configuration file is saved."
    And the daemon no longer lists the plan "demo-home"

  Scenario: An approval refused because what the proposal would forget changed is an error alert, said assertively, until a new forget preview
    Given a demo plan "demo-home" with a tiny scratch repository
    And the plan "demo-home" has been backed up 3 times
    And the plan "demo-home" keeps the last 1 snapshots
    And a proposal to remove the plan "demo-home"
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    And I press Tab until focus is on the button "Show what it would forget"
    And I press Enter
    Then the proposal says "After the proposal, retention would no longer forget 2 snapshots of the plan demo-home"
    When the plan "demo-home" has been backed up 1 times
    And I press Tab until focus is on the textbox "Proposal ID"
    And I type the proposal's short ID
    And I press Tab until focus is on the button "Approve the proposal"
    And I press Enter
    Then the page shows an error alert containing "What this proposal would forget has changed since you previewed it, or a repository it touches couldn't be read again, so nothing was applied. Show what it would forget again, then approve it if it still looks right."
    And the assertive region comes to contain "What this proposal would forget has changed since you previewed it, or a repository it touches couldn't be read again, so nothing was applied."
    And focus stays on the button "Approve the proposal"
    And the daemon lists the plan "demo-home"
    When I press Shift+Tab until focus is on the button "Show what it would forget"
    And I press Enter
    Then the proposal says "After the proposal, retention would no longer forget 3 snapshots of the plan demo-home"
    And the open proposal shows no alert
    When I press Tab until focus is on the button "Approve the proposal"
    And I press Enter
    Then the polite region comes to contain "is applied, and the configuration file is saved."
    And the daemon no longer lists the plan "demo-home"

  Scenario: A list of pending proposals the daemon refuses is an error alert, said assertively
    Given a demo plan "demo-home" with a scratch repository, and a proposal to add the plan "demo-docs"
    And the daemon refuses each read of the pending proposals
    When I open the address "/proposals"
    Then the page shows an error alert containing "Ladderback could not read or keep the proposals."
    And the assertive region comes to contain "Ladderback could not read or keep the proposals."
    And the main heading reads "Pending changes"
    And the page has no accessibility violations

  Scenario: A proposal whose review the daemon refuses is an error alert, said assertively, and focus stays on its Review button
    Given a demo plan "demo-home" with a scratch repository, and a proposal to add the plan "demo-docs"
    And the daemon refuses the review of the proposal
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    Then the page shows an error alert containing "Ladderback could not read or keep the proposals."
    And the assertive region comes to contain "Ladderback could not read or keep the proposals."
    And focus stays on the proposal's Review button
    And the page has no accessibility violations

  Scenario: A rejection the daemon refuses is an error alert in the open proposal, said assertively, and focus stays on Reject
    Given a demo plan "demo-home" with a scratch repository, and a proposal to add the plan "demo-docs"
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    And the AI agent withdraws the proposal
    And I press Tab until focus is on the button "Reject the proposal"
    And I press Enter
    Then the open proposal shows an error alert containing "The proposal was withdrawn, so nothing more was done to it."
    And the page shows an error alert containing "The proposal was withdrawn, so nothing more was done to it."
    And the assertive region comes to contain "The proposal was withdrawn, so nothing more was done to it."
    And focus stays on the button "Reject the proposal"
    And the page has no accessibility violations

  Scenario: A forget preview the daemon refuses is an error alert in the open proposal, said assertively, and focus stays on its button
    Given a demo plan "demo-home" with a scratch repository, and a proposal to remove it
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    And the AI agent withdraws the proposal
    And I press Tab until focus is on the button "Show what it would forget"
    And I press Enter
    Then the open proposal shows an error alert containing "The proposal was withdrawn, so nothing more was done to it."
    And the page shows an error alert containing "The proposal was withdrawn, so nothing more was done to it."
    And the assertive region comes to contain "The proposal was withdrawn, so nothing more was done to it."
    And focus stays on the button "Show what it would forget"
    And the page has no accessibility violations

  Scenario: A repository the forget preview can't read says why in an error alert in the open proposal, said assertively
    Given a demo plan "demo-home" with a scratch repository, and a proposal to remove it
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    And the repository is moved away
    And I press Tab until focus is on the button "Show what it would forget"
    And I press Enter
    Then the open proposal shows an error alert containing "No repository was found at this location"
    And the page shows an error alert containing "No repository was found at this location"
    And the assertive region comes to contain "No repository was found at this location"
    And the page offers no button "Approve the proposal"
    And the page has no accessibility violations

  Scenario: A refusal in the open proposal takes the place of the one before it, so one failure shows one error alert
    Given a demo plan "demo-home" with a scratch repository, and a proposal to add the plan "demo-docs"
    When I open the address "/proposals"
    And I skip to the content
    And I press Tab until focus is on the proposal's Review button
    And I press Enter
    And I press Tab until focus is on the textbox "Proposal ID"
    And I type the proposal's short ID
    And the AI agent withdraws the proposal
    And I press Tab until focus is on the button "Approve the proposal"
    And I press Enter
    Then the open proposal shows an error alert containing "The proposal was withdrawn, so nothing more was done to it."
    When I press Tab until focus is on the button "Reject the proposal"
    And I press Enter
    Then the open proposal shows one alert, after the button "Reject the proposal"

  Scenario: With sign-in off, the page's notice that approving is no safety boundary is a warning banner
    Given a demo plan "demo-home" with a scratch repository, and a proposal to add the plan "demo-docs"
    When I open the address "/proposals"
    Then the page shows a warning banner that says "Sign-in is off, so approving here is a convenience, not a safety boundary"
    And the page has no accessibility violations
