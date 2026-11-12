Feature: Notification rules
  A notification rule sends the events it names to one channel: a webhook,
  an ntfy topic or a healthchecks check (technical design §11). A rule may
  be for some plans only, and may write its messages with its own template.
  No notification carries a secret, a location or the engine's own words
  (I4): only the facts of the event and the code of a failure.

  @e2e
  Scenario: A webhook on loopback is told of a backup that succeeded
    Given a webhook listening on loopback that the rule "endings" sends "backup.succeeded" to
    And Ladderback is serving on loopback with the plan "home"
    When the daemon runs a backup of the plan "home"
    Then the webhook is told "backup.succeeded" for the plan "home"

  Rule: Each rule sends the events it names, for the plans it names, to its channel

    Background:
      Given the plans "home" and "work" back up to the repository "nas"

    Scenario: A failed backup is sent to the channel of the rule for it
      Given a webhook channel "ops"
      And a rule "failures" that sends "backup.failed" to "ops"
      When the backup of "home" fails
      Then "ops" is sent "backup.failed" for the plan "home" with the code "operation-failed"

    Scenario: An event no rule names is sent nowhere
      Given a webhook channel "ops"
      And a rule "failures" that sends "backup.failed" to "ops"
      When the backup of "home" starts
      And the backup of "home" succeeds
      Then no channel is sent anything

    Scenario: A rule for one plan is not sent another plan's events
      Given a webhook channel "ops"
      And a rule "home-only" that sends "backup.failed" to "ops" for the plan "home"
      When the backup of "work" fails
      And the backup of "home" fails
      Then "ops" is sent only "backup.failed" for the plan "home"

    Scenario: A backup that ends with warnings is told as a warning, not as a success
      Given a webhook channel "ops"
      And a rule "endings" that sends "backup.succeeded, backup.warning" to "ops"
      When the backup of "home" succeeds with a warning
      Then "ops" is sent only "backup.warning" for the plan "home"

    Scenario: A healthchecks channel is pinged when a backup starts and when it fails
      Given a healthchecks channel "home-check"
      And a rule "check" that sends "backup.started, backup.succeeded, backup.failed" to "home-check" for the plan "home"
      When the backup of "home" starts
      And the backup of "home" fails
      Then "home-check" is pinged "start" and then "fail"

    Scenario: A template is written with the facts of the event and never a secret
      Given a webhook channel "ops" whose URL holds the canary "canary-url-secret"
      And a rule "failures" that sends "backup.failed" to "ops" with the template "{event} {plan} {repository} {code}"
      When the backup of "home" fails and the engine says "canary-engine-detail"
      Then "ops" is sent the message "backup.failed home nas operation-failed"
      And nothing "ops" is sent shows "canary-url-secret" or "canary-engine-detail"

    Scenario: A channel that can't be reached is reported, and the next channel is still sent
      Given a webhook channel "down" that nothing answers at
      And a webhook channel "ops"
      And a rule "a-down" that sends "backup.failed" to "down"
      And a rule "b-ops" that sends "backup.failed" to "ops"
      When the backup of "home" fails
      Then "ops" is sent "backup.failed" for the plan "home" with the code "operation-failed"
      And the notification of "backup.failed" to "down" is reported as "unreachable"

    Scenario: A channel whose URL can't be read is reported, and nothing is sent to it
      Given a webhook channel "ops" whose URL is in a variable that isn't set
      And a rule "failures" that sends "backup.failed" to "ops"
      When the backup of "home" fails
      Then no channel is sent anything
      And the notification of "backup.failed" to "ops" is reported as "secret-unavailable"

    Scenario: A prune that fails names its repository
      Given an ntfy channel "pager"
      And a rule "maintenance" that sends "prune.failed" to "pager"
      When a prune of "nas" fails
      Then "pager" is sent "prune.failed" for the repository "nas" with the code "operation-failed"

    Scenario: A rule that sends to a channel the configuration doesn't have is refused
      Given a webhook channel "ops"
      And a rule "failures" that sends "backup.failed" to "pager"
      Then the configuration is refused because the rule "failures" sends to no channel "pager"
