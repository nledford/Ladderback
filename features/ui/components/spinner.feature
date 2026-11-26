@ui
Feature: The spinner
  A spinner, which a label names, which never speaks for itself, which takes
  its tone, size and placement from the shared vocabulary and which, for a
  reader who asks for less motion, stands still as three dots, never as a
  part of a circle that could read as an amount done.

  Scenario: A spinner with a visible label is read as its label, and a screen reader skips its drawing
    When I open the component gallery
    Then the "Spinner" demo shows the text "Loading the demo snapshots…" beside a spinner a screen reader skips
    And the button "Save the demo notes" holds a spinner a screen reader skips

  Scenario: A spinner in a line of text stands on the line's baseline, its drawing centered on the text
    When I open the component gallery
    Then the spinner labeled "Loading the demo snapshots…" stands in its line of text, its label on the line's baseline and its drawing centered on the text

  Scenario: A spinner whose label is hidden is an image named by its label
    When I open the component gallery
    Then the "Spinner" demo has the image "Demo spinner at the Medium size"
    And the "Spinner" demo shows no text "Demo spinner at the Medium size"

  Scenario: A spinner is never a live region of its own, which the page's announcer is
    When I open the component gallery
    Then the "Spinner" demo has spinners but no live region

  Scenario: A spinner turns steadily about once a second, and stands still when the reader asks for less motion
    When I open the component gallery
    Then every spinner of the "Spinner" demo turns steadily, once in 0.8 to 1 second
    When the reader asks for less motion
    Then no spinner of the "Spinner" demo moves

  # A still arc over a faint track reads as a gauge a quarter full. The still
  # form is a row of three dots, as an ellipsis says that more is coming, in
  # the same box, so the drawing keeps its size, its place on the line and
  # its name.
  Scenario: For a reader who asks for less motion, a spinner stands still as three dots, which never read as an amount
    When I open the component gallery
    Then no spinner of the "Spinner" demo draws its still dots
    When the reader asks for less motion
    Then every spinner of the "Spinner" demo stands still as a row of three dots in its drawing's color, with no arc or track
    And no spinner of the "Spinner" demo moves
    And the spinner labeled "Loading the demo snapshots…" stands in its line of text, its label on the line's baseline and its drawing centered on the text
    And the "Spinner" demo shows the text "Loading the demo snapshots…" beside a spinner a screen reader skips
    And the "Spinner" demo has the image "Demo spinner at the Medium size"
    And the button "Save the demo notes" holds a spinner a screen reader skips

  Scenario: Under forced colors and less motion, a spinner's still dots take the text's color
    Given the browser asks for "forced colors"
    When I open the component gallery
    And the reader asks for less motion
    Then every spinner of the "Spinner" demo stands still as a row of three dots in the color of the text around it, with no arc or track

  Scenario: A spinner turns its drawing inside the drawing's box, which keeps its length as it turns
    When I open the component gallery
    Then no spinner of the "Spinner" demo reaches out of its drawing's box as it turns

  Scenario Outline: A spinner's size is a multiple of the reader's text size, Small unless its page names another
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the image "<spinner>" is <times> times as tall as the page's text

    Examples:
      | spinner                              | times |
      | Demo spinner at the Small size       | 1     |
      | Demo spinner at the Medium size      | 1.5   |
      | Demo spinner at the Large size       | 2     |
      | Demo spinner at the Extra large size | 3     |

  Scenario: A spinner's tone colors its drawing only, Primary unless its page names another, and its label stays in the text's color
    When I open the component gallery
    Then each spinner of the "Spinner" demo draws its arc in its tone and its track in a faint version of it:
      | spinner                            | tone               |
      | Demo spinner in the Primary tone   | --accent           |
      | Demo spinner in the Secondary tone | --muted            |
      | Demo spinner in the Success tone   | --success          |
      | Demo spinner in the Warning tone   | --warning          |
      | Demo spinner in the Error tone     | --error            |
      | Demo spinner in the Inherit tone   | the text around it |
    And the label of every spinner of the "Spinner" demo is in the color of the text around it

  Scenario: A spinner placed Start, Center or End sits on a line of its own, at the line's start, middle or end
    When I open the component gallery
    Then the spinners on lines of their own read "Loading at the start of the line…", "Loading in the middle of the line…" and "Loading at the end of the line…", in that order
    And the spinners on lines of their own sit at the start, the middle and the end of lines that run left to right

  Scenario: In a page that runs right to left, a spinner's line starts at the right
    When I open the component gallery asking for the locale "ar-XB"
    Then the spinners on lines of their own sit at the start, the middle and the end of lines that run right to left

  Scenario Outline: On a small screen with larger text, a long label on a line of its own wraps beside its spinner, not under it
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery asking for the locale "<locale>"
    Then the label of each spinner on a line of its own wraps onto more lines beside its drawing, which stands centered on the first, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: Under forced colors, a spinner's arc takes the text's color, as every tone does, and its track the color of unavailable text
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every spinner of the "Spinner" demo draws its arc in the color of the text around it and its track in GrayText
