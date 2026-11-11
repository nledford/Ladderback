@ui
Feature: Repositories page
  The Repositories page lists the repositories Ladderback backs up to, tests a
  location, connects one that exists and creates one that doesn't, with the
  keyboard alone (A1, A2). A row offers to create its repository where its
  entry says once a backup found no repository there, as an imported
  profile's repository needs, with the password the entry already names; once
  it is created the list is read again, so its button goes, and focus moves to
  the sentence that says it was created. A failed attempt moves focus to an
  error summary
  that names each problem and links to its field, and what the daemon said is
  shown as the catalog writes it (A4, components.md §4.5). A password is never
  shown again once typed (I4). A connected repository is maintained from this
  machine only when the person turns that on, for only one place may maintain
  a repository (I1). In these scenarios {repo} is a scratch copy of
  the demo repository, {empty} and {hot} are empty scratch folders, and the
  demo repository's password is demo-password.

  Scenario: A daemon with no repositories says so
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    Then the page is titled "Repositories – Ladderback"
    And the page shows the paragraph "No repositories yet."
    And the page has no accessibility violations

  Scenario: The repositories page is accessible while it waits for the daemon's first answer
    Given a demo plan "demo-home" with a scratch repository
    And the daemon's answers to the UI are delayed
    When I open the address "/repositories"
    Then the page shows the paragraph "Loading…"
    And the page has no accessibility violations

  Scenario: The page lists the configured repositories without a password
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/repositories"
    Then the table "Repositories" has a row "demo" that shows "Kept in the configuration file"
    And the page doesn't show "demo-password"
    And the page has no accessibility violations

  Scenario: A password written in the configuration file is flagged on its row in words
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/repositories"
    Then the table "Repositories" has a row "demo" that shows "of this repository is written as text in the configuration file."
    And the page doesn't show "demo-password"
    And the page has no accessibility violations

  Scenario: The Repositories link in the navigation moves focus to the page's heading
    When I open the web UI
    And I press Tab until focus is on the link "Repositories"
    And I press Enter
    Then focus is on the main heading "Repositories"
    And the page is titled "Repositories – Ladderback"
    And the live regions stay silent

  Scenario: Testing a repository by keyboard alone shows its ID and keeps nothing
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Location"
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Test the repository"
    And I press Enter
    Then the page shows a repository ID
    And the polite region comes to contain "The repository opens."
    And the page shows the paragraph "No repositories yet."
    And the page doesn't show "demo-password"
    And the page has no accessibility violations

  Scenario: A wrong password moves focus to an error summary that says what the engine said
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Location"
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "not-the-password"
    And I press Tab until focus is on the button "Test the repository"
    And I press Enter
    Then focus is on the heading "There is 1 problem with the form"
    And the page shows a message containing "The operation failed:"
    And the page shows a message containing "Check the location and the password, then try again."
    And the page doesn't show "not-the-password"
    And the page has no accessibility violations

  Scenario: A folder with no repository in it is refused as holding none, without the advice to try again
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Location"
    And I type "{empty}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Test the repository"
    And I press Enter
    Then focus is on the heading "There is 1 problem with the form"
    And the page shows a message containing "There is no repository at this location."
    And the page doesn't show "Check the location and the password, then try again."

  Scenario: A form missing its location and password names both, and each problem links to its field
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the button "Test the repository"
    And I press Enter
    Then focus is on the heading "There are 2 problems with the form"
    And the page has no accessibility violations
    When I press Tab
    Then focus is on the link "Enter where the repository is."
    When I press Enter
    Then focus is on the textbox "Location"
    And the field "Location" is marked invalid
    And the field "Password" is marked invalid

  Scenario: Connecting a repository by keyboard alone adds it to the list
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Name"
    And I type "copy"
    And I press Tab
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Connect the repository"
    And I press Enter
    Then the table "Repositories" has a row "copy" that shows "Kept in the configuration file"
    And the polite region comes to contain "Connected copy."
    And the page doesn't show "demo-password"
    And the page has no accessibility violations

  Scenario: Connecting a repository to maintain from this agent offers its maintenance
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Name"
    And I type "copy"
    And I press Tab
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the checkbox "Maintain this repository from this agent"
    Then the checkbox "Maintain this repository from this agent" is not checked
    And the checkbox "Maintain this repository from this agent" is described by "When this is on, this agent forgets, prunes and checks the repository you connect, and adopts history into its plans. Only one place may maintain a repository, so leave this off when another agent or the hub already does. A repository you create here under a name is always maintained from here."
    When I press Space
    And I press Tab until focus is on the button "Connect the repository"
    And I press Enter
    Then the polite region comes to contain "Connected copy."
    And the checkbox "Maintain this repository from this agent" is not checked
    And the page has no accessibility violations
    When I open the address "/snapshots"
    And I press Tab until focus is on the button "Snapshots of copy"
    And I press Enter
    Then the main content shows the line "Each of these shows what it would do first. Nothing changes until you confirm."
    And the page offers the button "Check the repository"

  Scenario: Connecting under a name already used is refused at the name
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Name"
    And I type "demo"
    And I press Tab
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the checkbox "Maintain this repository from this agent"
    And I press Space
    And I press Tab until focus is on the button "Connect the repository"
    And I press Enter
    Then focus is on the heading "There is 1 problem with the form"
    And the page shows a message containing "already has a repository called demo"
    And the field "Name" is marked invalid
    And the checkbox "Maintain this repository from this agent" is checked
    And the page has no accessibility violations

  Scenario: A name that is not a name is refused at the name
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Name"
    And I type "Not A Name"
    And I press Tab
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Connect the repository"
    And I press Enter
    Then focus is on the heading "There is 1 problem with the form"
    And the field "Name" is marked invalid

  Scenario: Creating a new local repository by keyboard alone shows its ID
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Location"
    And I type "{empty}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Create the repository"
    And I press Enter
    Then the page shows a repository ID
    And the polite region comes to contain "The repository was created."
    And the page has no accessibility violations

  Scenario: Creating a hot/cold repository takes the hot storage too
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Location"
    And I type "{empty}"
    And I press Tab
    And I type "{hot}"
    And I press Tab
    And I type "demo-password"
    And I press Tab until focus is on the button "Create the repository"
    And I press Enter
    Then the page shows a repository ID

  Scenario: Creating where a repository already exists is refused, and says to connect it instead
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Location"
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Create the repository"
    And I press Enter
    Then focus is on the heading "There is 1 problem with the form"
    And the page shows a message containing "because one is already there"
    And the page has no accessibility violations

  Scenario: A repository an imported profile names is created from its row by keyboard alone once a backup found none, and backed up to
    Given a daemon with a profile imported as "demo" whose repository folder is empty
    When I open the address "/repositories"
    Then the table "Repositories" has a row "demo" that shows "Read from the file"
    And the page offers no button "Create demo"
    When a backup of "demo-home" fails because no repository is there
    And I open the address "/repositories"
    Then the table "Repositories" has a row "demo" that shows "Read from the file"
    And the button "Create demo" sits on one line
    And the row header "demo" sits on one line
    And the column header "Name" sits on one line
    When I press Tab until focus is on the button "Create demo"
    And I press Enter
    Then focus is on the message "Created demo."
    And the page shows a repository ID
    And the button "Create demo" leaves the page
    And focus is on the message "Created demo."
    And the page doesn't show "demo-password"
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Test the repository"
    And I press Enter
    Then focus is on the heading "There are 2 problems with the form"
    And the page doesn't show "Created demo."
    When I open the address "/operations"
    And I press Tab until focus is on the button "Start backup of demo-home"
    And I press Enter
    Then the table "Operations" has a row "demo-home" that shows "Succeeded"
    When I open the address "/repositories"
    Then the table "Repositories" has a row "demo" that shows "Read from the file"
    And the page offers no button "Create demo"

  Scenario: A row's Create takes no second press while the list is read again
    Given a daemon with a profile imported as "demo" whose repository folder is empty
    When a backup of "demo-home" fails because no repository is there
    And I open the address "/repositories"
    And the list of repositories is slow to answer from now on
    And I press Tab until focus is on the button "Create demo"
    And I press Enter
    Then focus is on the message "Created demo."
    When I press Shift+Tab until focus is on the button "Create demo"
    And I press Enter
    Then the button "Create demo" leaves the page
    And the page asked to create "demo" once

  Scenario: Creating a repository that is already there from its row says so, and changes nothing
    Given a daemon with a profile imported as "demo" whose repository folder is empty
    When a backup of "demo-home" fails because no repository is there
    And the demo repository is copied into the profile's repository folder
    And I open the address "/repositories"
    And I press Tab until focus is on the button "Create demo"
    And I press Enter
    Then the page shows an alert containing "There is already a repository at the location of"
    And focus stays on the button "Create demo"
    And the page has no accessibility violations

  Scenario: A row's Create where the repository's folder isn't there is refused, and makes no folder
    Given a daemon with a profile imported as "demo" whose repository folder is empty
    When a backup of "demo-home" fails because no repository is there
    And the profile's repository is moved to a folder that isn't there
    And I open the address "/repositories"
    And I press Tab until focus is on the button "Create demo"
    And I press Enter
    Then the page shows an alert containing "This repository wasn't created"
    And focus stays on the button "Create demo"
    And no folder is where the profile's repository was moved to

  Scenario: A page whose daemon has stopped says so when a test gets no answer
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And the daemon has stopped
    And I press Tab until focus is on the textbox "Location"
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Test the repository"
    And I press Enter
    Then the page shows the heading "Cannot reach Ladderback"
    And the page has no accessibility violations

  Scenario: Leaving the page while a repository is being tested breaks nothing
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Location"
    And I type "{repo}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Test the repository"
    And I press Enter
    And I press Shift+Tab until focus is on the link "Operations"
    And I press Enter
    Then focus is on the main heading "Operations"
    And the page raises no script error while the daemon answers

  Scenario Outline: The repositories page is in the language of the page
    When I open the address "/repositories?locale=<locale>"
    Then the page has 4 text boxes
    And every visible text is transformed
    And the page title is transformed

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario Outline: A row's Create is in the language of the page
    Given a daemon with a profile imported as "demo" whose repository folder is empty
    When a backup of "demo-home" fails because no repository is there
    And I open the address "/repositories?locale=<locale>"
    And I press Tab until focus is on the button of the repository "demo"
    Then every visible text but the data of the tables and lists is transformed
    And the page has no accessibility violations

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario Outline: The repositories page reflows on a small screen, a name or a label broken inside a word only when the word is longer than a line
    Given a daemon with a profile imported as "<alias>" whose repository folder is empty
    And a viewport of 320 by 256 pixels
    When a backup of "demo-home" fails because no repository is there
    And I open the address "/repositories"
    Then the table "Repositories" has a row "<alias>" that shows "Read from the file"
    When I press Tab until focus is on the button "Create <alias>"
    Then the page doesn't scroll sideways
    And no text is clipped
    And the button "Create <alias>" breaks <breaks>
    And the row header "<alias>" breaks <breaks>
    And the page has no accessibility violations

    Examples:
      | alias                                                            | breaks                                           |
      | demo                                                             | only at a space or a hyphen                      |
      | offsite-copies-of-the-family-photo-library-kept-on-the-attic-nas | a word only where the word is longer than a line |
      | offsitecopiesofthefamilyphotolibrarykeptontheatticnasandthecloud | a word only where the word is longer than a line |

  Scenario: The repositories page reflows with larger text, a long name offered Create
    Given a daemon with a profile imported as "offsitecopiesofthefamilyphotolibrarykeptontheatticnasandthecloud" whose repository folder is empty
    And a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When a backup of "demo-home" fails because no repository is there
    And I open the address "/repositories"
    And I press Tab until focus is on the button "Create offsitecopiesofthefamilyphotolibrarykeptontheatticnasandthecloud"
    Then the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario: The repositories page reflows with larger text and wider spacing, an error showing
    Given a demo plan "demo-home" with a scratch repository
    And a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the reader increases the text spacing
    When I open the address "/repositories"
    And I press Tab until focus is on the button "Test the repository"
    And I press Enter
    Then focus is on the heading "There are 2 problems with the form"
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations
