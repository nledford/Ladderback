@ui
Feature: Plans page
  The Plans page lists the plans in the configuration file and creates,
  changes and deletes them, with the keyboard alone (A1, A2): its form has a
  field for each setting of a plan, its sources and filters, its
  repository, its schedule, its retention and its hooks. A save, or closing
  the form without saving, returns focus to the button that opened the
  form, or to the page's heading when the list can't be read again and that
  button is gone, unless the reader
  has moved it out of the form while the plan was saved, and leaves open a
  form they opened meanwhile for another plan (A1). A refusal moves
  focus to an error summary that names each problem and links to its field
  (A4, components/accessibility.md §4.5). Deleting a plan asks first, and says what
  becomes of its snapshots: they stay in the repository, and since forget
  removes only snapshots a plan owns (I6), nothing forgets them on its own
  any more; they can still be forgotten by selecting them. A new plan given
  the deleted plan's ID would take its snapshots over and apply its own
  retention to them, and the dialog says so. A plan a notification rule
  names is not deleted, and the dialog says which rule and what to do.

  Scenario: The Plans link in the navigation moves focus to the page's heading
    Given a demo plan "demo-home" with a scratch repository
    When I open the web UI
    And I press Tab until focus is on the link "Plans"
    And I press Enter
    Then focus is on the main heading "Plans"
    And the page is titled "Plans – Ladderback"
    And the table "Plans" has a row "demo-home" that shows "demo"
    And the page has no accessibility violations

  Scenario: Creating a plan by keyboard alone lists it with its next run
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Add a plan"
    And I press Enter
    Then focus is on the heading "New plan"
    When I press Tab until focus is on the textbox "Name"
    And I type "demo-docs"
    And I press Tab until focus is on the textbox "Folders to back up"
    And I type "{source}"
    And I press Tab until focus is on the textbox "Cron schedule"
    And I type "0 3 * * *"
    And I press Tab until focus is on the textbox "Keep the last"
    And I type "7"
    And I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-docs is saved."
    And the table "Plans" has a row "demo-docs" that shows "demo"
    And the row "demo-docs" of the table "Plans" shows a next run
    And the page has no accessibility violations

  Scenario: An invalid schedule is refused at its field
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Add a plan"
    And I press Enter
    Then focus is on the heading "New plan"
    When I press Tab until focus is on the textbox "Name"
    And I type "demo-docs"
    And I press Tab until focus is on the textbox "Folders to back up"
    And I type "{source}"
    And I press Tab until focus is on the textbox "Cron schedule"
    And I type "61 * * * *"
    And I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then focus is on the heading "There is 1 problem with the form"
    And the page has no accessibility violations
    When I press Tab
    Then focus is on the link "Enter a cron schedule of five fields, such as 0 3 * * *."
    When I press Enter
    Then focus is on the textbox "Cron schedule"
    And the field "Cron schedule" is marked invalid
    And the table "Plans" has no row "demo-docs"
    And the page shows no toast

  Scenario: Changing a plan's schedule by keyboard alone keeps it in the list
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    Then focus is on the heading "Change the plan demo-home"
    When I press Tab until focus is on the textbox "Cron schedule"
    And I select all the text and type "30 2 * * *"
    And I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-home is saved."
    And the button "Save the plan" leaves the page
    And focus is on the button "Change demo-home"
    And the table "Plans" has a row "demo-home" that shows "demo"
    And the page has no accessibility violations

  Scenario: A plan that runs at an interval says so at its empty cron field, and keeps it when saved unchanged
    Given a demo plan "demo-home" that runs every "1h" with a scratch repository
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    Then focus is on the heading "Change the plan demo-home"
    And the field "Cron schedule" is described by "as its configuration file sets it. A cron schedule typed here replaces that interval; left empty, the plan keeps it."
    When I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-home is saved."
    And the button "Save the plan" leaves the page
    And the plan "demo-home" still runs every "1h"
    And the row "demo-home" of the table "Plans" shows a next run
    And the page has no accessibility violations

  Scenario: A saved plan is answered in a toast, and focus returns to the button that opened the form
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    And I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the toast reads "The plan demo-home is saved." after its icon "Success"
    And the button "Save the plan" leaves the page
    And focus is on the button "Change demo-home"
    And the polite region comes to contain "The plan demo-home is saved."
    And the page has no accessibility violations

  Scenario: A save whose list can't be read again moves focus to the page's heading
    Given a demo plan "demo-home" with a scratch repository
    And the daemon refuses each read of the plans list after the first
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    Then focus is on the heading "Change the plan demo-home"
    When I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-home is saved."
    And the button "Save the plan" leaves the page
    And focus is on the main heading "Plans"

  Scenario: A save whose list read again gets no answer moves focus to the page's heading
    Given a demo plan "demo-home" with a scratch repository
    And each read of the plans list after the first reaches no daemon
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    Then focus is on the heading "Change the plan demo-home"
    When I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-home is saved."
    And the button "Save the plan" leaves the page
    And focus is on the main heading "Cannot reach Ladderback"

  Scenario: Closing a form whose list can't be read again moves focus to the page's heading
    Given two demo plans "demo-home" and "demo-work" with a scratch repository
    And the daemon refuses each read of the plans list after the first
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    Then focus is on the heading "Change the plan demo-home"
    When I press Shift+Tab until focus is on the button "Delete demo-work"
    And I press Enter
    And I press Tab until focus is on the button "Delete the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-work is deleted."
    And the page shows an error alert containing "could not read or write its configuration file /etc/ladderback/config.toml"
    And the assertive region comes to contain "could not read or write its configuration file /etc/ladderback/config.toml"
    When I press Tab until focus is on the button "Close without saving"
    And I press Enter
    Then the button "Close without saving" leaves the page
    And focus is on the main heading "Plans"

  Scenario: A plan whose form can't be read is an error alert after the Add button, said assertively, and focus stays on its Change button
    Given a demo plan "demo-home" with a scratch repository
    And the daemon refuses the read of the plan "demo-home"
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    Then the page shows an error alert containing "could not read or write its configuration file /etc/ladderback/config.toml"
    And the assertive region comes to contain "could not read or write its configuration file /etc/ladderback/config.toml"
    And focus stays on the button "Change demo-home"
    And the page has no accessibility violations

  Scenario: A reader who leaves the form while its plan is saved keeps their place
    Given a demo plan "demo-home" with a scratch repository
    And the daemon holds back its answer to each read of the plans list after the first
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    Then focus is on the heading "Change the plan demo-home"
    When I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-home is saved."
    When I press Shift+Tab until focus is on the link "Pending changes"
    Then the page shows the button "Save the plan"
    When the daemon has answered the held reads of the plans list
    Then the button "Save the plan" leaves the page
    And focus stays on the link "Pending changes"

  Scenario: A plan's form opened while another plan is saved stays open
    Given two demo plans "demo-home" and "demo-work" with a scratch repository
    And the daemon holds back its answer to each read of the plans list after the first
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    Then focus is on the heading "Change the plan demo-home"
    When I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-home is saved."
    When I press Shift+Tab until focus is on the button "Change demo-work"
    And I press Enter
    Then focus is on the heading "Change the plan demo-work"
    When the daemon has answered the held reads of the plans list
    Then focus stays on the heading "Change the plan demo-work"

  Scenario: Leaving the Plans page while a saved plan is listed again breaks nothing
    Given a demo plan "demo-home" with a scratch repository
    And the daemon holds back its answer to each read of the plans list after the first
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Change demo-home"
    And I press Enter
    Then focus is on the heading "Change the plan demo-home"
    When I press Tab until focus is on the button "Save the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-home is saved."
    When I press Shift+Tab until focus is on the link "Operations"
    And I press Enter
    Then focus is on the main heading "Operations"
    When the daemon has answered the held reads of the plans list
    Then focus stays on the heading "Operations"
    And the page raises no script error while the daemon answers

  Scenario: Deleting a plan asks first and says its snapshots stay
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Delete demo-home"
    And I press Enter
    Then the dialog "Delete the plan demo-home?" opens with focus on the button "Cancel"
    And the dialog "Delete the plan demo-home?" says "Its snapshots stay in the repository."
    And the dialog "Delete the plan demo-home?" says "Nothing forgets them on its own any more. You can still forget them by selecting them on the Snapshots page."
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Delete the plan"
    And I press Enter
    Then the polite region comes to contain "The plan demo-home is deleted. Its snapshots stay in the repository."
    And the table "Plans" has no row "demo-home"

  Scenario: A deleted plan is answered in a toast, said once, and focus goes to the page's heading as its row goes
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Delete demo-home"
    And I press Enter
    And I press Tab until focus is on the button "Delete the plan"
    And I press Enter
    Then the toast reads "The plan demo-home is deleted. Its snapshots stay in the repository." after its icon "Success"
    And the toast is no live region, nor holds one
    And focus is on the main heading "Plans"
    And the polite region reads "The plan demo-home is deleted. Its snapshots stay in the repository."
    And the polite region keeps reading "The plan demo-home is deleted. Its snapshots stay in the repository."
    And the page has no accessibility violations

  Scenario: Deleting a plan says a new plan with its ID would take its snapshots over
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Delete demo-home"
    And I press Enter
    Then the dialog "Delete the plan demo-home?" says "A new plan with the ID demo-home would take these snapshots over and apply its own retention to them."

  Scenario: A plan a notification rule names is not deleted, and the dialog says why in an error alert of its own
    Given a demo plan "demo-home" that the notification rule "failures" names
    When I open the address "/plans"
    And I skip to the content
    And I press Tab until focus is on the button "Delete demo-home"
    And I press Enter
    Then the dialog "Delete the plan demo-home?" opens with focus on the button "Cancel"
    When I press Tab until focus is on the button "Delete the plan"
    And I press Enter
    Then the dialog "Delete the plan demo-home?" alerts "The notification rule failures names the plan demo-home, so the plan was not deleted. Remove the plan from that rule, then delete it again."
    And the page shows an error alert containing "The notification rule failures names the plan demo-home, so the plan was not deleted. Remove the plan from that rule, then delete it again."
    And the dialog "Delete the plan demo-home?" stays open
    And the page has no accessibility violations
    And the table "Plans" has a row "demo-home" that shows "demo"
    When I press Escape
    Then focus is on the button "Delete demo-home"
    And the assertive region stays empty

  Scenario Outline: The Plans page and its form are in the language of the page
    Given a demo plan "demo-home" with a scratch repository
    When I open the address "/plans?locale=<locale>"
    And I skip to the content
    And I press Tab until focus is on the first button of the plan "demo-home"
    And I press Enter
    Then the page shows the row "demo-home"
    And the page shows the form of the plan "demo-home"
    And every visible text but the data of the tables and lists is transformed
    And the page title is transformed
    And the page has no accessibility violations

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |
