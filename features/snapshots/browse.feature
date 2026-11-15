@snapshots
Feature: Browsing a snapshot's tree
  A person opens a snapshot and walks its tree one folder at a time: each
  folder's entries with their name, type, size and modification time, in the
  order the snapshot records them. A name shows as the file system wrote it,
  with a character that would break the row or hide in it made visible, and
  a folder is opened by its key, rustic's escaped form of its name.
  A browse is a read: it takes no
  maintenance slot and writes nothing to the operation log. A folder is read
  in pages of at most 1,000 entries, each saying where the next starts, so no
  folder, however large, sizes one answer.

  Background:
    Given a repository with a snapshot of a source folder

  @e2e
  Scenario: Opening a snapshot shows its top folder
    When the agent opens the snapshot
    Then the folder lists "source" as a folder
    And the folder is read in one page

  @e2e
  Scenario: Opening a folder shows each entry's name, type, size and modification time
    When the agent opens the folder "source/docs" of the snapshot
    Then the folder lists "drafts" as a folder
    And the folder lists "notes.md" as a file of 5 bytes modified at "2026-11-01T12:00:00Z"
    And the folder lists "report.txt" as a file of 11 bytes modified at "2026-11-02T08:30:00Z"

  @e2e
  Scenario: An empty folder lists nothing
    When the agent opens the folder "source/empty" of the snapshot
    Then the folder is empty

  @e2e
  Scenario: A folder past the page cap is read in pages
    When the agent opens the folder "source/many" of the snapshot
    Then the page lists 1000 of the folder's 1001 entries and says where the next starts
    When the agent opens the next page of the folder
    Then the page lists 1 of the folder's 1001 entries and is the folder's last

  @e2e
  Scenario: A snapshot that's gone can't be opened
    Given the snapshot has been forgotten
    When the agent opens the snapshot
    Then the snapshot is said to be unknown

  @e2e
  Scenario: A folder the snapshot doesn't hold can't be opened
    When the agent opens the folder "source/gone" of the snapshot
    Then the folder is said to be unknown

  @e2e
  Scenario: A name is shown as it was written, with what would hide in it made visible
    When the agent opens the folder "source/names" of the snapshot
    Then the folder shows its entries as:
      | shown              | kind   |
      | say "hi"           | folder |
      | back\\slash.txt    | file   |
      | café.txt           | file   |
      | new␊line.txt       | file   |
      | tab␉here.txt       | file   |
      | abc[U+202E]txt.exe | file   |

  @e2e
  Scenario: A folder is opened by its key, whatever name it shows
    When the agent opens the folder it shows as 'say "hi"' in "source/names"
    Then the folder lists "inner" as a folder
