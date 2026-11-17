@ui
Feature: The shared components in the gallery
  The shared component set (technical design §4.5) is exercised in the
  component gallery, a development page at /_gallery, with keyboard-only
  journeys: a dialog, a table, sorted by keyboard, which keeps a focused
  control in a row through the row's changes and scrolls sideways in a
  frame of its own on a small screen, a checkbox, a
  radio group, which marks a choice's label written in another language with
  that language and its direction, a progress bar a keyboard user can reach, whose changes
  are never announced and are told at most every 30 seconds, which shows
  no percentage while its size isn't known, and whose fill takes its tone
  and whose track its thickness from the shared vocabulary, a toast, which
  answers what the reader did without taking focus, its icon naming its
  tone, and which goes by itself only when nothing in it needs the reader,
  never while the reader is on it, the
  control that pauses live updates, a status badge for every state an
  operation can be in, whose icon tells the state by its shape, a
  snapshot's tree, one stop for Tab, whose folders are read as they are
  opened, the step-up field, which takes the admin password from a
  password manager or a paste, or shows a short ID to copy and paste back,
  and the shared vocabulary every component takes: its tones, each
  named in words and drawn in a color of its own, a pair of colors for
  each status tone, its words on its fill, its sizes and its
  placements, which follow the page's direction, a spinner, which a
  label names, which never speaks for itself, which takes its tone, size
  and placement from that vocabulary and which stands still for a reader
  who asks for less motion, an icon, decoration a screen reader skips
  unless a label names it, as tall as the text and in the text's color
  unless its page names a size or a tone, sharing the spinner's sizes, and
  a button, pressed by Enter and Space, drawn in an appearance and a tone
  named in words, as tall as a multiple of the text, which stays in the Tab
  order and ignores presses while it is unavailable or busy, keeps its name
  while a spinner stands in it, and says it is pressed by its state and by
  its weight and edge, a text field, its label, description and error
  above it and wired to it, its error announced and drawn in a thicker
  edge in the Error tone, which takes one line, many lines, a password it
  can show, a number or a code, says in words that it is required, read
  only or unavailable, and is as tall as a button of its size, and
  a tooltip, a short note beside a control that keyboard focus shows at
  once and a resting pointer after a moment, which Escape dismisses, which
  stays while the pointer is over it, which stands on the side its control
  asks for where there is room and never covers the control, and which
  describes the control, or shows an icon-only control's name, a link,
  always underlined, which goes to a page of the app without reloading,
  says in words when it goes to another site and marks the page the reader
  is on, a skeleton, the still shape of content on its way, which a screen
  reader and Tab both skip, beside the words that say the wait in a region
  that is busy until the content takes its place, as tall as it was, a set
  of tabs, one stop for Tab, in a row above its panel, in a column at its
  start, or in whichever of the two its container has room for, whose arrow
  keys follow the layout shown and the page's direction, each panel kept as
  it was while another is shown or the layout changes, an alert, an inline
  message about a page's state whose icon names its tone in words, which
  is no live region and no stop for Tab, and whose Dismiss button takes it
  away and leaves focus where its page names, and a badge, a count or a
  tag on a pill of its tone's colors, which says what it counts or names
  in words, never by its color alone, its icon a decoration a screen
  reader skips, and which Tab never stops on.
  Every demo state a component can show (loading, empty, error, success,
  on and off) stays clear
  of accessibility violations, and each component's structure is pinned by an
  ARIA snapshot. The text of a control, such as a button's label, grows with
  the reader's text size as the page's does, and the gallery still fits a
  small screen when the text has grown (A4).
  A banner, a page-wide notice above a page's content, shows its tone by an
  icon named in words, is no live region and no stop for Tab, and its
  Dismiss button takes it away, leaves focus where its page names and is
  remembered in this browser until a newer edition of it comes.

  Scenario: A dialog opens from a button, traps focus, closes on Escape and returns focus
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the demo dialog"
    And I press Enter
    Then focus is inside the dialog "Demo dialog"
    And I press Tab until focus is on the button "Close the demo dialog"
    And I press Tab
    Then focus is on the first control of the dialog "Demo dialog"
    When I press Escape
    Then focus is on the button "Open the demo dialog"

  Scenario: A dialog can start on a button of its own, hand Escape to its page, and close once its page has answered
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the demo warning"
    And I press Enter
    Then the dialog "Demo warning" opens with focus on the button "Keep the demo warning"
    And the dialog "Demo warning" is described as saying "Escape keeps this warning open"
    And the page has no accessibility violations
    When I press Escape
    Then the dialog "Demo warning" politely says "The page keeps the demo warning open."
    And the dialog "Demo warning" stays open
    When I press Tab
    Then focus is on the first control of the dialog "Demo warning"
    And focus is on the button "Answer the demo warning"
    When I press Enter
    Then the dialog "Demo warning" is closed
    And focus is on the button "Open the demo warning"

  Scenario: A dialog can be titled under its section and return focus to the control it names, even one drawn anew
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Turn the demo setting on"
    And I press Enter
    Then the dialog "Demo setting" opens with focus on the button "Keep the demo setting"
    And the dialog "Demo setting" is titled by a heading at level 3
    And the dialog "Demo setting" is described as saying "drawn anew"
    And the page has no accessibility violations
    When I press Tab
    Then focus is on the button "Change the demo setting"
    When I press Enter
    Then the dialog "Demo setting" is closed
    And focus is on the button "Turn the demo setting off"

  Scenario: The step-up field takes a password from a password manager or paste
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the demo step-up"
    And I press Enter
    Then focus is inside the dialog "Demo step-up"
    And the textbox "Admin password" has the autocomplete token "current-password"
    When I press Tab until focus is on the textbox "Admin password"
    And I paste "canary-demo-password"
    Then the field "Admin password" holds what was pasted
    And the page has no accessibility violations

  Scenario: A password shown in the step-up field is hidden again when its dialog is opened again
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the demo step-up"
    And I press Enter
    And I press Tab until focus is on the textbox "Admin password"
    And I type "demo-secret"
    And I press Tab
    Then focus is on the button "Show password" described by "Admin password"
    When I press Space
    Then the field "Admin password" shows what it holds, "demo-secret", its caret 0 characters before its end
    When I press Escape
    Then focus is on the button "Open the demo step-up"
    When I press Enter
    Then focus is inside the dialog "Demo step-up"
    And the field "Admin password" hides what it holds
    And the textbox "Admin password" still holds ""
    And the button "Show password" described by "Admin password" is not pressed

  Scenario: The step-up field shows a short ID to copy and takes it back by paste
    Given the page may use the clipboard
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the demo short-ID step-up"
    And I press Enter
    Then focus is inside the dialog "Demo short-ID step-up"
    And the dialog "Demo short-ID step-up" shows "1a2b3c4d" as text that can be copied
    When I press Tab until focus is on the button "Copy the ID"
    And I press Enter
    Then the clipboard holds "1a2b3c4d"
    And the dialog "Demo short-ID step-up" says "Copied."
    When I press Tab until focus is on the textbox "Demo ID"
    And I paste "1a2b3c4d"
    Then the field "Demo ID" holds what was pasted
    And the page has no accessibility violations

  Scenario: Until its first sort a table shows its rows in the order given, and no column says it is sorted
    When I open the component gallery
    Then the rows of the table "Demo folders" are "photos, music, documents, notes"
    And no column header of the table "Demo folders" says it is sorted

  Scenario: A table's column is sorted by Enter, then the other way by Space, and each sort is said
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by size"
    And I press Enter
    Then the rows of the table "Demo folders" are "notes, documents, photos, music"
    And the column header "Size" alone of the table "Demo folders" says it is sorted "ascending"
    And the polite region reads "Sorted by size, smallest first."
    When I press Space
    Then the rows of the table "Demo folders" are "music, photos, documents, notes"
    And the column header "Size" alone of the table "Demo folders" says it is sorted "descending"
    And the polite region reads "Sorted by size, largest first."

  Scenario: Rows that tie keep their order, whichever way the table is sorted
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by files"
    And I press Enter
    Then the rows of the table "Demo folders" are "music, notes, documents, photos"
    When I press Enter
    Then the rows of the table "Demo folders" are "photos, documents, music, notes"

  Scenario: A table's sort button sits in its column header apart from the header's text
    When I open the component gallery
    Then the button "Sort by changed" sits inside the column header "Changed", apart from its text

  Scenario: A row's action keeps its focus while its row changes and moves
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Sort by files"
    And I press Enter
    And I press Tab until focus is on the button "Add a file to music"
    And I press Enter
    Then the row "music" of the table "Demo folders" reads "13" in the column "Files"
    And the row "Total" of the table "Demo folders" reads "1,541" in the column "Files"
    And the rows of the table "Demo folders" are "notes, music, documents, photos"
    And focus is kept on the button "Add a file to music"

  Scenario: A table's numbers stand at the end of their column in figures of one width
    When I open the component gallery
    Then the column "Size" of the table "Demo folders" is aligned to its end in figures of one width
    And the column "Files" of the table "Demo folders" is aligned to its end in figures of one width
    And the column "Changed" of the table "Demo folders" is aligned to its start

  Scenario Outline: A table that can't show its rows says why in their place
    When I open the component gallery
    And I press Shift+Tab until focus is on the radio "Demo rows"
    And I press the down arrow until focus is on the radio "<choice>"
    Then the "Table" demo says "<sentence>" in the place of the table "Demo folders"

    Examples:
      | choice           | sentence                                              |
      | Demo empty       | No demo folders yet.                                  |
      | Demo unreachable | Ladderback can't be reached, so no demo folders show. |

  Scenario: A table whose rows are on their way says so beside a spinner
    When I open the component gallery
    And I press Shift+Tab until focus is on the radio "Demo rows"
    And I press the down arrow until focus is on the radio "Demo loading"
    Then the "Table" demo says "Loading the demo folders…" in the place of the table "Demo folders"
    And the "Table" demo shows the text "Loading the demo folders…" beside a spinner a screen reader skips

  Scenario Outline: On a small screen with larger text, a table's frame scrolls by keyboard and each row's header stays in view, <direction>
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the region "<table>"
    And I press <key> until the region "<table>" has scrolled to its end
    Then each row header of the table "<table>" stands whole at the start of its frame
    And the page doesn't scroll sideways

    Examples:
      | direction     | table                       | key        |
      | left to right | Demo folders                | ArrowRight |
      | right to left | Demo folders, right to left | ArrowLeft  |

  Scenario Outline: A table's row headers are filled with the page's background, so the cells scrolled under them don't show through, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then each row header of the table "Demo folders" is filled with the page's background color

    Examples:
      | colors        |
      | light colors  |
      | dark colors   |
      | forced colors |

  Scenario: A checkbox turns on and off by keyboard and is described by its hint
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the checkbox "Send a demo notification"
    Then the checkbox "Send a demo notification" is not checked
    And the checkbox "Send a demo notification" is described by "A demo choice: turning it on changes nothing."
    When I press Space
    Then the checkbox "Send a demo notification" is checked
    When I press Space
    Then the checkbox "Send a demo notification" is not checked

  Scenario: A radio group is one stop for Tab, which lands on the choice made, and the arrow keys choose within it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the radio "Every week"
    Then the radio "Every week" of the group "Demo check frequency" is checked
    And the group "Demo check frequency" is described by "A demo choice: choosing one changes nothing."
    When I press the down arrow until focus is on the radio "Every month"
    Then the radio "Every month" of the group "Demo check frequency" is checked
    And the radio "Every week" of the group "Demo check frequency" is not checked
    When I press Tab
    Then focus has left the group "Demo check frequency"
    When I press Shift+Tab
    Then focus is on the radio "Every month"

  Scenario Outline: A radio group marks a choice's label written in another language with that language and the way it runs
    When I open the component gallery
    Then the label of the radio "<name>" is in the language "<language>", running <direction>

    Examples:
      | name    | language | direction |
      | English | en-US    | ltr       |
      | 日本語     | ja       | ltr       |
      | العربية | ar       | rtl       |

  Scenario: The progress bar is reached by keyboard and its value changes without any announcement
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
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

  Scenario: A toast answers the reader politely without taking focus, is no live region, and goes by itself unannounced
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Save the demo settings"
    And I press Enter
    Then the toast reads "Settings saved." after its icon "Success"
    And the toast is no live region, nor holds one
    And focus stays on the button "Save the demo settings"
    And the polite region reads "Settings saved."
    When the toast "Settings saved." is gone
    Then the polite region reads "Settings saved."

  Scenario Outline: A toast in the <tone> tone names its tone by its icon, is said in the <region> region and takes no focus
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "<button>"
    And I press Enter
    Then the toast reads "<words>" after its icon "<name>"
    And the <region> region reads "<words>"
    And focus stays on the button "<button>"
    And the "Toast" demo has no accessibility violations

    Examples:
      | tone    | button                    | words                                                          | name        | region    |
      | primary | Queue a demo check        | Demo check queued.                                             | Information | polite    |
      | success | Save the demo settings    | Settings saved.                                                | Success     | polite    |
      | warning | Back up the demo plan     | Demo backup finished with warnings: 2 files couldn't be read. | Warning     | polite    |
      | error   | Restore the demo snapshot | The demo restore failed.                                       | Error       | assertive |

  Scenario Outline: A toast in the <tone> tone goes by itself <lifetime> seconds after it arrives
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "<button>"
    And I press Enter
    And the page's clock moves on 5 seconds
    Then the toast reads "<words>" after its icon "<name>"
    When the page's clock moves on 2 seconds
    Then the "Toast" demo shows no toast

    Examples:
      | tone    | button                 | words              | name        | lifetime |
      | primary | Queue a demo check     | Demo check queued. | Information | 6        |
      | success | Save the demo settings | Settings saved.    | Success     | 6        |

  Scenario Outline: A toast in the <tone> tone needs the reader, so it stays until the reader dismisses it
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "<button>"
    And I press Enter
    And the page's clock moves on 600 seconds
    Then the toast reads "<words>" after its icon "<name>"

    Examples:
      | tone    | button                    | words                                                          | name    |
      | warning | Back up the demo plan     | Demo backup finished with warnings: 2 files couldn't be read. | Warning |
      | error   | Restore the demo snapshot | The demo restore failed.                                       | Error   |

  Scenario: A toast waits while focus is on it, and has its whole time again once focus leaves
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Save the demo settings"
    And I press Enter
    And the page's clock moves on 3 seconds
    And I press Tab until focus is on the toast's Dismiss button
    And the page's clock moves on 600 seconds
    Then the toast reads "Settings saved." after its icon "Success"
    When I press Tab
    And the page's clock moves on 5 seconds
    Then the toast reads "Settings saved." after its icon "Success"
    When the page's clock moves on 2 seconds
    Then the "Toast" demo shows no toast

  Scenario: A toast waits while the pointer rests on it, and has its whole time again once the pointer leaves
    Given the page's clock can be moved on
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Save the demo settings"
    And I press Enter
    And the page's clock moves on 3 seconds
    And I rest the pointer on the toast
    And the page's clock moves on 600 seconds
    Then the toast reads "Settings saved." after its icon "Success"
    When I move the pointer off the toast
    And the page's clock moves on 5 seconds
    Then the toast reads "Settings saved." after its icon "Success"
    When the page's clock moves on 2 seconds
    Then the "Toast" demo shows no toast

  Scenario Outline: Escape on a toast's Dismiss button leaves the toast to the reader, and <key> takes it away, says nothing more and hands focus to the control its page names, the failure staying on the page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Restore the demo snapshot"
    And I press Enter
    Then the assertive region reads "The demo restore failed."
    And the "Toast" demo shows an alert with the words "The demo restore failed: its target folder can't be written to. Choose another folder and restore again."
    When I press Tab until focus is on the toast's Dismiss button
    And I press Escape
    Then the toast reads "The demo restore failed." after its icon "Error"
    When I press <key>
    Then the "Toast" demo shows no toast
    And focus is on the button "Save the demo settings"
    And the polite region stays empty
    And the "Toast" demo shows an alert with the words "The demo restore failed: its target folder can't be written to. Choose another folder and restore again."

    Examples:
      | key   |
      | Enter |
      | Space |

  Scenario: A new toast takes the place of the one shown
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Restore the demo snapshot"
    And I press Enter
    And I press Shift+Tab until focus is on the button "Save the demo settings"
    And I press Enter
    Then the toast reads "Settings saved." after its icon "Success"
    When I press Tab until focus is on the button "Back up the demo plan"
    And I press Enter
    Then the toast reads "Demo backup finished with warnings: 2 files couldn't be read." after its icon "Warning"

  Scenario Outline: A toast in the <tone> tone lies on its fill with its words, its icon and its Dismiss button in its words' color, edged in its tone, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "<button>"
    And I press Enter
    Then the toast lies on <fill> with its words, its icon and its Dismiss button in <words>, edged 1 pixel wide in <edge>

    Examples:
      | tone    | colors       | button                    | fill         | words        | edge      |
      | primary | light colors | Queue a demo check        | --surface    | --fg         | --accent  |
      | success | light colors | Save the demo settings    | --success-bg | --success-fg | --success |
      | warning | light colors | Back up the demo plan     | --warning-bg | --warning-fg | --warning |
      | error   | light colors | Restore the demo snapshot | --error-bg   | --error-fg   | --error   |
      | primary | dark colors  | Queue a demo check        | --surface    | --fg         | --accent  |
      | success | dark colors  | Save the demo settings    | --success-bg | --success-fg | --success |
      | warning | dark colors  | Back up the demo plan     | --warning-bg | --warning-fg | --warning |
      | error   | dark colors  | Restore the demo snapshot | --error-bg   | --error-fg   | --error   |

  Scenario: Under forced colors a toast is edged in the text's color, its icon in the text's color too
    Given the browser asks for "forced colors"
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Restore the demo snapshot"
    And I press Enter
    Then the toast is edged 1 pixel wide in CanvasText, its icon in the text's color

  Scenario Outline: A toast's icon stands at the start of its line and its Dismiss button at the end, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    And I skip to the content
    And I press Enter on the first button of the toast demo
    Then the toast has its icon before its words and its Dismiss button after them, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, a toast's words wrap inside it, its icon and Dismiss button inside it too
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Back up the demo plan"
    And I press Enter
    Then the toast holds its icon, its words and its Dismiss button inside its edge
    And the page doesn't scroll sideways
    And no text is clipped

  Scenario: The pause control holds the demo's updates and says once what changed when it resumes
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Pause the demo updates"
    Then the button "Pause the demo updates" is not pressed
    And the button "Pause the demo updates" has no description
    When I press Enter
    Then the button "Pause the demo updates" is pressed
    And focus stays on the button "Pause the demo updates"
    And the button "Pause the demo updates" is described by "Live updates are paused. Nothing has changed since."
    And the polite region reads "Live updates are paused."
    And the page has no accessibility violations
    When I press Tab until focus is on the button "Finish a demo run"
    And I press Enter
    And I press Enter
    Then the button "Pause the demo updates" is described by "Live updates are paused. 2 changes are waiting."
    And the polite region keeps reading "Live updates are paused."
    When I press Shift+Tab until focus is on the button "Pause the demo updates"
    And I press Enter
    Then the button "Pause the demo updates" is not pressed
    And focus stays on the button "Pause the demo updates"
    And the button "Pause the demo updates" has no description
    And the polite region reads "Live updates are on again. 2 runs ended while they were paused, and none of them failed."

  Scenario: A message nobody asked for waits while updates are paused and is said with the resume, once
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Pause the demo updates"
    And I press Enter
    And the polite region reads "Live updates are paused."
    And I press Shift+Tab until focus is on the button "Say politely: backup finished"
    And I press Enter
    And I press Enter
    Then the polite region keeps reading "Live updates are paused."
    When I press Tab until focus is on the button "Pause the demo updates"
    And I press Enter
    Then the polite region reads "Live updates are on again. No run ended while they were paused. Backup finished."

  Scenario: A finished run is said at once while updates are not paused
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Finish a demo run"
    And I press Enter
    Then the polite region reads "Backup finished."
    And the button "Pause the demo updates" has no description

  Scenario: The tree is one stop for Tab, and the arrow keys, Home and End move within it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    Then the tree "Demo snapshot" has one tab stop
    And the tree item "documents" is item 1 of 10 at level 1
    When I press ArrowDown
    Then focus is on the tree item "empty"
    When I press End
    Then focus is on the tree item "readme.txt"
    And the tree item "readme.txt" is described by "File, 1.2 KiB"
    When I press Home
    Then focus is on the tree item "documents"
    When I press ArrowDown
    And I press Tab
    Then focus has left the tree "Demo snapshot"
    When I press Shift+Tab
    Then focus is on the tree item "empty"

  Scenario: Right opens a folder, reading it as it opens, and Left closes it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    Then the tree item "documents" is collapsed
    When I press ArrowRight
    Then the tree item "documents" is expanded
    And the tree item "notes.txt" is item 1 of 2 at level 2
    And focus is on the tree item "documents"
    When I press ArrowRight
    Then focus is on the tree item "notes.txt"
    When I press ArrowLeft
    Then focus is on the tree item "documents"
    When I press ArrowLeft
    Then the tree item "documents" is collapsed
    And the tree shows no tree item "notes.txt"
    When I press Enter
    Then the tree item "documents" is expanded

  Scenario: A folder holding nothing says it is empty
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I press ArrowDown
    And I press ArrowRight
    Then the tree item "empty" is expanded
    And the tree item "empty" is described by "Folder, empty"

  Scenario: A folder read in pages ends in a row that reads the next page, and focus moves to what it read
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I type "m" in the tree
    And I press ArrowRight
    Then the tree item "alpha.txt" is item 1 of 5 at level 2
    When I press ArrowDown until focus is on the tree item "Show more: 2 of 5 shown"
    And I press Enter
    Then focus is on the tree item "charlie.txt"
    And the tree item "charlie.txt" is item 3 of 5 at level 2
    And the tree has the tree item "Show more: 4 of 5 shown"

  Scenario: A folder still being read is marked busy
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I type "s" in the tree
    And I press ArrowRight
    Then the tree item "slow" is busy
    And the tree item "slow" is described by "Folder, loading"

  Scenario: A folder that can't be read says why and stays closed
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I type "u" in the tree
    And I press ArrowRight
    Then the page alerts "The demo folder could not be read."
    And the tree item "unreadable" is collapsed
    And focus is on the tree item "unreadable"
    And the "Tree" demo has no accessibility violations

  Scenario: Typing the first letters of a name moves focus to it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    And I type "re" in the tree
    Then focus is on the tree item "readme.txt"
    When I type "l" in the tree
    Then focus is on the tree item "latest"
    And the tree item "latest" is described by "Link"

  Scenario: A row is named by the name it shows, not by the key that opens it
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    Then the tree has the tree item "abc[U+202E]txt.exe"

  Scenario: A name written right to left keeps its direction in a left-to-right page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the treeitem "documents"
    Then the tree item "הערות.txt" shows its name right to left

  Scenario: In a page that runs right to left, Left opens a folder and Right closes it
    When I open the component gallery asking for the locale "ar-XB"
    And I skip to the content
    And I press Tab until focus is on a tree item
    And I press ArrowLeft
    Then the focused tree item is expanded
    When I press ArrowRight
    Then the focused tree item is collapsed

  Scenario: A tab list is one stop for Tab, on its selected tab, and Tab from it goes into that tab's panel
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo overview"
    Then the tab "Demo overview" is selected and the one Tab stop of its list
    And the tab "Demo overview" shows its panel, and every other panel of its list is drawn but hidden
    When I press Tab
    Then focus is on the panel of the tab "Demo overview", which the tab names
    When I press Shift+Tab
    Then focus is on the tab "Demo overview"

  Scenario: The arrow keys move between the tabs and wrap at either end, Home and End go to the first and last, and each tab arrived at is selected
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo overview"
    And I press ArrowRight
    Then focus is on the tab "Demo note"
    And the tab "Demo note" is selected and the one Tab stop of its list
    And the tab "Demo note" shows its panel, and every other panel of its list is drawn but hidden
    When I press ArrowRight
    Then focus is on the tab "Demo history"
    When I press ArrowRight
    Then focus is on the tab "Demo overview"
    And the tab "Demo overview" is selected and the one Tab stop of its list
    When I press ArrowLeft
    Then focus is on the tab "Demo history"
    And the tab "Demo history" is selected and the one Tab stop of its list
    When I press Home
    Then focus is on the tab "Demo overview"
    When I press End
    Then focus is on the tab "Demo history"
    And the tab "Demo history" shows its panel, and every other panel of its list is drawn but hidden
    And the live regions stay silent

  Scenario: Tabs that are chosen by hand move focus with the arrow keys, select a tab only on Enter or Space, and tell the page which tab is chosen
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo log"
    Then the tab list of the tab "Demo log" is described by "These demo tabs are chosen with Enter or Space. The arrow keys only move between them."
    And the demo says "Chosen demo tab: Demo log"
    When I press ArrowRight
    Then focus is on the tab "Demo stats"
    And the tab "Demo log" is selected and the one Tab stop of its list
    And the tab "Demo log" shows its panel, and every other panel of its list is drawn but hidden
    And the demo says "Chosen demo tab: Demo log"
    When I press Tab
    Then focus is on the panel of the tab "Demo log", which the tab names
    When I press Shift+Tab
    Then focus is on the tab "Demo log"
    When I press ArrowRight
    And I press Enter
    Then the tab "Demo stats" is selected and the one Tab stop of its list
    And the tab "Demo stats" shows its panel, and every other panel of its list is drawn but hidden
    And the demo says "Chosen demo tab: Demo stats"
    When I press End
    Then focus is on the tab "Demo plans"
    And the tab "Demo stats" is selected and the one Tab stop of its list
    And the demo says "Chosen demo tab: Demo stats"
    When I press Space
    Then the tab "Demo plans" is selected and the one Tab stop of its list
    And focus is on the tab "Demo plans"
    And the demo says "Chosen demo tab: Demo plans"
    And the live regions stay silent

  Scenario: In tabs that run right to left, the left arrow goes to the next tab and the right arrow to the one before
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "First demo tab"
    Then the tab "Second demo tab" stands to the left of the tab "First demo tab"
    When I press ArrowLeft
    Then focus is on the tab "Second demo tab"
    And the tab "Second demo tab" is selected and the one Tab stop of its list
    When I press ArrowRight
    Then focus is on the tab "First demo tab"
    When I press ArrowRight
    Then focus is on the tab "Third demo tab"
    When I press ArrowLeft
    Then focus is on the tab "First demo tab"

  Scenario: What is typed in a panel stays while another tab is shown, and a panel holding a field is no Tab stop of its own
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo overview"
    And I press ArrowRight
    Then the panel of the tab "Demo note" is no Tab stop of its own
    When I press Tab
    Then focus is on the textbox "Demo note text"
    When I type "Kept while another tab shows"
    And I press Shift+Tab
    Then focus is on the tab "Demo note"
    When I press ArrowRight
    Then the tab "Demo history" shows its panel, and every other panel of its list is drawn but hidden
    When I press ArrowLeft
    Then the textbox "Demo note text" still holds "Kept while another tab shows"

  Scenario Outline: The selected tab is bold with a bar in Ladder Blue on the rule under the list, and the others stand on the rule, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the tab "Demo overview" is bold, with a 3 pixel bar in --accent on the rule under its list
    And the tab "Demo note" is in the text's weight and color, with no bar, on a 1 pixel rule in --muted
    And the tab list of the tab "Demo overview" stands on a 1 pixel rule in --muted
    And the tabs of the list of the tab "Demo overview" touch their neighbours on each line
    And the "Tabs" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors the selected tab keeps its weight and its bar, and the others stand on the rule, in the system's text color
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then the tab "Demo overview" is bold, with a 3 pixel bar in the system's text color on the rule under its list
    And the tab "Demo note" is in the text's weight and color, with no bar, on a 1 pixel rule in the system's text color
    And the tab list of the tab "Demo overview" stands on a 1 pixel rule in the system's text color

  Scenario: On a small screen with larger text the tabs wrap onto more lines, every tab whole and in view on its own rule, above the list's rule
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then every tab of the list of the tab "Demo overview" shows whole inside the window, on more than one line, above the list's rule
    And the tab "Demo overview" stands on a line above the last of its list
    And every tab of the list of the tab "Demo overview" stands on its own rule: the selected one on a 3 pixel bar in --accent, the others on a 1 pixel rule in --muted
    And the page doesn't scroll sideways

  Scenario: Horizontal tabs, the default, stand in a row above their panel, say so, and leave the up and down arrows to the page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo overview"
    Then the tab list of the tab "Demo overview" is horizontal, a row above its panel
    When I press ArrowDown
    Then focus is on the tab "Demo overview"
    When I press ArrowUp
    Then focus is on the tab "Demo overview"
    And the tab "Demo overview" is selected and the one Tab stop of its list

  Scenario: Vertical tabs stand in a column at the start of their panel, say so, and the up and down arrows move between them, wrapping at either end
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo sources"
    Then the tab list of the tab "Demo sources" is vertical, a column at the start of its panel
    And the tab "Demo sources" is selected and the one Tab stop of its list
    When I press ArrowDown
    Then focus is on the tab "Demo times"
    And the tab "Demo times" is selected and the one Tab stop of its list
    And the tab "Demo times" shows its panel, and every other panel of its list is drawn but hidden
    When I press ArrowDown
    Then focus is on the tab "Demo hooks"
    When I press ArrowDown
    Then focus is on the tab "Demo sources"
    When I press ArrowUp
    Then focus is on the tab "Demo hooks"
    And the tab "Demo hooks" is selected and the one Tab stop of its list
    When I press Home
    Then focus is on the tab "Demo sources"
    When I press End
    Then focus is on the tab "Demo hooks"
    When I press ArrowLeft
    Then focus is on the tab "Demo hooks"
    When I press ArrowRight
    Then focus is on the tab "Demo hooks"
    When I press Tab
    Then focus is on the panel of the tab "Demo hooks", which the tab names
    And the live regions stay silent

  Scenario Outline: Vertical tabs, right to left too, and responsive tabs in a column stand on a rule at the end of their list, the selected one bold with a bar in Ladder Blue on it, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the tab "Demo sources" is bold, with a 3 pixel bar in --accent on the rule beside its list
    And the tab "Demo times" is in the text's weight and color, with no bar, on a 1 pixel rule in --muted beside its list
    And the tab list of the tab "Demo sources" stands beside a 1 pixel rule in --muted
    And the tab "First vertical tab" is bold, with a 3 pixel bar in --accent on the rule beside its list
    And the tab "Second vertical tab" is in the text's weight and color, with no bar, on a 1 pixel rule in --muted beside its list
    And the tab list of the tab "First vertical tab" stands beside a 1 pixel rule in --muted
    And the tab "Demo general" is bold, with a 3 pixel bar in --accent on the rule beside its list
    And the tab "Demo address" is in the text's weight and color, with no bar, on a 1 pixel rule in --muted beside its list
    And the tab list of the tab "Demo general" stands beside a 1 pixel rule in --muted
    And the "Vertical tabs" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors vertical tabs, right to left too, and responsive tabs in a column keep the selected tab's weight and bar, and the others stand on the rule, in the system's text color
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then the tab "Demo sources" is bold, with a 3 pixel bar in the system's text color on the rule beside its list
    And the tab "Demo times" is in the text's weight and color, with no bar, on a 1 pixel rule in the system's text color beside its list
    And the tab list of the tab "Demo sources" stands beside a 1 pixel rule in the system's text color
    And the tab "First vertical tab" is bold, with a 3 pixel bar in the system's text color on the rule beside its list
    And the tab "Second vertical tab" is in the text's weight and color, with no bar, on a 1 pixel rule in the system's text color beside its list
    And the tab list of the tab "First vertical tab" stands beside a 1 pixel rule in the system's text color
    And the tab "Demo general" is bold, with a 3 pixel bar in the system's text color on the rule beside its list
    And the tab "Demo address" is in the text's weight and color, with no bar, on a 1 pixel rule in the system's text color beside its list
    And the tab list of the tab "Demo general" stands beside a 1 pixel rule in the system's text color

  Scenario: Vertical tabs in a part of the page that runs right to left stand at its start, on the right, and the down arrow still goes to the next tab
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo sources"
    And I press Tab until focus is on the tab "First vertical tab"
    Then the tab list of the tab "First vertical tab" is vertical, a column at the start of its panel
    When I press ArrowDown
    Then focus is on the tab "Second vertical tab"
    And the tab "Second vertical tab" is selected and the one Tab stop of its list
    When I press ArrowUp
    Then focus is on the tab "First vertical tab"
    When I press ArrowUp
    Then focus is on the tab "Third vertical tab"
    When I press ArrowLeft
    Then focus is on the tab "Third vertical tab"
    When I press ArrowRight
    Then focus is on the tab "Third vertical tab"
    And the tab "Third vertical tab" is selected and the one Tab stop of its list

  Scenario: Responsive tabs are vertical where their container has room for the list beside the panel, and horizontal in a narrow column of the same wide page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo general"
    Then the tab list of the tab "Demo general" is vertical, a column at the start of its panel
    When I press ArrowDown
    Then focus is on the tab "Demo address"
    And the tab "Demo address" is selected and the one Tab stop of its list
    When I press Tab until focus is on the tab "First column tab"
    Then the tab list of the tab "First column tab" is horizontal, a row above its panel
    When I press ArrowLeft
    Then focus is on the tab "Second column tab"
    And the tab "Second column tab" is selected and the one Tab stop of its list
    When I press ArrowDown
    Then focus is on the tab "Second column tab"
    When I press ArrowRight
    Then focus is on the tab "First column tab"

  Scenario: Responsive tabs that change layout keep the selected tab, the focus and what was typed in a panel, and their arrow keys follow the new layout
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the tab "Demo general"
    And I press ArrowDown
    And I press Tab
    Then focus is on the textbox "Demo address text"
    When I type "Kept while the layout changes"
    And I press Shift+Tab
    Then focus is on the tab "Demo address"
    When the window narrows to 480 pixels
    Then the tab list of the tab "Demo address" is horizontal, a row above its panel
    And focus is on the tab "Demo address"
    And the tab "Demo address" is selected and the one Tab stop of its list
    When I press ArrowRight
    Then focus is on the tab "Demo about"
    When I press ArrowLeft
    Then the textbox "Demo address text" still holds "Kept while the layout changes"
    When the window widens to 1280 pixels
    Then the tab list of the tab "Demo address" is vertical, a column at the start of its panel
    And focus is on the tab "Demo address"
    And the tab "Demo address" is selected and the one Tab stop of its list
    When I press ArrowUp
    Then focus is on the tab "Demo general"
    And the live regions stay silent

  Scenario Outline: On a small screen with larger text responsive tabs are horizontal, right to left too, every tab whole and in view, in <colors>
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    And the browser asks for "<colors>"
    When I open the component gallery
    Then the tab list of the tab "Demo general" is horizontal, a row above its panel
    And every tab of the list of the tab "Demo general" shows whole inside the window, on more than one line, above the list's rule
    And the tab list of the tab "First column tab" is horizontal, a row above its panel
    And every tab of the list of the tab "First column tab" shows whole inside the window, on more than one line, above the list's rule
    And the page doesn't scroll sideways
    And the "Responsive tabs" demo has no accessibility violations

    Examples:
      | colors        |
      | light colors  |
      | dark colors   |
      | forced colors |

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
      | Shared vocabulary |
      | Spinner     |
      | Icon        |
      | Button      |
      | Table       |
      | Skeleton    |
      | Tooltip     |
      | Link        |
      | Alert       |
      | Step-up field |

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
      | Shared vocabulary |
      | Spinner     |
      | Icon        |
      | Button      |
      | Table       |
      | Skeleton    |
      | Tooltip     |
      | Link        |
      | Alert       |
      | Step-up field |

  Scenario: A review dialog opens on its dismiss button, keeps Tab inside and returns focus on Escape
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Enter
    Then focus is inside the dialog "Demo review to confirm"
    And focus is on the button "Cancel"
    And the dialog "Demo review to confirm" is titled by a heading at level 3
    When I press Tab until focus is on the button "Confirm the demo review now"
    And I press Tab
    Then focus is on the first control of the dialog "Demo review to confirm"
    When I press Escape
    Then focus is on the button "Open a demo review"

  Scenario: A review dialog's second way to go ahead comes after its confirm button
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Enter
    Then focus is on the button "Cancel"
    When I press Tab
    Then focus is on the button "Confirm the demo review"
    When I press Tab
    Then focus is on the button "Confirm the demo review now"
    When I press Tab
    Then focus is on the first control of the dialog "Demo review to confirm"
    And focus is on the button "Cancel"

  Scenario: A confirmed review dialog closes, returns focus and announces after it closed
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo review"
    And I press Enter
    And I press Tab until focus is on the button "Confirm the demo review"
    And I press Enter
    Then focus is on the button "Open a demo review"
    And the polite region reads "Demo review confirmed."

  Scenario: A review dialog with nothing to do offers only to close
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open an empty demo review"
    And I press Enter
    Then focus is inside the dialog "Demo review with nothing to do"
    And focus is on the button "Close"
    And the dialog "Demo review with nothing to do" offers only the button "Close"
    When I press Enter
    Then focus is on the button "Open an empty demo review"

  Scenario: A review dialog can open on a button of its own, and still closes on Escape and returns focus
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a demo secret"
    And I press Enter
    Then focus is inside the dialog "Demo secret shown once"
    And focus is on the button "Copy the demo secret"
    When I press Tab until focus is on the button "Close"
    And I press Tab
    Then focus is on the first control of the dialog "Demo secret shown once"
    When I press Escape
    Then focus is on the button "Open a demo secret"

  Scenario: A held review dialog says why beside its confirm button, and pressing it only says so
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open a held demo review"
    And I press Enter
    Then focus is on the button "Cancel"
    When I press Tab until focus is on the button "Confirm the held demo review"
    Then the dialog "Demo review on hold" has its button "Confirm the held demo review" unavailable
    And the button "Confirm the held demo review" is described by "The demo review waits for something that never comes."
    When I press Enter
    Then the dialog "Demo review on hold" politely says "The demo review waits for something that never comes."
    And the dialog "Demo review on hold" stays open
    And focus is on the button "Confirm the held demo review"
    When I press Escape
    Then focus is on the button "Open a held demo review"

  Scenario Outline: An open review dialog has no accessibility violations
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "<opener>"
    And I press Enter
    Then focus is inside the dialog "<dialog>"
    And the page has no accessibility violations

    Examples:
      | opener                    | dialog                         |
      | Open a demo review        | Demo review to confirm         |
      | Open an empty demo review | Demo review with nothing to do |
      | Open a demo secret        | Demo secret shown once         |
      | Open a held demo review   | Demo review on hold            |

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

  Scenario: Each state of a status badge has an icon of its own beside its text, which a screen reader skips
    When I open the component gallery
    Then each of the 7 badges of the "Status badge" demo has an icon of its own, in its text's color, which a screen reader skips

  Scenario: No status badge moves, the running one's icon either, however long the operation runs
    When I open the component gallery
    Then none of the 7 badges of the "Status badge" demo moves

  Scenario: A status badge's icon grows with the reader's text size
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the icon of each badge of the "Status badge" demo is as tall as its text

  Scenario: A badge says what it counts or names in words, with no role, its icon a decoration a screen reader skips, and Tab never stops on it
    When I open the component gallery
    Then every badge of the "Badge" demo says its words with no role and nothing in it to focus, any icon in it a decoration a screen reader skips
    When I skip to the content
    And I press Tab until focus is on the radio "English"
    And I press Tab
    Then focus has passed the "Badge" demo without stopping in it

  Scenario Outline: Each tone of a badge is named in words and drawn on its own pair of tokens, its icon in its words' color, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the badges of the "Badge" demo named for their tones are drawn on their pairs of tokens, at least 4.5:1 apart:
      | badge     | words              | fill               |
      | Primary   | --badge-loading-fg | --badge-loading-bg |
      | Secondary | --badge-empty-fg   | --badge-empty-bg   |
      | Success   | --success-fg       | --success-bg       |
      | Warning   | --warning-fg       | --warning-bg       |
      | Error     | --error-fg         | --error-bg         |

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a badge's words and icon take the system's text color, edged 1 pixel wide in it, so its words tell the tones apart and each badge stands apart from the next
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every badge of the "Badge" demo draws its words and icon in the color of the text around it, edged 1 pixel wide in that color

  Scenario Outline: A badge's icon stands before its words, centered on their line and as tall as the text, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    Then the icon of each badge of the badge demo stands before its words, centered on their line and as tall as the text, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, a badge's words wrap within the room it is given
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then every badge of the "Badge" demo keeps within the room it is given
    And the page doesn't scroll sideways

  Scenario: Each tone is named in words, and draws its text and its swatch in a color of its own, which a screen reader skips
    When I open the component gallery
    Then each of the 6 tones of the "Shared vocabulary" demo is named in words, its swatch drawn in its text's color and skipped by a screen reader
    And the 5 tones of the "Shared vocabulary" demo other than Inherit each have a color of their own
    And the tone "Inherit" of the "Shared vocabulary" demo takes the color of the text around it

  Scenario: Under forced colors every tone draws its text and its swatch in the system's text color, so its words tell it apart
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then each of the 6 tones of the "Shared vocabulary" demo is named in words, its swatch drawn in its text's color and skipped by a screen reader
    And the 6 tones of the "Shared vocabulary" demo all draw in the color of the text around them

  Scenario Outline: Each status tone has a pair of colors of its own, its words on its fill, named in words, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then each status pair of the "Shared vocabulary" demo is named in words, drawn on its fill:
      | pair    | words        | fill         |
      | Success | --success-fg | --success-bg |
      | Warning | --warning-fg | --warning-bg |
      | Error   | --error-fg   | --error-bg   |

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario Outline: A status badge of an operation that succeeded or failed is drawn in its status tone's pair, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then each of these badges of the "Status badge" demo is drawn on its fill:
      | badge                   | words        | fill         |
      | Succeeded               | --success-fg | --success-bg |
      | Succeeded with warnings | --success-fg | --success-bg |
      | Failed                  | --error-fg   | --error-bg   |

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario Outline: Start, Center and End place a part at its line's start, middle and end, in the page's direction
    When I open the component gallery asking for the locale "<locale>"
    Then the placements of the vocabulary demo put each part where they say, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: A spinner with a visible label is read as its label, and a screen reader skips its drawing
    When I open the component gallery
    Then the "Spinner" demo shows the text "Loading the demo snapshots…" beside a spinner a screen reader skips
    And the button "Save the demo notes" holds a spinner a screen reader skips

  Scenario: A spinner in a line of text stands on the line's baseline, its drawing centered on the text
    When I open the component gallery
    Then the spinner labelled "Loading the demo snapshots…" stands in its line of text, its label on the line's baseline and its drawing centered on the text

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

  Scenario: An icon beside words is decoration a screen reader skips, in the words' color, as tall as them and centered on them
    When I open the component gallery
    Then the "Icon" demo shows the text "The next demo backup starts at 02:00 every night." beside an icon a screen reader skips, in the text's color, as tall as it and centered on it

  Scenario: An icon-only button is named by its own label, and the icon inside it is skipped by a screen reader
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Demo notifications"
    Then the button "Demo notifications" shows only an icon, which a screen reader skips
    And the button "Demo notifications" is not pressed
    And the button "Demo notifications" shows a crossed-out bell
    When I press Space
    Then the button "Demo notifications" is pressed
    And the button "Demo notifications" shows only an icon, which a screen reader skips
    And the button "Demo notifications" shows a bell, not crossed out
    When I press Space
    Then the button "Demo notifications" is not pressed
    And the button "Demo notifications" shows a crossed-out bell

  Scenario: An icon given a label is an image named by it
    When I open the component gallery
    Then the "Icon" demo has the image "Demo icon at the Medium size"
    And the "Icon" demo shows no text "Demo icon at the Medium size"

  Scenario Outline: An icon's size is a multiple of the reader's text size, Small unless its page names another, as a spinner's of the same size is
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the image "<icon>" is <times> times as tall as the page's text
    And the image "<icon>" is as tall as the image "<spinner>"

    Examples:
      | icon                              | spinner                              | times |
      | Demo icon at the Small size       | Demo spinner at the Small size       | 1     |
      | Demo icon at the Medium size      | Demo spinner at the Medium size      | 1.5   |
      | Demo icon at the Large size       | Demo spinner at the Large size       | 2     |
      | Demo icon at the Extra large size | Demo spinner at the Extra large size | 3     |

  Scenario: An icon's tone colors its glyph, named in words beside it, and an icon in no tone takes the color of the text around it
    When I open the component gallery
    Then each tone of the "Icon" demo, named in words, draws its icon in it:
      | tone      | color              |
      | Primary   | --accent           |
      | Secondary | --muted            |
      | Success   | --success          |
      | Warning   | --warning          |
      | Error     | --error            |
      | Inherit   | the text around it |

  Scenario: Under forced colors every icon is drawn in the color of the text around it, as every tone is
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every icon of the "Icon" demo is drawn in the color of the text around it

  Scenario: A button is pressed by Enter and by Space
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Enter
    Then the "Button" demo shows the sentence "The demo buttons were pressed once."
    When I press Space
    Then the "Button" demo shows the sentence "The demo buttons were pressed 2 times."

  Scenario: A button draws its appearance in its tone, both named in words, Outlined in Secondary unless its page names others
    When I open the component gallery
    Then each button of the "Button" demo draws its appearance in its tone:
      | button                         | label              | fill     | edge               |
      | Filled Primary                 | --on-fill          | --accent | --accent           |
      | Filled Secondary               | --on-fill          | --muted  | --muted            |
      | Filled Success                 | --on-fill          | --success | --success         |
      | Filled Warning                 | --on-fill          | --warning | --warning         |
      | Filled Error                   | --on-fill          | --error  | --error            |
      | Outlined Primary               | --accent           | none     | --accent           |
      | Outlined Secondary             | --muted            | none     | --muted            |
      | Outlined Success               | --success          | none     | --success          |
      | Outlined Warning               | --warning          | none     | --warning          |
      | Outlined Error                 | --error            | none     | --error            |
      | Outlined Inherit               | the text around it | none     | the text around it |
      | Ghost Primary                  | --accent           | none     | none               |
      | Ghost Secondary                | --muted            | none     | none               |
      | Ghost Success                  | --success          | none     | none               |
      | Ghost Warning                  | --warning          | none     | none               |
      | Ghost Error                    | --error            | none     | none               |
      | Ghost Inherit                  | the text around it | none     | none               |
      | Demo button at the Medium size | --muted            | none     | --muted            |

  Scenario Outline: A button is a multiple of the reader's text size tall, Medium unless its page names another
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the button "<button>" is <times> times as tall as the page's text

    Examples:
      | button                              | times |
      | Demo button at the Small size       | 1.75  |
      | Demo button at the Medium size      | 2     |
      | Demo button at the Large size       | 2.5   |
      | Demo button at the Extra large size | 3     |
      | Demo details at the Small size      | 1.75  |
      | Demo details at the Extra large size | 3    |

  Scenario Outline: An icon-only button is named by its label, and stays square and at least 24 pixels a side at every size
    When I open the component gallery
    Then the button "<button>" shows only an icon, which a screen reader skips
    And the button "<button>" is square, at least 24 pixels a side

    Examples:
      | button                               |
      | Demo details at the Small size       |
      | Demo details at the Medium size      |
      | Demo details at the Large size       |
      | Demo details at the Extra large size |

  Scenario Outline: A button's icon stands before its label, as tall as its text, in the page's direction
    When I open the component gallery asking for the locale "<locale>"
    Then every button with an icon and words shows its icon before its words, as tall as them, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: An unavailable button stays in the Tab order, says it is unavailable and ignores Enter and Space
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Delete the demo snapshot"
    Then the button "Delete the demo snapshot" says it is unavailable, and is not disabled
    When I press Enter
    And I press Space
    Then focus stays on the button "Delete the demo snapshot"
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."

  Scenario: A busy button keeps its name and its place in the Tab order, shows a spinner a screen reader skips, and ignores presses
    When I open the component gallery
    And the widths of the buttons "Check the demo repository" and "Refresh the demo" are noted
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then the button "Demo work in progress" is pressed
    And the button "Save the demo plan" is busy, a spinner a screen reader skips before its label
    And the button "Check the demo repository" is busy, a spinner a screen reader skips in its icon's place
    And the button "Refresh the demo" is busy, a spinner a screen reader skips in its icon's place
    And the buttons "Check the demo repository" and "Refresh the demo" keep the widths noted
    And the "Button" demo has no accessibility violations
    When I press Tab until focus is on the button "Check the demo repository"
    And I press Enter
    And I press Space
    Then focus stays on the button "Check the demo repository"
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."
    When I press Shift+Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then the button "Check the demo repository" is not busy, its icon back in its place

  Scenario: A toggle button keeps its one name and says it is pressed, drawn bolder with an inset edge
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo's hidden files"
    Then the button "Show the demo's hidden files" is not pressed
    And the button "Show the demo's hidden files" is drawn at its resting weight and edge
    When I press Space
    Then the button "Show the demo's hidden files" is pressed
    And the button "Show the demo's hidden files" is drawn bolder, with an inset edge
    And the "Button" demo has no accessibility violations
    When I press Enter
    Then the button "Show the demo's hidden files" is not pressed
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."

  Scenario: A submit button sends its form from the keyboard
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Enter
    Then the polite region comes to contain "Demo form sent."
    And the "Button" demo shows the sentence "The demo buttons haven't been pressed yet."

  Scenario: A busy submit button sends no form, from Enter or from Space
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Space
    And I press Tab until focus is on the button "Send the demo form"
    Then the button "Send the demo form" is busy, a spinner a screen reader skips before its label
    When I press Enter
    And I press Space
    Then focus stays on the button "Send the demo form"
    And the polite region stays empty

  Scenario: Under forced colors a button takes the reader's button colors, an unavailable one the color of unavailable text, and pressed keeps its weight and edge
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every button of the "Button" demo is drawn in the reader's button colors, an unavailable one in the color of unavailable text
    When I skip to the content
    And I press Tab until focus is on the button "Open the modal stand-in"
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo's hidden files"
    And I press Space
    Then the button "Show the demo's hidden files" is drawn bolder, with an inset edge
    When I press Shift+Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then the button "Demo work in progress" is drawn bolder, with an inset edge
    And every pressed button of the "Button" demo is drawn in the reader's button colors, a Ghost one's edge too

  Scenario: Under forced colors a busy button keeps the reader's button text, apart from an unavailable one
    Given the browser asks for "forced colors"
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Space
    Then every busy button of the "Button" demo is drawn in the reader's button colors, its label and edge in the button text, not the color of unavailable text

  Scenario: A skeleton is hidden from a screen reader, has no role and is never a stop for Tab
    When I open the component gallery
    Then every skeleton of the "Skeleton" demo is hidden from a screen reader, with no role and nothing inside it to focus
    When I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab
    Then focus is on the button "Show the demo plan"

  Scenario: A region says its wait in words beside its skeleton while it is busy, and once the content comes it is no longer busy, keeps its height and leaves focus where it was
    When I open the component gallery
    Then the region of the "Skeleton" demo is busy and shows the words "Loading the demo plan…" beside a skeleton
    When I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And the height of the region of the "Skeleton" demo is noted
    And I press Enter
    Then the region of the "Skeleton" demo is no longer busy and shows "Backs up the demo folder." in place of its skeleton
    And the region of the "Skeleton" demo is as tall as it was, within 2 pixels
    And focus stays on the button "Show the demo plan"
    And the polite region stays empty
    And the assertive region stays empty

  Scenario: A skeleton never moves, whether or not the reader asks for less motion
    When I open the component gallery
    Then no skeleton of the "Skeleton" demo moves, fades or changes
    When the reader asks for less motion
    Then no skeleton of the "Skeleton" demo moves, fades or changes

  Scenario Outline: A skeleton's lines fill their room, each as tall as the text on a line of its own, the last one shorter, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    Then the skeleton lines of the skeleton demo fill their room, each 1 em tall on a line of the text, the last one shorter and at the line's start, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, a skeleton's block grows with the text and keeps to the room it is given
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then the skeleton block of the "Skeleton" demo is 2.5 rem tall and 20 rem wide, or as wide as its room where that is less
    And the page doesn't scroll sideways

  Scenario Outline: A skeleton is Pencil Gray, which shows on the page and on Ledger Gray alike, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then every skeleton of the "Skeleton" demo is filled in --skeleton, at least 1.4:1 apart from what lies behind it, on the page and on --surface

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a skeleton is edged 1 pixel wide in the color of unavailable text, so its shape still shows
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every skeleton of the "Skeleton" demo is edged 1 pixel wide in GrayText

  Scenario: A banner is no live region and no stop for Tab, and its icon names its tone in words
    When I open the component gallery
    Then no banner of the "Banner" demo is a live region, nor holds one
    And the banners of the "Banner" demo show these icons, each named for its tone, one shape per tone, in this order:
      | tone    | name        |
      | primary | Information |
      | warning | Warning     |
      | primary | Information |
    When I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab
    Then focus is on the button "Dismiss" of the "Banner" demo

  Scenario Outline: <key> on a banner's Dismiss button takes the banner away, says nothing, leaves focus on the control its page names and keeps it away on the next load
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the button "Dismiss" of the "Banner" demo
    And I press <key>
    Then the "Banner" demo shows no banner with the words "A demo update is ready."
    And focus is on the button "Show a newer demo notice"
    And the polite region stays empty
    And the assertive region stays empty
    When I reload the page
    Then the "Banner" demo shows 2 banners, none with the words "A demo update is ready."

    Examples:
      | key   |
      | Enter |
      | Space |

  Scenario: A newer edition of a dismissed banner shows again
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the button "Dismiss" of the "Banner" demo
    And I press Enter
    And I press Enter
    Then the "Banner" demo shows a banner with the words "A demo update is ready."
    And focus stays on the button "Show a newer demo notice"

  Scenario: A banner dismissed in a browser that keeps nothing goes for now and shows again on the next load
    Given this browser keeps nothing a page stores
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the button "Dismiss" of the "Banner" demo
    And I press Enter
    Then the "Banner" demo shows no banner with the words "A demo update is ready."
    And focus is on the button "Show a newer demo notice"
    When I reload the page
    Then the "Banner" demo shows a banner with the words "A demo update is ready."

  Scenario Outline: A banner lies on Ledger Gray, square, its words in the text's color and its icon and start edge in its tone, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then every banner of the "Banner" demo lies square on --surface with its words in --fg, its icon and its 0.25 rem start edge in its tone's color

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a banner is edged all round in the text's color, its start edge thicker, and its icon in the text's color
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every banner of the "Banner" demo is edged 1 pixel wide in CanvasText, its start edge 0.25 rem, and its icon in the text's color

  Scenario Outline: A banner's edge and icon stand at the start of its line and its Dismiss button at the end, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    Then every banner of the banner demo has its edge on its start side, its icon before its words and its Dismiss button after them, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, a banner's words wrap inside it, its icon and Dismiss button inside it too
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then every banner of the "Banner" demo holds its icon, its words and its Dismiss button inside its edge
    And the page doesn't scroll sideways
    And no text is clipped

  Scenario: Keyboard focus on a control shows its tooltip at once, beside the control, which it describes
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." shows beside the button "Prune the demo repository", without covering it
    And the button "Prune the demo repository" is described by "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing."
    And the page has no accessibility violations
    When I press Shift+Tab
    Then no tooltip shows

  Scenario: Escape hides a tooltip and leaves focus on its control, which it still describes
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    And I press Escape
    Then no tooltip shows
    And focus stays on the button "Prune the demo repository"
    And the button "Prune the demo repository" is described by "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing."

  Scenario: A resting pointer shows a tooltip after a moment, which stays while the pointer moves onto it, until Escape
    When I open the component gallery
    And I rest the pointer on the button "Prune the demo repository"
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." shows only once the pointer has rested a moment
    When I move the pointer onto the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing."
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." still shows a moment later
    When I press Escape
    Then no tooltip shows

  Scenario: A tooltip goes once the pointer has left both its control and the tooltip
    When I open the component gallery
    And I rest the pointer on the button "Prune the demo repository"
    Then the tooltip "Frees the space no snapshot uses any longer. A demo: pressing it changes nothing." shows only once the pointer has rested a moment
    When I move the pointer away from the button "Prune the demo repository" and its tooltip
    Then no tooltip shows

  Scenario Outline: A tooltip with room on the side its control asks for stands there, in the direction of the control's line
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "<button>"
    Then the tooltip of the button "<button>" stands <where> it

    Examples:
      | button                                | where           |
      | Demo note above                       | above           |
      | Demo note below                       | below           |
      | Demo note at the start                | to the left of  |
      | Demo note at the end                  | to the right of |
      | Demo note at the start, right to left | to the right of |
      | Demo note at the end, right to left   | to the left of  |

  Scenario: At the foot of the page, a tooltip asked to stand below its control has no room there and stands above it
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo note at the foot of the page"
    Then the tooltip of the button "Demo note at the foot of the page" stands above it

  Scenario: On a small screen with larger text, a tooltip with no room at either end of its control stands above or below it, inside the window
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo note at the end"
    Then the tooltip of the button "Demo note at the end" stands above or below it, inside the window
    When I press Shift+Tab until focus is on the button "Demo note at the start"
    Then the tooltip of the button "Demo note at the start" stands above or below it, inside the window

  Scenario: A long note wraps onto lines of its own, as wide as the window allows, never cut and never over its control
    Given a viewport of 320 by 640 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo long note"
    Then the tooltip of the button "Demo long note" wraps onto lines within the window's width, none of its words cut, without covering the button

  Scenario: A tooltip's words are a step smaller than the page's and grow with the reader's text, wrapping at about 20 of their widths
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo long note"
    Then the tooltip of the button "Demo long note" is set a step smaller than the page's text and wraps at 20 times its text size

  Scenario: An icon-only control's tooltip shows the control's name, which a screen reader hears once, from the control
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo schedule"
    Then the button "Demo schedule" shows only an icon, which a screen reader skips
    And the tooltip of the button "Demo schedule" shows "Demo schedule", which a screen reader skips
    And the button "Demo schedule" has no description

  Scenario Outline: A tooltip is drawn in the page's colors swapped, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" is filled with the page's text color, its words in the page's background color, with no shadow

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a tooltip takes the system's colors and an edge in its text color
    Given the browser asks for "forced colors"
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" is drawn in the page's own colors, edged in its text color

  Scenario: A tooltip fades in quickly, and appears at once for a reader who asks for less motion
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Prune the demo repository"
    Then the tooltip of the button "Prune the demo repository" fades in over 100 ms
    When the reader asks for less motion
    Then the tooltip of the button "Prune the demo repository" appears without fading

  Scenario: A tooltip follows its control as the page scrolls
    When I open the component gallery
    And I press Shift+Tab until focus is on the button "Demo note above"
    And the page scrolls up a few lines
    Then the tooltip of the button "Demo note above" stands beside it, without covering it

  Scenario: A text field's error is shown above it, describes it, marks it invalid with a thicker edge in the Error tone, and is announced
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Enter
    Then the field "Repository name" shows the error "A repository name is required."
    And the field "Repository name" is described by "A repository name is required."
    And the field "Repository name" is marked invalid
    And the field "Repository name" is edged 2 pixels wide in --error
    And the polite region reads "A repository name is required."
    When I press Enter
    Then the field "Repository name" shows no error
    And the field "Repository name" is described only by "The name of the repository to check."
    And the field "Repository name" is not marked invalid
    And the field "Repository name" is edged 1 pixel wide in --field-edge

  Scenario: A text field with no known purpose turns the browser's autocomplete off
    When I open the component gallery
    Then the textbox "Demo host" has the autocomplete token "off"

  Scenario: A field already invalid shows its label, its description and its error above it, each describing it
    When I open the component gallery
    Then the field "Demo host" shows its label, its description and its error above it, in that order
    And the field "Demo host" is described by "The demo machine's host name."
    And the field "Demo host" is described by "Enter a host name, such as"
    And the field "Demo host" is marked invalid
    And the field "Demo host" is edged 2 pixels wide in --error

  Scenario: A multi-line text field takes an entry a line and shows its error as a one-line field does
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the textbox "Demo folders"
    And I type "/srv/demo/photos"
    And I press Enter
    And I type "/srv/demo/notes"
    Then the textbox "Demo folders" holds the lines "/srv/demo/photos" and "/srv/demo/notes"
    And the field "Demo folders" is described only by "One folder or file a line."
    And the field "Demo folders" is edged 1 pixel wide in --field-edge
    When I press Tab until focus is on the button "Check the demo folders"
    And I press Enter
    Then the field "Demo folders" shows the error "Enter at least one folder."
    And the field "Demo folders" is marked invalid
    And the field "Demo folders" is edged 2 pixels wide in --error
    And the polite region reads "Enter at least one folder."

  Scenario: A password's Show password button shows it and hides it again, keeping its value, its caret and focus, and says it is pressed
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Tab until focus is on the textbox "Demo password"
    And I type "demo-secret"
    And I press ArrowLeft
    Then the field "Demo password" hides what it holds
    When I press Tab
    Then focus is on the button "Show password"
    And the button "Show password" is not pressed
    And the button "Show password" is described by "Demo password"
    When I press Space
    Then focus is on the button "Show password"
    And the button "Show password" is pressed
    And the field "Demo password" shows what it holds, "demo-secret", its caret 1 character before its end
    And the textbox "Demo password" has the autocomplete token "current-password"
    And the "Text field" demo has no accessibility violations
    When I press Enter
    Then focus is on the button "Show password"
    And the button "Show password" is not pressed
    And the field "Demo password" hides what it holds
    And the textbox "Demo password" still holds "demo-secret"

  Scenario: A number field asks for the keyboard of digits, is never a spin box, and takes what is typed for its form to check
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Tab until focus is on the textbox "Demo copies to keep"
    Then the textbox "Demo copies to keep" asks for the keyboard of digits, and is a text input
    When I type "1a2"
    Then the textbox "Demo copies to keep" still holds "1a2"

  Scenario: A read-only field stays in the Tab order behind a dashed edge, a disabled one is skipped with its Show password button, and each says so in words
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the button "Check the repository name"
    And I press Tab until focus is on the textbox "Demo snapshot ID"
    Then the field "Demo snapshot ID" is described by "Read only: you can select and copy it, but not change it."
    And the field "Demo snapshot ID" is edged with dashes
    When I type "9"
    Then the textbox "Demo snapshot ID" still holds "1a2b3c4d"
    When I press Tab
    Then focus is on the textbox "Demo tag at the Small size"
    And the field "Demo repository location" is described by "Unavailable: it can't be changed now."
    And the field "Demo repository location" is disabled
    And the field "Demo keyring password" is described by "Unavailable: it can't be changed now."
    And the field "Demo keyring password" is disabled

  Scenario: A required field says so in words after its label, which a screen reader skips, and says it once itself
    When I open the component gallery
    Then the field "Demo plan name" shows "(required)" after its label, which a screen reader skips
    And the field "Demo plan name" is required, its name its label alone

  Scenario Outline: A text field is as tall as a button of its size, and the two stand level in a row of actions
    Given the reader sets the text size to 200 percent
    When I open the component gallery
    Then the field "<field>" is <times> times as tall as the page's text
    And the field "<field>" is as tall as the button "<button>" beside it, their feet level

    Examples:
      | field                            | button                                   | times |
      | Demo tag at the Small size       | Add the demo tag at the Small size       | 1.75  |
      | Demo tag at the Medium size      | Add the demo tag at the Medium size      | 2     |
      | Demo tag at the Large size       | Add the demo tag at the Large size       | 2.5   |
      | Demo tag at the Extra large size | Add the demo tag at the Extra large size | 3     |

  Scenario Outline: A text field's icon stands at its start, inside its edge, in the page's direction, and a screen reader skips it
    When I open the component gallery asking for the locale "<locale>"
    Then the field with an icon shows it at its start, inside its edge, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario Outline: A text field's edge is drawn in its own token, in light colors and in dark
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then the field "Repository name" is edged 1 pixel wide in --field-edge
    And the field "Demo snapshot ID" is edged with dashes
    And the "Text field" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a text field takes the reader's field colors, a disabled one the color of unavailable text, and invalid and read only keep their edges
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every field of the "Text field" demo is drawn in the reader's field colors, a disabled one in the color of unavailable text
    And the field "Demo host" is edged 2 pixels wide
    And the field "Demo snapshot ID" is edged with dashes

  Scenario: Enter on a link to a page of the app goes there without reloading, titles the page and moves focus to its heading
    When I open the component gallery
    And I skip to the content
    And the page carries a mark that a reload would lose
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the link "Demo link to the start page"
    And I press Enter
    Then focus is on the main heading "Welcome to Ladderback"
    And the page is titled "Ladderback"
    And the page still carries its mark
    And the live regions stay silent

  Scenario: A link to the page the reader is on says it is the current page and is bold, and a link elsewhere is neither
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the link "Demo link to this page"
    Then the link "Demo link to this page" is the current page, drawn bold
    And the link "Demo link to the start page" is not the current page, drawn in the weight of the text around it

  Scenario: A link to another site says so in its name after its words, shows an icon after them that a screen reader skips, is marked as leaving the app, and tells that site nothing of the page
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Send the demo form"
    And I press Tab until focus is on the link "Demo rustic documentation (goes to another site)"
    Then the link "Demo rustic documentation (goes to another site)" shows the words "Demo rustic documentation", then an icon a screen reader skips
    And the link "Demo rustic documentation (goes to another site)" is marked external, sends no referrer and opens where the reader is
    And the link "Demo rustic documentation, right to left (goes to another site)" shows the words "Demo rustic documentation, right to left", then an icon a screen reader skips

  Scenario Outline: A link is underlined without a pointer over it, below its letters, in Ladder Blue or the color of the text around it
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then every link of the "Link" demo is underlined below its letters
    And each link of the "Link" demo is drawn in its tone:
      | link                                                            | color              |
      | Demo link to the start page                                     | --accent           |
      | Demo link to this page                                          | --accent           |
      | Demo rustic documentation (goes to another site)                | --accent           |
      | the demo plan format                                            | --accent           |
      | the demo log                                                    | the text around it |
      | Demo rustic documentation, right to left (goes to another site) | --accent           |
    And the "Link" demo has no accessibility violations

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors a link takes the system's link color and keeps its underline
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every link of the "Link" demo is drawn in the system's link color
    And every link of the "Link" demo is underlined below its letters

  Scenario Outline: Under the pointer a link's underline is thicker than at rest
    Given the browser asks for "<colors>"
    When I open the component gallery
    And I rest the pointer on the link "Demo link to the start page"
    Then the underline of the link "Demo link to the start page" is thicker than at rest

    Examples:
      | colors        |
      | light colors  |
      | forced colors |

  Scenario: On a small screen with larger text, a long link wraps onto a second line inside the page
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then the link "Demo rustic documentation (goes to another site)" wraps onto more than one line
    And the page doesn't scroll sideways
    And no text is clipped

  Scenario: An alert is no live region and no stop for Tab, and its icon names its tone in words
    When I open the component gallery
    Then no alert of the "Alert" demo is a live region, nor holds one
    And the alerts of the "Alert" demo show these icons, each named for its tone, one shape per tone, in this order:
      | tone    | name        |
      | primary | Information |
      | success | Success     |
      | warning | Warning     |
      | error   | Error       |
      | primary | Information |
    When I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the link "the demo operations"
    And I press Tab
    Then focus is on the button "Dismiss" of the "Alert" demo

  Scenario Outline: <key> on an alert's Dismiss button takes the alert away, says nothing and leaves focus on the control its page names
    When I open the component gallery
    And I skip to the content
    And I press Tab until focus is on the button "Filled Primary"
    And I press Tab until focus is on the button "Demo work in progress"
    And I press Tab until focus is on the button "Show the demo plan"
    And I press Tab until focus is on the button "Dismiss" of the "Alert" demo
    And I press <key>
    Then the "Alert" demo shows no alert with the words "This demo notice goes when you dismiss it."
    And focus is on the button "Show the demo notice again"
    And the polite region stays empty
    And the assertive region stays empty
    When I press Enter
    Then the "Alert" demo shows an alert with the words "This demo notice goes when you dismiss it."
    And focus stays on the button "Show the demo notice again"

    Examples:
      | key   |
      | Enter |
      | Space |

  Scenario Outline: An alert lies on Ledger Gray, its words in the text's color and its icon and start edge in its tone, in <colors>
    Given the browser asks for "<colors>"
    When I open the component gallery
    Then every alert of the "Alert" demo lies on --surface with its words in --fg, its icon and its 0.25 rem start edge in its tone's color

    Examples:
      | colors       |
      | light colors |
      | dark colors  |

  Scenario: Under forced colors an alert is edged all round in the text's color, its start edge thicker, and its icon in the text's color
    Given the browser asks for "forced colors"
    When I open the component gallery
    Then every alert of the "Alert" demo is edged 1 pixel wide in CanvasText, its start edge 0.25 rem, and its icon in the text's color

  Scenario Outline: An alert's edge and icon stand at the start of its line and its Dismiss button at the end, in a page that runs <direction>
    When I open the component gallery asking for the locale "<locale>"
    Then every alert of the alert demo has its edge on its start side, its icon before its words and its Dismiss button after them, in a page that runs <direction>

    Examples:
      | locale | direction     |
      | en-US  | left to right |
      | ar-XB  | right to left |

  Scenario: On a small screen with larger text, an alert's words wrap inside it, its icon and Dismiss button inside it too
    Given a viewport of 320 by 256 pixels
    And the reader sets the text size to 200 percent
    When I open the component gallery
    Then every alert of the "Alert" demo holds its icon, its words and its Dismiss button inside its edge
    And the page doesn't scroll sideways
    And no text is clipped
