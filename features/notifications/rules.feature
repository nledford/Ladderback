Feature: Notification rules
  A notification rule sends the events it names to one channel: a webhook,
  an ntfy topic or a healthchecks check (technical design §11). A rule may
  be for some plans only, and may write its messages with its own template.
  No notification carries a secret, a location or the engine's own words
  (I4): only the facts of the event, the code of a failure or of each kind
  of warning, and an ntfy topic's title, a sentence of the catalog.

  @e2e
  Scenario: A webhook on loopback is told of a backup that succeeded
    Given a webhook listening on loopback that the rule "endings" sends "backup.succeeded" to
    And Ladderback is serving on loopback with the plan "home"
    When the daemon runs a backup of the plan "home"
    Then the webhook is told "backup.succeeded" for the plan "home"

  @e2e
  Scenario: An ntfy topic is told of a backup that warned, under a title in words, tagged with the event and with the warning's code
    Given an ntfy topic listening on loopback that the rule "warnings" sends "backup.warning" to with the template "{event}: {plan} ({code})"
    And Ladderback is serving on loopback with the plan "home"
    And the plan's before hook says "canary-hook-output" and exits 3, with on-failure warn
    When the daemon runs a backup of the plan "home"
    Then the ntfy topic is told "backup.warning: home (hook-warning)" under the title "Backup finished with warnings", tagged "backup.warning"
    And nothing the ntfy topic is told shows its URL or "canary-hook-output"

  @e2e
  Scenario: An ntfy topic is told of a backup that failed under a title in words
    Given an ntfy topic listening on loopback that the rule "failures" sends "backup.failed" to
    And Ladderback is serving on loopback with the plan "home"
    And the plan's before hook says "canary-hook-output" and exits 3, with on-failure error
    When the daemon runs a backup of the plan "home"
    Then the ntfy topic is told the default message under the title "Backup failed", tagged "backup.failed"
    And nothing the ntfy topic is told shows its URL or "canary-hook-output"

  @e2e
  Scenario: An ntfy title is written in the agent's locale
    Given an ntfy topic listening on loopback that the rule "failures" sends "backup.failed" to
    And the agent's locale is "en-XA"
    And Ladderback is serving on loopback with the plan "home"
    And the plan's before hook says "canary-hook-output" and exits 3, with on-failure error
    When the daemon runs a backup of the plan "home"
    Then the ntfy topic is told the default message under the title "Ɓaaƈķuuƥ ƒaaiŀeeḓ", tagged "backup.failed"

  Rule: Each rule sends the events it names, for the plans it names, to its channel

    Background:
      Given the plans "home" and "work" back up to the repository "nas"

    Scenario: A failed backup is sent to the channel of the rule for it
      Given a webhook channel "ops"
      And a rule "failures" that sends "backup.failed" to "ops"
      When the backup of "home" fails
      Then "ops" is sent "backup.failed" for the plan "home" with the code "operation-failed"

    Scenario: A retry planned for a failed backup is sent with its attempt and when it runs
      Given a webhook channel "ops"
      And a rule "retries" that sends "backup.retrying" to "ops"
      When the backup of "home" fails
      And the scheduler plans retry 1 of 3 of the failed backup of "home" for "2026-11-12T10:05:00Z"
      Then "ops" is sent "backup.retrying" for the plan "home": retry 1 of 3 at "2026-11-12T10:05:00Z"

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

    Scenario: A backup that succeeds without a warning is told as a success
      Given a webhook channel "ops"
      And a rule "endings" that sends "backup.succeeded, backup.warning" to "ops"
      When the backup of "home" succeeds
      Then "ops" is sent only "backup.succeeded" for the plan "home"

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

    Scenario: A template names the code of each kind of warning a backup had, once each, in the order they came
      Given a webhook channel "ops" whose URL holds the canary "canary-url-secret"
      And a rule "warnings" that sends "backup.warning" to "ops" with the template "{event} {plan} ({code})"
      When the backup of "home" succeeds with the warnings "source-unreadable, hook-warning, source-unreadable"
      Then "ops" is sent the message "backup.warning home (source-unreadable, hook-warning)"
      And nothing "ops" is sent shows "canary-url-secret" or "canary-warning-detail"

    Scenario: A template names the repository of the plan that ran
      Given the plan "office" backs up to the repository "cloud"
      And a webhook channel "ops"
      And a rule "failures" that sends "backup.failed" to "ops" with the template "{plan} {repository}"
      When the backup of "office" fails
      Then "ops" is sent the message "office cloud"

    Scenario: A default message is written in the agent's locale
      Given a webhook channel "ops"
      And a rule "failures" that sends "backup.failed" to "ops"
      And the agent's locale is "de-DE"
      When the backup of "home" fails
      Then "ops" is sent the default message in the locale "de-DE"

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

    Scenario: Each event that comes while the configuration can't be read is reported, and nothing is sent
      Given a webhook channel "ops"
      And a rule "failures" that sends "backup.failed" to "ops"
      And the configuration can't be read
      When the backup of "home" starts
      And the backup of "home" fails
      Then no channel is sent anything
      And that the configuration can't be read is reported once for each of the 2 events

    Scenario: A prune that fails names its repository
      Given an ntfy channel "pager"
      And a rule "maintenance" that sends "prune.failed" to "pager"
      When a prune of "nas" fails
      Then "pager" is sent "prune.failed" for the repository "nas" with the code "operation-failed"

    Scenario: A rule that sends to a channel the configuration doesn't have is refused
      Given a webhook channel "ops"
      And a rule "failures" that sends "backup.failed" to "pager"
      Then the configuration is refused because the rule "failures" sends to no channel "pager"
