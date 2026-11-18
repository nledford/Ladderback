@ui
Feature: The progress bar
  A progress bar a keyboard user can reach, whose changes are never announced
  and are told at most every 30 seconds, which shows no percentage while its
  size isn't known, and whose fill takes its tone and whose track its thickness
  from the shared vocabulary.

  Scenario: The progress bar is reached by keyboard and its value changes without any announcement
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Tab until focus is on the button "Check the demo folders"
    And I press Tab until focus is on the progressbar "Demo backup progress"
    Then the progress bar "Demo backup progress" reads "Backing up… 40%, 4 GiB of 10 GiB"
    When I press Tab until focus is on the button "Advance the demo progress"
    And I press Enter
    Then the progress bar "Demo backup progress" is drawn at 80 percent
    And the progress bar "Demo backup progress" shows 40 percent
    And the live regions stay silent
    When the page's clock moves on 30 seconds
    Then the progress bar "Demo backup progress" shows 80 percent
    And the progress bar "Demo backup progress" reads "Backing up… 80%, 8 GiB of 10 GiB"
    And the live regions stay silent

  Scenario: A progress bar whose size isn't known yet shows no percentage
    When I open the component gallery
    Then the progress bar "Demo progress of unknown size" shows no percentage
    And the progress bar "Demo progress of unknown size" reads "Working…"
    And the progress bar "Demo progress of a step" reads "Reading the index…"
    And the "Progress" demo has no accessibility violations

  Scenario: Every progress bar is a stop for Tab whatever its size and tone, its focus ring around all it draws, and reads its value
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Tab until focus is on the button "Check the demo folders"
    And I press Tab until focus is on the progressbar "Demo progress of a step"
    Then pressing Tab moves focus to these progress bars in turn, each holding its track and its words inside its focus ring:
      | bar                                   |
      | Demo progress at the Small size       |
      | Demo progress at the Medium size      |
      | Demo progress at the Large size       |
      | Demo progress at the Extra large size |
      | Demo progress in the Primary tone     |
      | Demo progress in the Secondary tone   |
      | Demo progress in the Success tone     |
      | Demo progress in the Warning tone     |
      | Demo progress in the Error tone       |
      | Demo progress in the Inherit tone     |
    And the progress bar "Demo progress in the Inherit tone" reads "Backing up… 60%, 6 GiB of 10 GiB"
    And the live regions stay silent

  Scenario Outline: A progress bar's track is half its size on the shared scale thick, a multiple of the reader's text size, Small unless its page names another
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the track of the progress bar "<bar>" is <times> times as thick as the page's text

    Examples:
      | bar                                   | times |
      | Demo progress at the Small size       | 0.5   |
      | Demo progress at the Medium size      | 0.75  |
      | Demo progress at the Large size       | 1     |
      | Demo progress at the Extra large size | 1.5   |

  Scenario Outline: A progress bar's tone colors its fill only, Primary unless its page names another, on a Ledger Gray track, its words in the text's color, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then each progress bar fills its track in its tone, the track in --surface and its words in the color of the text around it:
      | bar                                 | tone               |
      | Demo progress in the Primary tone   | --accent           |
      | Demo progress in the Secondary tone | --muted            |
      | Demo progress in the Success tone   | --success          |
      | Demo progress in the Warning tone   | --warning          |
      | Demo progress in the Error tone     | --error            |
      | Demo progress in the Inherit tone   | the text around it |

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors, every progress bar fills its track in the text's color, as every tone is, and its track is GrayText
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every progress bar of the gallery fills its track in the color of the text around it, and its track is GrayText

  Scenario Outline: On a small screen with larger text, every progress bar keeps its track and its words inside it, its fill at the track's start, in a page that runs <direction>
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery asking for the locale "<locale>"
    Then every progress bar of the gallery holds its track and its words inside it, its fill at the track's start, in a page that runs <direction>
    And the page doesn't scroll sideways
    And no text is clipped

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |
