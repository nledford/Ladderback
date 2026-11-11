@ui
Feature: Repositories page
  The Repositories page lists the repositories Ladderback backs up to, tests a
  location, connects one that exists and creates one that doesn't, with the
  keyboard alone (A1, A2). A failed attempt moves focus to an error summary
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

  Scenario: A location with no repository is refused the same way
    Given a daemon with a scratch demo repository and no repositories
    When I open the address "/repositories"
    And I press Tab until focus is on the textbox "Location"
    And I type "{empty}"
    And I press Tab until focus is on the textbox "Password"
    And I type "demo-password"
    And I press Tab until focus is on the button "Test the repository"
    And I press Enter
    Then focus is on the heading "There is 1 problem with the form"
    And the page shows a message containing "The operation failed:"

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

  Scenario: The repositories page reflows on a small screen
    Given a demo plan "demo-home" with a scratch repository
    And a viewport of 320 by 256 pixels
    When I open the address "/repositories"
    Then the page shows the paragraph "Kept in the configuration file"
    And the page doesn't scroll sideways
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
