@ui
Feature: The fingerprint
  A key's fingerprint as a person compares it with another screen: in full,
  in groups of four, a group named by its caller. A screen reader speaks it a
  character at a time and never says the hyphens, a copy of it is the plain
  string, and it wraps between its groups, never inside one (§16, decision
  121, "The fingerprint").

  Scenario: A screen reader speaks a fingerprint a character at a time, its hyphens hidden, and Tab never stops in it
    When I open the "Fingerprint" demo
    Then the fingerprint "Demo hub key fingerprint" is spoken a character at a time as "7K3QM9XD2RTV8HFAQW5N"
    When I press Tab
    Then focus has passed the "Fingerprint" demo without stopping in it

  Scenario: A copy of a fingerprint is its plain string, hyphens and all
    When I open the "Fingerprint" demo
    And I select the whole fingerprint "Demo hub key fingerprint"
    Then the selection reads "7K3Q-M9XD-2RTV-8HFA-QW5N"

  Scenario: On a small screen with larger text, a fingerprint wraps between its groups, never inside one
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the "Fingerprint" demo
    Then the fingerprint "Demo hub key fingerprint" takes more than one line, each of its groups whole on one
    And the page doesn't scroll sideways
