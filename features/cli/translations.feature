@e2e
Feature: Keep a translation current
  `just translations` (scripts/translations.ts) lists, for each language
  besides American English, the messages its catalog lacks, the ones
  translated but not yet accepted after review, and the stale ones: those
  whose English has changed since they were translated, each with the English
  it was translated from and the English now. `just accept-translations`
  records, for each message accepted after review, the English it was
  translated from, so that a change to that English makes the translation
  stale (decision 101). A release offers a language only when every message
  of it is current.

  In the examples the catalogs are a scratch folder kept in a git repository
  of its own, the language is German, and each catalog is one file.

  Background:
    Given the English catalog holds:
      """
      greeting = Hello
      farewell = Goodbye
          .title = Leave
      """
    And the German catalog holds:
      """
      greeting = Hallo
      farewell = Auf Wiedersehen
          .title = Verlassen
      """
    And the owner has accepted the German translations of "greeting" and "farewell"
    And the catalogs are committed

  Scenario: A translation accepted after review is current
    When the owner lists the translations
    Then the command exits with status 0
    And the translations listed are:
      """
      de: 2 of 2 messages current
      """

  Scenario: A changed English message makes its translation stale, with the English it was translated from
    Given the English catalog holds:
      """
      greeting = Hello there
      farewell = Goodbye
          .title = Leave
      """
    When the owner lists the translations
    Then the command exits with status 0
    And the translations listed are:
      """
      de: 1 of 2 messages current
        stale: greeting
          translated from:
            greeting = Hello
          now:
            greeting = Hello there
      """

  Scenario: A changed attribute makes its message stale
    Given the English catalog holds:
      """
      greeting = Hello
      farewell = Goodbye
          .title = Quit
      """
    When the owner lists the translations
    Then the command exits with status 0
    And the translations listed are:
      """
      de: 1 of 2 messages current
        stale: farewell
          translated from:
            farewell = Goodbye
                .title = Leave
          now:
            farewell = Goodbye
                .title = Quit
      """

  Scenario: A message the language lacks is missing
    Given the English catalog holds:
      """
      greeting = Hello
      farewell = Goodbye
          .title = Leave
      welcome = Welcome
      """
    When the owner lists the translations
    Then the command exits with status 0
    And the translations listed are:
      """
      de: 2 of 3 messages current
        missing: welcome
      """

  Scenario: A translation nobody has accepted yet isn't current
    Given the English catalog holds:
      """
      greeting = Hello
      farewell = Goodbye
          .title = Leave
      welcome = Welcome
      """
    And the German catalog holds:
      """
      greeting = Hallo
      farewell = Auf Wiedersehen
          .title = Verlassen
      welcome = Willkommen
      """
    When the owner lists the translations
    Then the command exits with status 0
    And the translations listed are:
      """
      de: 2 of 3 messages current
        not reviewed: welcome
      """

  Scenario: Accepting a stale translation after review makes it current again
    Given the English catalog holds:
      """
      greeting = Hello there
      farewell = Goodbye
          .title = Leave
      """
    And the German catalog holds:
      """
      greeting = Hallo zusammen
      farewell = Auf Wiedersehen
          .title = Verlassen
      """
    When the owner accepts the German translation of "greeting"
    And the owner lists the translations
    Then the command exits with status 0
    And the translations listed are:
      """
      de: 2 of 2 messages current
      """

  Scenario: A message English doesn't have can't be accepted
    When the owner accepts the German translation of "welcome"
    Then the command exits with status 1
    And standard error says "de: en-US has no message welcome"
