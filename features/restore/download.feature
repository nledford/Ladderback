@restore
Feature: Downloading files from a snapshot
  A person picks one entry of a snapshot's tree and downloads it to the
  computer their browser runs on: a file as itself, a folder or the
  snapshot's top as one archive, in the format they pick. Every download
  needs step-up confirmation first, as a restore does: the admin password
  with sign-in on, the snapshot's short ID otherwise. Once confirmed, the
  download is handed over once, within a minute, and streamed a piece at a
  time, so the agent never holds a whole file. At most two downloads run at
  once on an agent. A download changes nothing, but the history shows each
  one the snapshot held, as it shows a restore: who started it, from which
  repository and how it ended. A reader that goes away stops it.

  # The untagged scenarios run the download use case with a scripted engine
  # and a fake clock. The scripted snapshot holds "home/docs/notes.md", a
  # file of 11 bytes reading "hello world", and the folders "home/docs" and
  # "home/photos". Its engine fails "home/docs/torn.md" once it has handed
  # all of its 11 bytes over, and hands "home/docs/grown.md", of 5 bytes,
  # over as 11.

  Scenario: A file is downloaded as itself once the admin password confirms it
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is on with the admin password "canary-admin-password"
    When the person downloads "home/docs/notes.md" of the snapshot, confirming with "canary-admin-password"
    Then the engine reads "home/docs/notes.md" of the snapshot for a download
    And the download is the file "notes.md" of 11 bytes
    And the download hands over "hello world"
    And the history shows the download, started by hand, as succeeded

  Scenario Outline: A folder is downloaded as one archive in the format picked
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person downloads "home/docs" of the snapshot as <format>, confirming with "3f2a1b4c"
    Then the engine reads "home/docs" of the snapshot for a download as <format>
    And the download is the archive "<name>"

    Examples:
      | format | name     |
      | tar    | docs.tar |
      | zip    | docs.zip |

  Scenario: The snapshot's top is downloaded as an archive named for the snapshot
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person downloads the top of the snapshot as tar, confirming with "3f2a1b4c"
    Then the download is the archive "laptop-3f2a1b4c.tar"

  Scenario: A download without the admin password is refused
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is on with the admin password "canary-admin-password"
    When the person downloads "home/docs/notes.md" of the snapshot, confirming with "not-the-password"
    Then the download is refused because the step-up confirmation failed
    And the engine was asked for nothing
    And the history shows no operation

  Scenario: A wrong step-up password for a download makes the next one wait
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is on with the admin password "canary-admin-password"
    When the person downloads "home/docs/notes.md" of the snapshot, confirming with "not-the-password"
    And the person downloads "home/docs/notes.md" of the snapshot, confirming with "canary-admin-password"
    Then the download is refused while the sign-in wait lasts

  Scenario: With sign-in off, a download is confirmed by the snapshot's short ID and refused without it
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person downloads "home/docs/notes.md" of the snapshot, confirming with "3f2a1b4d"
    Then the download is refused because the step-up confirmation failed
    When the person downloads "home/docs/notes.md" of the snapshot, confirming with " 3F2A1B4C "
    Then the download is the file "notes.md" of 11 bytes

  Scenario: An entry the snapshot doesn't hold is refused before anything is handed over
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person downloads "home/gone.txt" of the snapshot, confirming with "3f2a1b4c"
    Then the download is refused because the snapshot holds no such entry
    And the history shows no operation

  Scenario: A third download is refused while two are under way
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person confirms downloads of "home/docs" and "home/photos" of the snapshot as tar, confirming with "3f2a1b4c"
    And the person downloads "home/docs/notes.md" of the snapshot, confirming with "3f2a1b4c"
    Then the download is refused because two downloads are under way
    When the first download has been handed over to its end
    And the person downloads "home/docs/notes.md" of the snapshot, confirming with "3f2a1b4c"
    Then the download is the file "notes.md" of 11 bytes

  Scenario: A confirmed download is handed over once
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person downloads "home/docs/notes.md" of the snapshot, confirming with "3f2a1b4c"
    And the browser asks for the same download again
    Then the second hand-over is refused because the download is unknown

  Scenario: A confirmed download nobody fetches within a minute is given up
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person confirms a download of "home/docs/notes.md" of the snapshot, confirming with "3f2a1b4c"
    And 61 seconds pass before the browser fetches it
    And the agent gives up the downloads whose minute ran out
    Then the hand-over is refused because the download is unknown
    And the engine's download job was stopped
    And the history shows the download, started by hand, as cancelled

  # The browser's request can end at any moment, as when the person closes
  # the tab, so giving up a download whose minute ran out is the agent's
  # work, never the request's: none is left running in the history.
  Scenario: A browser that hangs up after a download's minute leaves it in the history as cancelled
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person confirms a download of "home/docs/notes.md" of the snapshot, confirming with "3f2a1b4c"
    And 61 seconds pass before the browser fetches it and hangs up while the history is written
    And the agent gives up the downloads whose minute ran out
    Then the engine's download job was stopped
    And the history shows the download, started by hand, as cancelled

  Scenario: A reader that goes away midway stops the download
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person downloads "home/docs" of the snapshot as zip, confirming with "3f2a1b4c", and the browser goes away after the first piece
    Then the engine's download job was stopped
    And the history shows the download, started by hand, as cancelled
    And a new download can start at once

  # A file goes with its size, and an HTTP server reads nothing more of a
  # body once that many bytes are out: the last piece is handed over only
  # once the engine has said the download ended.
  Scenario: A reader that stops at a file's last byte has it in the history as succeeded
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person downloads "home/docs/notes.md" of the snapshot, confirming with "3f2a1b4c", and the browser stops reading at the file's last byte
    Then the history shows the download, started by hand, as succeeded

  # The browser takes a file whose size it has as whole: the last piece is
  # kept back unless the engine ends the download as it said it would.
  Scenario Outline: A file the engine doesn't end as it said ends short of its last byte
    Given a snapshot "3f2a1b4c" of the host "laptop" in the repository "demo"
    And sign-in is off
    When the person downloads "<file>" of the snapshot, confirming with "3f2a1b4c", and the browser reads it as far as it goes
    Then the download ended short of the file's last byte
    And the history shows the download, started by hand, as failed

    Examples:
      | file               |
      | home/docs/torn.md  |
      | home/docs/grown.md |

  @e2e
  Scenario: A downloaded file is the original byte for byte
    Given a repository with a snapshot of a source folder
    When the agent downloads "source/docs/report.txt" of the snapshot
    Then the download is the original "source/docs/report.txt" byte for byte

  @e2e
  Scenario Outline: A folder downloaded as an archive holds each original file byte for byte
    Given a repository with a snapshot of a source folder
    When the agent downloads "source/docs" of the snapshot as <format>
    Then the archive unpacks to "docs" holding the original "source/docs" byte for byte

    Examples:
      | format |
      | tar    |
      | zip    |

  @e2e
  Scenario: A large file is streamed a piece at a time, never held whole
    Given a repository with a snapshot of a source folder holding a file of 64 MiB
    When the agent downloads "source/big.bin" of the snapshot
    Then no piece the agent handed over was larger than 64 KiB
    And the download is the original "source/big.bin" byte for byte

  # Over HTTP a file goes with its size, and the server takes its body as
  # sent once that many bytes are out. The history still says how the
  # download ended, an empty file's too.
  @e2e
  Scenario Outline: A file downloaded whole over HTTP is in the history as succeeded
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's source holds the file "<file>" reading "<content>"
    And the daemon has finished 1 backup of the plan "home"
    When the browser downloads "<file>" of the plan's latest snapshot over HTTP
    Then the browser received "<content>"
    And the daemon's history shows the download as succeeded

    Examples:
      | file      | content     |
      | notes.md  | hello world |
      | empty.txt |             |

  @e2e
  Scenario: With sign-in on, the snapshot's short ID doesn't confirm a download
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And a "POST" request for "/api/v1/repositories/demo/snapshots/3f2a1b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708/download" with the session cookie and its CSRF token
    And the request has the header "Content-Type: application/json"
    And the request has the body "{\"path\":\"home/docs/notes.md\",\"format\":\"tar\",\"confirmation\":\"3f2a1b4c\"}"
    When the request is sent
    Then the route answers with status 403 and the error code "step-up-failed"

  @ui
  Scenario: Downloading a file by keyboard alone, confirmed by the snapshot's short ID
    Given a demo plan "home" with a tiny scratch repository
    And the plan "home" has been backed up 1 times
    When I open the newest snapshot of the plan "home"
    And I press Tab until focus is on a tree item
    And I press ArrowRight until focus is on the tree item "demo-1.txt"
    And I press Tab until focus is on the button "Download demo-1.txt"
    And I press Enter
    Then the dialog "Download demo-1.txt" opens with focus inside it
    And focus is on the button "Cancel"
    When I press Tab until focus is on the textbox "Snapshot ID"
    And I type the newest snapshot's short ID of the plan "home"
    And I press Tab until focus is on the button "Download"
    And I press Enter
    Then the browser saves "demo-1.txt" holding what the plan backed up as "demo-1.txt"
    And the dialog "Download demo-1.txt" is closed
    And the page has no accessibility violations
