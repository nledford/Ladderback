@ui
Feature: Settings lists the folders an AI agent's plan dry run may read
  An AI agent can ask for a plan dry run: Ladderback walks a draft plan's
  sources through its source filter and tells the AI agent how many files and
  bytes it would back up, never a file's name. It reads only within the
  dry-run allowlist, the folders a person lists on Settings' "Dry-run
  allowlist" tab, none by default (§16, decision 89). The tab lists them, adds
  one by its full path and removes one once asked. Adding the home folder
  warns first of what it opens to the AI agent, and suggests a narrower one.

  Scenario: An empty allowlist says that the AI agent's dry run refuses every source
    Given a daemon with sign-in off
    When I open the address "/settings#dry-run-allowlist"
    Then the tab "Dry-run allowlist" is selected and the one Tab stop of its list
    And the page shows the paragraph "No folders yet, so a plan dry run an AI agent asks for refuses every source. Add a folder to let it count what a draft plan would back up there."
    And the page has no accessibility violations

  Scenario: A folder added by keyboard alone is listed, and removing it asks first, starting on Cancel
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    When I open the address "/settings#dry-run-allowlist"
    And I skip to the content
    And I press Tab until focus is on the textbox "Folder"
    And I type "/home/demo/Documents"
    And I press Tab until focus is on the button "Add the folder"
    And I press Enter
    Then the table "Dry-run allowlist" has the row "/home/demo/Documents"
    And the polite region comes to contain "Folder /home/demo/Documents added to the dry-run allowlist."
    And focus is on the button "Add the folder"
    And the page has no accessibility violations
    When I press Shift+Tab until focus is on the button "Remove /home/demo/Documents"
    And I press Enter
    Then the dialog "Remove /home/demo/Documents from the dry-run allowlist" opens with focus on the button "Cancel"
    And the dialog "Remove /home/demo/Documents from the dry-run allowlist" is described as saying "can no longer read this folder"
    And the page has no accessibility violations
    When I press Escape
    Then the dialog "Remove /home/demo/Documents from the dry-run allowlist" is closed
    And focus is on the button "Remove /home/demo/Documents"
    And the table "Dry-run allowlist" has the row "/home/demo/Documents"
    When I press Enter
    And I press Tab until focus is on the button "Remove the folder"
    And I press Enter
    Then the dialog "Remove /home/demo/Documents from the dry-run allowlist" is closed
    And focus is on the heading "Dry-run allowlist"
    And the polite region comes to contain "Folder /home/demo/Documents removed from the dry-run allowlist."
    And the page shows the paragraph "No folders yet, so a plan dry run an AI agent asks for refuses every source. Add a folder to let it count what a draft plan would back up there."

  Scenario: A folder that isn't a full path is the field's error, naming it with a full path to follow, and nothing is saved
    Given a daemon with sign-in off
    When I open the address "/settings#dry-run-allowlist"
    And I skip to the content
    And I press Tab until focus is on the textbox "Folder"
    And I type "Documents"
    And I press Enter
    Then the assertive region comes to contain "Documents is not a full path."
    And the field "Folder" shows the error "is not a full path. Give the folder's whole path, such as"
    And focus is on the textbox "Folder"
    And the textbox "Folder" still holds "Documents"
    And the page shows no alert containing "absolute path"
    And the daemon's dry-run allowlist holds no folder
    And the page has no accessibility violations

  Scenario: Nothing typed, or a folder the list holds already, is the field's error, and nothing is sent
    Given a daemon with sign-in off
    And the dry-run allowlist holds "/home/demo/Documents"
    When I open the address "/settings#dry-run-allowlist"
    And I skip to the content
    And I press Tab until focus is on the textbox "Folder"
    And I press Enter
    Then the field "Folder" shows the error "Give the folder's full path."
    And the assertive region comes to contain "Give the folder's full path."
    And focus is on the textbox "Folder"
    When I type "/home/demo/Documents"
    And I press Enter
    Then the field "Folder" shows the error "The list holds this folder already."
    And the assertive region comes to contain "The list holds this folder already."
    And focus is on the textbox "Folder"
    And the textbox "Folder" still holds "/home/demo/Documents"
    And the table "Dry-run allowlist" has 1 rows
    And the page has no accessibility violations

  Scenario: A folder typed that holds the home folder warns first, as "Add home folder" does, and Cancel returns to the field
    Given a daemon with sign-in off
    When I open the address "/settings#dry-run-allowlist"
    And I skip to the content
    And I press Tab until focus is on the textbox "Folder"
    And I type "/"
    And I press Enter
    Then the dialog "Add your home folder to the dry-run allowlist?" opens with focus on the button "Cancel"
    And the dialog "Add your home folder to the dry-run allowlist?" is described as saying "including ~/.ssh and browser profiles"
    When I press Escape
    Then the dialog "Add your home folder to the dry-run allowlist?" is closed
    And focus is on the textbox "Folder"
    And the textbox "Folder" still holds "/"
    And the daemon's dry-run allowlist holds no folder

  Scenario: A list the daemon can't read is said calmly in its place, with no form, and the panel is a Tab stop
    Given a daemon with sign-in off
    And the daemon can't read its dry-run allowlist
    When I open the address "/settings#dry-run-allowlist"
    Then the page shows a message containing "could not read or write its configuration file"
    And the page shows no alert containing "could not read or write its configuration file"
    And the page offers no button "Add the folder"
    When I skip to the content
    And I press Tab until focus is on the tab "Dry-run allowlist"
    And I press Tab
    Then focus is on the panel of the tab "Dry-run allowlist", which the tab names
    And the page has no accessibility violations

  Scenario: Adding the home folder warns of what it opens, starting on Cancel, and suggests a narrower folder
    Given a daemon with sign-in off
    When I open the address "/settings#dry-run-allowlist"
    And I skip to the content
    And I press Tab until focus is on the button "Add home folder"
    And I press Enter
    Then the dialog "Add your home folder to the dry-run allowlist?" opens with focus on the button "Cancel"
    And the dialog "Add your home folder to the dry-run allowlist?" is described as saying "including ~/.ssh and browser profiles"
    And the dialog "Add your home folder to the dry-run allowlist?" is described as saying "only counts and sizes, never a file's name"
    And the dialog "Add your home folder to the dry-run allowlist?" says "A narrower folder is safer"
    And the page has no accessibility violations
    When I press Enter
    Then the dialog "Add your home folder to the dry-run allowlist?" is closed
    And focus is on the button "Add home folder"
    And the daemon's dry-run allowlist holds no folder
    When I press Enter
    And I press Tab until focus is on the button "Add the home folder"
    And I press Enter
    Then the dialog "Add your home folder to the dry-run allowlist?" is closed
    And the table "Dry-run allowlist" has the row of the daemon's home folder
    And the daemon's dry-run allowlist holds its home folder alone
    And focus is on the heading "Dry-run allowlist"

  Scenario: A screen reader finds each folder as a row of the table the tab's heading names, with a Remove button named for it
    Given a daemon with sign-in off
    And the dry-run allowlist holds "/home/demo/Documents" and "/home/demo/Pictures"
    When I open the address "/settings#dry-run-allowlist"
    Then the table "Dry-run allowlist" has 2 rows
    And the row "/home/demo/Documents" of the table "Dry-run allowlist" holds the button "Remove /home/demo/Documents"
    And the row "/home/demo/Pictures" of the table "Dry-run allowlist" holds the button "Remove /home/demo/Pictures"
    And the page has no accessibility violations

  Scenario: On a small screen with larger text the folders and the form stay whole
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And a daemon with sign-in off
    And the dry-run allowlist holds "/home/demo/Documents/Photographs from the family archive"
    When I open the address "/settings#dry-run-allowlist"
    Then the table "Dry-run allowlist" has the row "/home/demo/Documents/Photographs from the family archive"
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: In a right-to-left language the tab runs right to left, every word of it but the folders translated
    Given a daemon with sign-in off
    And the dry-run allowlist holds "/home/demo/Documents"
    When I open the address "/settings?locale=ar-XB#dry-run-allowlist"
    Then the text direction is "rtl"
    And the page shows the row "/home/demo/Documents"
    And every visible text but the data of the tables and lists is transformed
    And the page doesn't scroll sideways
    And the page has no accessibility violations
