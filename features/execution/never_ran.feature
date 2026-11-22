Feature: A queued backup that waited for its keyring and never ran says why
  A backup left queued when the daemon stopped is taken up again by the next
  start, and when its repository's password is in a locked keyring it waits
  for the keyring to be unlocked, showing as queued while it does (decision
  135). The configuration is read again once the wait is over, and by then
  the backup may no longer be able to run: its plan was deleted, its plan
  now backs up to another repository, or the configuration can't be read or
  used. It then ends failed with a reason that says which, never as though
  the daemon had stopped during it, and nothing was written. A deleted or
  moved plan was the person's own change, so its backup sends no
  notification. A configuration that can't be read needs the person to act,
  so that backup sends its failure as any failed backup does, though no rule
  hears it while the configuration that holds the rules can't be read
  (#1293).

  Background:
    Given a webhook channel "ops"
    And a rule "failures" that sends "backup.failed" to "ops"

  Scenario: A backup whose plan was deleted while it waited ends as plan-removed and sends nothing
    Given the log shows a backup of the plan "home" as queued
    And the configuration no longer has the plan "home"
    When the keyring the queued backup of the plan "home" waited for is unlocked
    Then the log shows the backup of the plan "home" as failed with the code "plan-removed"
    And no channel is sent anything

  Scenario: A backup whose plan now backs up to another repository ends as plan-moved and sends nothing
    Given the log shows a backup of the plan "home" on the repository "nas" as queued
    And the plan "home" now backs up to the repository "usb"
    When the keyring the queued backup of the plan "home" waited for is unlocked
    Then the log shows the backup of the plan "home" as failed with the code "plan-moved"
    And no channel is sent anything

  Scenario: A backup whose configuration can't be read once it waited ends as config-unusable
    Given the log shows a backup of the plan "home" as queued
    And the configuration can't be read
    When the keyring the queued backup of the plan "home" waited for is unlocked
    Then the log shows the backup of the plan "home" as failed with the code "config-unusable"
    And no channel is sent anything
