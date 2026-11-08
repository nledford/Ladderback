@ui
Feature: Pseudolocales
  A development build can show the web UI in two made-up languages, so that
  hard-coded text, text that grows and layout that breaks right to left show up
  before any translation exists (technical design §8.4). `en-XA` accents every
  letter and makes the text longer. `ar-XB` turns the letters upside down and
  lays the page out right to left, as the page of an Arabic reader is. A page
  asks for one with `?locale=` in its address, or its browser lists one among
  its languages. A language nothing serves, or an address that names no
  language, leaves the page in American English.

  Scenario Outline: A pseudolocale tells the browser the language and direction of the page
    When I open the web UI asking for the locale "<locale>"
    Then the page language is "<language>"
    And the text direction is "<direction>"

    Examples:
      | locale | language | direction |
      | en-XA  | en-XA    | ltr       |
      | ar-XB  | ar       | rtl       |

  Scenario Outline: A pseudolocale transforms all the text of the page
    When I open the web UI asking for the locale "<locale>"
    Then every visible text is transformed
    And the page title is transformed

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario: The skip link starts at the right edge of a page that runs right to left
    When I open the web UI asking for the locale "ar-XB"
    And I press Tab
    Then the skip link is at the right edge of the page
    When I press Enter
    Then focus is in the main content

  Scenario: The text of a page that runs right to left starts at the right edge
    When I open the web UI asking for the locale "ar-XB"
    Then the main heading starts at the right edge of the content

  Scenario Outline: The transformed text reflows on a small screen
    Given a viewport of 320 by 256 pixels
    When I open the web UI asking for the locale "<locale>"
    Then every visible text is transformed
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario Outline: The transformed text copes with larger text and wider spacing together
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the reader increases the text spacing
    When I open the web UI asking for the locale "<locale>"
    Then every visible text is transformed
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario Outline: A pseudolocale page has no accessibility violations
    When I open the web UI asking for the locale "<locale>"
    Then every visible text is transformed
    And the page has no accessibility violations

    Examples:
      | locale |
      | en-XA  |
      | ar-XB  |

  Scenario: The page is in American English when nobody asks for another language
    When I open the web UI
    Then the page language is "en-US"
    And the text direction is "ltr"
    And the page is titled "Steading"
    And the main heading reads "Welcome to Steading"

  Scenario: A browser language nothing serves leaves the page in American English
    Given the browser's language is "de-DE"
    When I open the web UI
    Then the page language is "en-US"
    And the text direction is "ltr"
    And the main heading reads "Welcome to Steading"

  Scenario: A browser that lists a pseudolocale among its languages gets it
    Given the browser's language is "ar-XB"
    When I open the web UI
    Then the page language is "ar"
    And the text direction is "rtl"

  Scenario: The locale in the address comes before the language of the browser
    Given the browser's language is "ar-XB"
    When I open the web UI asking for the locale "en-XA"
    Then the page language is "en-XA"
    And the text direction is "ltr"

  Scenario: An address that names no language leaves the page in American English
    When I open the web UI asking for the locale "not a language!"
    Then the page language is "en-US"
    And the main heading reads "Welcome to Steading"

  Scenario Outline: A locale in the address that names no language is ignored, not obeyed
    The browser's own language, a pseudolocale here, still decides.

    Given the browser's language is "ar-XB"
    When I open the web UI asking for the locale "<locale>"
    Then the page language is "ar"
    And the text direction is "rtl"

    Examples:
      | locale                                                            |
      | not a language!                                                   |
      | und                                                               |
      | ../../etc/passwd                                                  |
      | en-aaaaaaaa-bbbbbbbb-cccccccc-dddddddd-eeeeeeee-ffffffff-gggggggg |
