@restore
Feature: Restoring files from a snapshot
  A person picks one entry of a snapshot's tree, a file, a folder or the
  snapshot's top, and restores it. By default it goes into a new folder, made
  for this restore under the agent's restore folder and named for the
  snapshot's host, its short ID and the time, so no file that was there is
  touched. Restoring in place, over the original paths, is offered only in
  the agent's own web UI, never through the hub, after a dry run has said how
  many files would be added, overwritten or left as they are; an in-place
  restore never deletes a file. Every restore needs step-up confirmation
  first: the admin password with sign-in on, the snapshot's short ID
  otherwise. A restore runs as an operation, one at a time on an agent, with
  the deadline a backup of its repository has; it can be cancelled, and is
  recorded in the history and announced as other operations are. The
  history names the entry it read, by its path from the snapshot's top, with
  any secret of the repository in it masked.

  # The untagged scenarios run the restore use case with a scripted engine
  # and a fake clock; the agent's restore folder is
  # "/home/demo/ladderback-restore", in its home folder, unless a scenario
  # says otherwise, and its clock reads 2027-01-15 08:00:00 in its own time
  # zone, UTC.

  Scenario: A restore goes into a new folder by default
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is on with the admin password "canary-admin-password"
    When the person restores "home/docs" of the snapshot, confirming with "canary-admin-password"
    Then the engine restores "home/docs" into the new folder "/home/demo/ladderback-restore/laptop-3f2a1b4c-20270115-080000"
    And the started restore names its new folder "/home/demo/ladderback-restore/laptop-3f2a1b4c-20270115-080000"
    And the restore ends as succeeded
    And the history shows the restore, started by hand

  Scenario: The history names the entry a restore read by its path from the snapshot's top
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person restores "home/docs" of the snapshot, confirming with "3f2a1b4c"
    Then the history names the entry "/home/docs"

  Scenario: An entry whose path holds the repository's password is named with the password masked
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And the repository "demo" has the password "canary-pass"
    And sign-in is off
    When the person restores "home/canary-pass/notes.md" of the snapshot, confirming with "3f2a1b4c"
    Then the history names the entry "/home/***/notes.md"

  Scenario: A snapshot host that isn't a safe folder name is made one
    Given a snapshot "3f2a1b4c" of the host "../evil/host" in the repository "demo"
    And sign-in is off
    When the person restores "home/docs" of the snapshot, confirming with "3f2a1b4c"
    Then the engine restores "home/docs" into the new folder "/home/demo/ladderback-restore/_evil_host-3f2a1b4c-20270115-080000"

  Scenario: A restore goes into the agent's restore-folder when its configuration sets one
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    And the agent's configuration sets its restore-folder to "/srv/restores"
    When the person restores "home/docs" of the snapshot, confirming with "3f2a1b4c"
    Then the engine restores "home/docs" into the new folder "/srv/restores/laptop-3f2a1b4c-20270115-080000"

  Scenario: Without a home folder or a restore-folder, a restore to a new folder is refused
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    And the agent has no home folder and its configuration sets no restore-folder
    When the person restores "home/docs" of the snapshot, confirming with "3f2a1b4c"
    Then the restore is refused because the agent has no folder to restore into
    And the engine was asked for nothing
    And the history shows no operation

  Scenario: A restore in place says first what its dry run found, and changes nothing until it runs
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is off
    And the original "home/docs" holds 2 files the snapshot changed, 3 it holds as they are and 4 it doesn't hold
    When the person previews restoring "home/docs" of the snapshot in place
    Then the preview says 1 file would be added, 2 overwritten and 3 left as they are
    And the preview says the 4 files the snapshot doesn't hold would stay
    And the history shows no operation
    When the person restores "home/docs" of the snapshot in place, confirming with "3f2a1b4c"
    Then the engine restores "home/docs" over its original paths, deleting nothing
    And the started restore names no new folder
    And the restore ends as succeeded

  Scenario: A restore without the admin password is refused
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is on with the admin password "canary-admin-password"
    When the person restores "home/docs" of the snapshot, confirming with "not-the-password"
    Then the restore is refused because the step-up confirmation failed
    And the engine was asked for nothing
    And the history shows no operation

  Scenario: A wrong step-up password makes the next sign-in wait, as a wrong sign-in does
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is on with the admin password "canary-admin-password"
    When the person restores "home/docs" of the snapshot, confirming with "not-the-password"
    And the person restores "home/docs" of the snapshot, confirming with "canary-admin-password"
    Then the restore is refused while the sign-in wait lasts

  Scenario: With sign-in off, a restore is confirmed by the snapshot's short ID and refused without it
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is off
    When the person restores "home/docs" of the snapshot, confirming with "3f2a1b4d"
    Then the restore is refused because the step-up confirmation failed
    When the person restores "home/docs" of the snapshot, confirming with " 3F2A1B4C "
    Then the restore ends as succeeded

  @e2e
  Scenario: With sign-in on, the snapshot's short ID doesn't confirm a restore
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And a "POST" request for "/api/v1/repositories/demo/snapshots/3f2a1b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708/restore" with the session cookie and its CSRF token
    And the request has the header "Content-Type: application/json"
    And the request has the body "{\"path\":\"home/docs\",\"destination\":\"new-folder\",\"confirmation\":\"3f2a1b4c\"}"
    When the request is sent
    Then the route answers with status 403 and the error code "step-up-failed"

  Scenario: A restore in place asked for through the hub is refused
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is off
    And the agent allows restores from the hub
    When the hub asks for "home/docs" of the snapshot to be restored in place
    Then the restore is refused because in place is offered only in the agent's own web UI
    And the engine was asked for nothing
    And the history shows no operation

  Scenario: A second restore is refused while one runs
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is off
    And the engine restores in 10 steps of 1 minute each
    When the person starts restoring "home/docs" of the snapshot, confirming with "3f2a1b4c"
    And the person restores "home/photos" of the snapshot, confirming with "3f2a1b4c"
    Then the second restore is refused, naming the restore that runs

  Scenario: A restore can start once the one before it has ended
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is off
    When the person restores "home/docs" of the snapshot, confirming with "3f2a1b4c"
    And the person restores "home/photos" of the snapshot, confirming with "3f2a1b4c"
    Then each restore started

  Scenario: The history names the repository a restore reads from, from the moment it is queued
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is off
    When the person starts restoring "home/docs" of the snapshot, confirming with "3f2a1b4c"
    Then the history shows the restore queued, reading from the repository "demo"

  Scenario: A restore cancelled midway is recorded as cancelled
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is off
    And the engine restores in 10 steps of 1 minute each
    When the person restores "home/docs" of the snapshot, confirming with "3f2a1b4c", and cancels it after 3 minutes
    Then the restore ends as cancelled
    And the history shows the restore as it ended

  Scenario: A restore still running at its deadline fails
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is off
    And the engine restores in 30 steps of 1 hour each
    When the person restores "home/docs" of the snapshot, confirming with "3f2a1b4c"
    Then the restore fails because its deadline passed

  Scenario: A restore whose target can't be written fails, and the failure is announced
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    Given sign-in is off
    And the engine can't write the restore's target
    When the person restores "home/docs" of the snapshot, confirming with "3f2a1b4c"
    Then the restore fails because its target can't be written
    And the failure is told as the event "restore.failed"

  @e2e
  Scenario: A restore into a new folder writes the snapshot's files there
    Given a repository with a snapshot of a source folder
    When the agent restores "source/docs" of the snapshot into a new folder
    Then the restore ends as succeeded
    And the new folder holds "notes.md" with "hello" and "report.txt" with "hello world"
    And the started restore named that new folder

  @e2e
  Scenario: A restore in place overwrites what the snapshot changed and keeps what it doesn't hold
    Given a repository with a snapshot of a source folder at its own path
    And the original "source/docs/notes.md" now holds "changed"
    And the original "source/docs" now also holds "extra.txt"
    When the agent previews restoring "source/docs" of the snapshot in place
    Then the preview says 0 files would be added, 1 overwritten and 1 left as they are
    When the agent restores "source/docs" of the snapshot in place
    Then the restore ends as succeeded
    And the original "source/docs/notes.md" holds "hello"
    And the original "source/docs/extra.txt" is still there

  @e2e @needs-file-permissions
  Scenario: A restore whose restore folder can't be written fails and writes nothing
    Given a repository with a snapshot of a source folder
    And the agent's restore folder can't be written
    When the agent restores "source/docs" of the snapshot into a new folder
    Then the restore fails because its target can't be written
    And the agent's restore folder is still empty


  @ui
  Scenario: Restoring a file into a new folder by keyboard alone, confirmed by the snapshot's short ID
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    And I press Tab until focus is on the button "Restore demo-1.txt"
    And I press Enter
    Then the dialog "Restore demo-1.txt" opens with focus inside it
    And focus is on the button "Cancel"
    And the radio "Into a new folder" of the group "Where to restore" is checked
    And the dialog "Restore demo-1.txt" shows the snapshot's short ID as text that can be copied
    When I press Tab until focus is on the textbox "Snapshot ID"
    And I type the newest snapshot's short ID of the plan "home"
    And I press Tab until focus is on the button "Restore"
    And I press Enter
    Then the dialog "Restore demo-1.txt" is closed
    And the polite region comes to contain "Restore of demo-1.txt finished. Its files are in "
    And the polite region names the new folder in the agent's restore folder, holding "demo-1.txt"
    And the page has no accessibility violations

  @ui
  Scenario: A restored file is named in its row on the Operations page
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    And I press Tab until focus is on the button "Restore demo-1.txt"
    And I press Enter
    And I press Tab until focus is on the textbox "Snapshot ID"
    And I type the newest snapshot's short ID of the plan "home"
    And I press Tab until focus is on the button "Restore"
    And I press Enter
    Then the polite region comes to contain "Restore of demo-1.txt finished."
    When I open the address "/operations"
    Then the row "Restore from demo" of the table "Operations" lists "demo-1.txt" in its "Result" column
    And the page has no accessibility violations

  @ui
  Scenario: A restore in place shows its dry run before it can be confirmed
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    And I press Tab until focus is on the button "Restore demo-1.txt"
    And I press Enter
    And I press Tab until focus is on the radio "Into a new folder"
    And I press the down arrow until focus is on the radio "Over the original files"
    Then the dialog "Restore demo-1.txt" comes to say "0 files added, 0 overwritten, 1 left as it is. Files the snapshot doesn't hold stay."
    And the page has no accessibility violations

  @ui
  Scenario: A restore over the original files is held, and says why, until its dry run is read again
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    And I press Tab until focus is on the button "Restore demo-1.txt"
    And I press Enter
    And the repository is moved away
    And I press Tab until focus is on the radio "Into a new folder"
    And I press the down arrow until focus is on the radio "Over the original files"
    And I press Tab until focus is on the button "Restore"
    Then the dialog "Restore demo-1.txt" has its button "Restore" unavailable
    And the button "Restore" is described by "The dry run couldn't be read, so nothing is restored over the original files. Read it again, or restore into a new folder."
    When I press Enter
    Then the dialog "Restore demo-1.txt" politely says "The dry run couldn't be read, so nothing is restored over the original files."
    And the dialog "Restore demo-1.txt" stays open
    When the repository is put back
    And I press Shift+Tab until focus is on the button "Read the dry run again"
    And I press Enter
    Then the dialog "Restore demo-1.txt" comes to say "0 files added, 0 overwritten, 1 left as it is."
    And the dialog "Restore demo-1.txt" alerts nothing
    And the dialog "Restore demo-1.txt" has its button "Restore" available
    And focus is on the button "Read the dry run again"
    And the page has no accessibility violations

  @ui
  Scenario: A wrong confirmation is said inside the dialog, which stays open
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    And I press Tab until focus is on the button "Restore demo-1.txt"
    And I press Enter
    And I press Tab until focus is on the textbox "Snapshot ID"
    And I type "00000000"
    And I press Tab until focus is on the button "Restore"
    And I press Enter
    Then the dialog "Restore demo-1.txt" alerts "That is not this snapshot's ID. Type or paste 8 characters shown above."
    And the field "Snapshot ID" is marked invalid
    And the page has no accessibility violations
