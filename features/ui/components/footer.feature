@ui
Feature: The footer
  The footer (technical design §4.5) closes every page that shows the pages:
  the page's contentinfo landmark, a sibling after the main content, never
  inside it. It is quiet: the running version in Slate Note under a hairline
  rule in Slate Note, no fill and no shadow, plain text that Tab never stops
  on and that is no live region. Its content stands in the page's frame, as
  the main content's does: centered at 60rem in Narrow, across the window in
  Full width, with the same 1rem inline padding, so the version starts where
  the page's text starts, left to right and right to left. On a short page it
  stands at the foot of the window, not in the middle of it.

  Scenario: The footer is plain text: Tab never stops in it, and it is no live region
    When I open the "Footer" demo
    And I press Tab
    Then focus has left the "Footer" demo
    And the footer holds no stop for Tab and no live region
    And the page has one footer, the contentinfo landmark, after the main landmark and outside it

  Scenario Outline: The version is Slate Note text under a Slate Note hairline, with no fill and no shadow, in <colors>
    Given the browser asks for "<colors>"
    When I open the "Footer" demo
    Then the footer draws its version in "--muted" on the page's background, under a 1px rule in "--muted", with no shadow
    And the page has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario Outline: The footer's content stands in the page's frame, its version where the main content's text starts, in <width> <direction>
    Given a viewport of 1600 by 900 pixels
    And the page width chosen in this browser is "<width>"
    When I open the component gallery asking for the locale "<locale>"
    Then the footer's version starts where the main content's text starts, in a frame as wide as the main content's

    Examples:
      | width      | locale | direction     |
      | Narrow     | en-US  | left to right |
      | Full width | en-US  | left to right |
      | Narrow     | ar-XB  | right to left |
      | Full width | ar-XB  | right to left |

  Scenario: On a short page the footer stands at the foot of the window
    Given a viewport of 1280 by 800 pixels
    When I open the address "/_missing"
    Then the footer stands at the foot of the window, and the page doesn't scroll
    And the footer's version starts where the main content's text starts, in a frame as wide as the main content's
