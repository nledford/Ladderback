@ui
Feature: Settings shows its sections as tabs
  Settings holds several sections: the language of the web UI, the width of
  its pages, the admin password, signing in, the API tokens, what AI agents
  may do without a token, the dry-run allowlist and the restart of
  Ladderback. Each is a tab of its own, in a
  column beside its section where the page is wide and in a row above it
  where it is narrow, and the first is
  selected when the page opens. Each tab has an address, Settings' own with
  the section's name after a "#", which opens Settings at that tab. Choosing a
  tab gives the page its address, so loading the page again, as saving the
  language does, comes back to the same tab. While Settings is open, an
  address that names another tab by its fragment alone, as one typed in the
  address bar, chooses that tab, and focus stays where it was. Only how a
  section is reached changes: what each one does is the same.

  Scenario: Settings shows a tab per section in a column beside it, the first selected
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    And a viewport of 1280 by 720 pixels
    When I open the address "/settings"
    Then the tab list of the tab "Language" is named "Settings" and holds the tabs, in order:
      | Language           |
      | Page width         |
      | Admin password     |
      | Signing in         |
      | API tokens         |
      | AI agents          |
      | Dry-run allowlist  |
      | Restart Ladderback |
    And the tab "Language" is selected and the one Tab stop of its list
    And the tab "Language" shows its panel, and every other panel of its list is drawn but hidden
    And the tab list of the tab "Language" is vertical, a column at the start of its panel
    And the page has no accessibility violations

  Scenario: A tab chosen by keyboard alone in the column gives the page its address, and a reload comes back to it
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    And a viewport of 1280 by 720 pixels
    When I open the address "/settings"
    And I skip to the content
    And I press Tab until focus is on the tab "Language"
    And I press the down arrow until focus is on the tab "API tokens"
    Then the tab "API tokens" is selected and the one Tab stop of its list
    And the tab "API tokens" shows its panel, and every other panel of its list is drawn but hidden
    And the page's address ends in the fragment "#api-tokens"
    When I press Tab
    Then focus is on the textbox "Label"
    When I reload the page
    Then the tab "API tokens" is selected and the one Tab stop of its list
    And the tab "API tokens" shows its panel, and every other panel of its list is drawn but hidden

  Scenario: On a small screen the tabs stand in a row above the section, and the arrow keys along the row choose one by keyboard alone
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    And a viewport of 320 by 640 pixels
    When I open the address "/settings"
    And I skip to the content
    And I press Tab until focus is on the tab "Language"
    Then the tab list of the tab "Language" is horizontal, a row above its panel
    When I press ArrowRight
    Then focus is on the tab "Page width"
    And the tab "Page width" is selected and the one Tab stop of its list
    And the tab "Page width" shows its panel, and every other panel of its list is drawn but hidden
    And the page's address ends in the fragment "#page-width"
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

  Scenario Outline: A section's address opens Settings at its tab
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    When I open the address "/settings#<fragment>"
    Then the tab "<tab>" is selected and the one Tab stop of its list
    And the tab "<tab>" shows its panel, and every other panel of its list is drawn but hidden

    Examples:
      | fragment          | tab                |
      | language          | Language           |
      | page-width        | Page width         |
      | admin-password    | Admin password     |
      | signing-in        | Signing in         |
      | api-tokens        | API tokens         |
      | ai-agents         | AI agents          |
      | dry-run-allowlist | Dry-run allowlist  |
      | restart           | Restart Ladderback |

  Scenario: An address that names no section opens Settings at its first tab
    When I open the address "/settings#backups"
    Then the tab "Language" is selected and the one Tab stop of its list
    And the tab "Language" shows its panel, and every other panel of its list is drawn but hidden

  Scenario: Changing the address's fragment to another section's chooses its tab, and focus stays where it was
    Given a viewport of 1280 by 720 pixels
    When I open the address "/settings"
    And I skip to the content
    And I press Tab until focus is on the tab "Language"
    And I change the address's fragment to "#restart"
    Then the tab "Restart Ladderback" is selected and the one Tab stop of its list
    And the tab "Restart Ladderback" shows its panel, and every other panel of its list is drawn but hidden
    And focus stays on the tab "Language"
    And the page's address ends in the fragment "#restart"
    And the page has no accessibility violations
    When I change the address's fragment to "#language"
    Then the tab "Language" is selected and the one Tab stop of its list
    And the tab "Language" shows its panel, and every other panel of its list is drawn but hidden
    And focus stays on the tab "Language"

  Scenario: Changing the address's fragment to one that names no section leaves the tab as it is
    When I open the address "/settings"
    And I skip to the content
    And I press Tab until focus is on the tab "Language"
    And I change the address's fragment to "#page-width"
    And I change the address's fragment to "#backups"
    Then the tab "Page width" is selected and the one Tab stop of its list
    And the tab "Page width" shows its panel, and every other panel of its list is drawn but hidden
    And focus stays on the tab "Language"

  Scenario: Going back to Settings' own address, which names no tab, chooses the first tab
    When I open the address "/settings"
    And I skip to the content
    And I press Tab until focus is on the tab "Language"
    And I change the address's fragment to "#restart"
    Then the tab "Restart Ladderback" is selected and the one Tab stop of its list
    When I go back to the address before
    Then the tab "Language" is selected and the one Tab stop of its list
    And the tab "Language" shows its panel, and every other panel of its list is drawn but hidden
    And the page's address is "/settings"
    And focus stays on the tab "Language"

  Scenario: With sign-in off, a section that holds only words is a Tab stop of its own after its tab
    When I open the address "/settings#api-tokens"
    And I skip to the content
    And I press Tab until focus is on the tab "API tokens"
    And I press Tab
    Then focus is on the panel of the tab "API tokens", which the tab names
    And the page has no accessibility violations

  Scenario: Saving a language chosen from another tab comes back to the Language tab, with focus on its heading
    Given a viewport of 1280 by 720 pixels
    When I open the address "/settings#page-width"
    And I skip to the content
    And I press Tab until focus is on the tab "Page width"
    And I press the up arrow until focus is on the tab "Language"
    Then the page's address ends in the fragment "#language"
    When I press Tab until focus is on the radio "The browser's languages"
    And I press the down arrow until focus is on the radio "English"
    And I press Tab until focus is on the button "Save the language"
    And I press Enter, and the page loads again
    Then focus is on the heading "Language"
    And the tab "Language" is selected and the one Tab stop of its list
    And the tab "Language" shows its panel, and every other panel of its list is drawn but hidden
    And the page's address ends in the fragment "#language"

  Scenario: A token created by keyboard alone, answered once another tab is chosen, shows its dialog over its own tab
    Given a daemon with sign-in on and the admin password "demo-admin-pass"
    And the admin is signed in with "demo-admin-pass"
    And the daemon answers the next creation of an API token 3 seconds late
    When I open the address "/settings#api-tokens"
    And I skip to the content
    And I press Tab until focus is on the textbox "Label"
    And I type "nightly-mcp"
    And I press Tab until focus is on the button "Create the token"
    And I press Enter
    And I press Shift+Tab until focus is on the tab "API tokens"
    And I press the down arrow until focus is on the tab "Restart Ladderback"
    Then the dialog "Copy the new API token" opens with focus on the button "Copy the token"
    And the tab "API tokens" is selected and the one Tab stop of its list
    And the tab "API tokens" shows its panel, and every other panel of its list is drawn but hidden
