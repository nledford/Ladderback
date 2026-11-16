@ui
Feature: Choosing the language of the web UI
  Settings' Language section chooses the language the web UI is shown in
  (technical design §8.4, decision 101). It offers "The browser's languages",
  chosen until another choice is, then each language this build offers, named
  in itself as its readers write it (English, 日本語). A release offers only
  the languages it lists as complete and current. A development build, which
  these journeys run, offers every catalog and both pseudolocales as well:
  `en-XA` is named "Eƞɠŀişħ" and `ar-XB` "Ǝuƃʅısɥ", English as each writes it.

  The choice is kept in the browser that made it, not by Ladderback, and comes
  ahead of the browser's languages, after the locale a page's address asks for.
  Saving it shows the page again in the language chosen, its language and its
  direction declared to the browser. Loading the page again drops anything else
  Settings was showing, such as a new API token's secret, so the section says
  that saving reloads the page.

  Scenario: Settings offers each language named in itself, and the browser's languages until one is chosen
    When I open the address "/settings"
    Then the group "Show Ladderback in" offers the radios, in order:
      | The browser's languages |
      | English                 |
      | Eƞɠŀişħ                 |
      | Ǝuƃʅısɥ                 |
    And the radio "The browser's languages" of the group "Show Ladderback in" is checked
    And the page has no accessibility violations

  Scenario: Settings says that saving the language reloads the page
    When I open the address "/settings"
    Then the group "Show Ladderback in" is described by "Kept in this browser. Saving reloads the page, which clears anything else Settings is showing. With the browser's languages, Ladderback uses the first of them it offers, and American English if it offers none."

  Scenario Outline: Settings' Language section, its hint included, is transformed in a pseudolocale
    "English" is the one text left as it is, marked as American English.

    When I open the address "/settings?locale=<locale>"
    Then the label of the radio "English" is in the language "en-US", running ltr
    And every visible text is transformed

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario Outline: Each language's name is marked as written in that language
    When I open the address "/settings"
    Then the label of the radio "<name>" is in the language "<language>", running <direction>

    Examples:
      | name    | language | direction |
      | English | en-US    | ltr       |
      | Eƞɠŀişħ | en-XA    | ltr       |
      | Ǝuƃʅısɥ | ar       | rtl       |

  Scenario: A language chosen by keyboard alone shows the page in it, now and after a reload
    When I open the address "/settings"
    And I press Tab until focus is on the radio "The browser's languages"
    And I press the down arrow until focus is on the radio "Ǝuƃʅısɥ"
    Then the radio "Ǝuƃʅısɥ" of the group "Show Ladderback in" is checked
    And the page language is "en-US"
    When I press Tab until focus is on the button "Save the language"
    And I press Enter, and the page loads again
    Then the page language is "ar"
    And the text direction is "rtl"
    And every visible text is transformed
    When I reload the page
    Then the page language is "ar"
    And the text direction is "rtl"

  Scenario: Saving a language shows Settings again with focus on the Language heading, not at the top
    Given the language chosen in this browser is "ar-XB"
    When I open the address "/settings"
    And I press Tab until focus is on the radio "Ǝuƃʅısɥ"
    And I press the up arrow until focus is on the radio "English"
    And I press Tab
    And I press Enter, and the page loads again
    Then focus is on the heading "Language"
    And the page language is "en-US"
    And the text direction is "ltr"

  Scenario: The language chosen comes before the browser's languages
    Given the browser's language is "ar-XB"
    And the language chosen in this browser is "en-XA"
    When I open the web UI
    Then the page language is "en-XA"
    And the text direction is "ltr"

  Scenario Outline: The locale the address asks for comes before the language chosen
    Given the language chosen in this browser is "ar-XB"
    When I open the web UI asking for the locale "<locale>"
    Then the page language is "<locale>"
    And the text direction is "ltr"
    When I open the web UI
    Then the page language is "ar"
    And the text direction is "rtl"

    Examples:
      | locale |
      | en-XA  |
      | en-US  |

  Scenario: Choosing the browser's languages again forgets the language chosen
    Given the language chosen in this browser is "en-US"
    When I open the address "/settings"
    Then the radio "English" of the group "Show Ladderback in" is checked
    When I press Tab until focus is on the radio "English"
    And I press the up arrow until focus is on the radio "The browser's languages"
    And I press Tab until focus is on the button "Save the language"
    And I press Enter, and the page loads again
    Then the radio "The browser's languages" of the group "Show Ladderback in" is checked
    And no language is chosen in this browser

  Scenario Outline: A language kept that this build doesn't offer, as Settings keeps it, leaves the page and Settings on the browser's languages
    Given the language chosen in this browser is "<kept>"
    When I open the address "/settings"
    Then the page language is "en-US"
    And the radio "The browser's languages" of the group "Show Ladderback in" is checked

    Examples:
      | kept  |
      | fr    |
      | EN-xa |

  Scenario: A browser that keeps nothing says so, and the page stays as it was
    Given this browser keeps nothing a page stores
    When I open the address "/settings"
    And I press Tab until focus is on the radio "The browser's languages"
    And I press the down arrow until focus is on the radio "Ǝuƃʅısɥ"
    And I press Tab until focus is on the button "Save the language"
    And I press Enter
    Then the page shows a message containing "This browser didn't keep the language."
    And the page language is "en-US"
    And the page has no accessibility violations
