@ui
Feature: The component gallery
  The shared component set (technical design §4.5) is exercised in the
  component gallery, a development page at /_gallery, one demo a component,
  each component's scenarios in a feature of their own beside this one. Every
  demo state a component can show (loading, empty, error, success, on and off)
  stays clear of accessibility violations, and each component's structure is
  pinned by an ARIA snapshot. The text of a control, such as a button's label,
  grows with the reader's text size as the page's does, and the gallery still
  fits a small screen when the text has grown (A4). Each demo has an anchor,
  /_gallery#<id>, whose address puts focus on the demo's heading, so a
  scenario can start at its own demo.

  Scenario Outline: The address of the "<demo>" demo puts focus on its heading
    When I open the "<demo>" demo
    Then focus is on the heading "<demo>"

    Examples:
      | demo              |
      | Announcer         |
      | Dialog            |
      | Checkbox          |
      | Radio group       |
      | Status badge      |
      | Badge             |
      | Shared vocabulary |
      | Toast             |
      | Review dialog     |
      | Tree              |
      | Step-up field     |
      | Pause control     |
      | Spinner           |
      | Icon              |
      | Button            |
      | Skeleton          |
      | Banner            |
      | Link              |
      | Alert             |
      | Tabs              |
      | Vertical tabs     |
      | Responsive tabs   |
      | Text field        |
      | Progress          |
      | Tabs starting on a marked tab |
      | Steps             |
      | Live table        |
      | Table             |
      | Tooltip           |

  Scenario: A demo's heading takes focus from the demo's address but is no stop for Tab
    When I open the "Dialog" demo
    And I press Tab
    Then focus is on the button "Open the demo dialog"
    When I press Shift+Tab
    Then focus is on the button "Open the modal stand-in"

  Scenario Outline: Each component's demo states have no accessibility violations
    When I open the component gallery
    Then the "<component>" demo has no accessibility violations


    Examples:
      | component   |
      | Dialog      |
      | Text field  |
      | Checkbox    |
      | Radio group |
      | Status badge |
      | Badge        |
      | Progress    |
      | Toast       |
      | Banner      |
      | Review dialog |
      | Pause control |
      | Tree        |
      | Tabs        |
      | Vertical tabs |
      | Responsive tabs |
      | Tabs starting on a marked tab |
      | Shared vocabulary |
      | Spinner     |
      | Icon        |
      | Button      |
      | Table       |
      | Live table  |
      | Skeleton    |
      | Tooltip     |
      | Link        |
      | Alert       |
      | Step-up field |
      | Steps       |

  Scenario Outline: Each component's structure is pinned by an ARIA snapshot
    When I open the component gallery
    Then the "<component>" demo matches its ARIA snapshot

    Examples:
      | component   |
      | Dialog      |
      | Text field  |
      | Checkbox    |
      | Radio group |
      | Status badge |
      | Badge        |
      | Progress    |
      | Toast       |
      | Banner      |
      | Review dialog |
      | Pause control |
      | Tree        |
      | Tabs        |
      | Vertical tabs |
      | Responsive tabs |
      | Tabs starting on a marked tab |
      | Shared vocabulary |
      | Spinner     |
      | Icon        |
      | Button      |
      | Table       |
      | Live table  |
      | Skeleton    |
      | Tooltip     |
      | Link        |
      | Alert       |
      | Step-up field |
      | Steps       |

  Scenario Outline: A button's label grows with the reader's text size
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "<button>"
    Then the label of the button "<button>" is as large as the page's text
    And the page has no accessibility violations

    Examples:
      | button                    |
      | Open the demo dialog      |
      | Pause the demo updates    |
      | Filled Primary            |

  # The progress demo sits past the tabs demos, so its walk stops on the way.
  Scenario: The progress demo's button label grows with the reader's text size
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Tab until focus is on the button "Check the demo folders"
    And I press Tab until focus is on the button "Advance the demo progress"
    Then the label of the button "Advance the demo progress" is as large as the page's text
    And the page has no accessibility violations

  # The table's sort button sits past the 40 Tab presses a journey allows
  # from the page's start, but within them by Shift+Tab.
  Scenario: A table's sort button's label grows with the reader's text size
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by name"
    Then the label of the button "Sort by name" is as large as the page's text
    And the page has no accessibility violations

  Scenario: The gallery fits a small screen with larger text, its buttons' labels grown with it
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then the label of the button "Open the demo dialog" is as large as the page's text
    And the page doesn't scroll sideways
    And no text is clipped
    And the page has no accessibility violations
