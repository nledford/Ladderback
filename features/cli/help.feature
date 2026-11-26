@e2e
Feature: The command line's help
  `ladderback --help`, and `--help` after a command, says what the program,
  the command, each command it lists and each of its options does, in the
  reader's language (§8.4). The names a person types (commands, options,
  values, addresses) stay as they are typed, and the parser's own words
  around the descriptions stay English: its headings, the usage line, and
  its `--help` and `--version` options and `help` command.

  A pseudolocale shows which text is the catalog's: under `en-XA` every
  description is transformed, and one left in plain English isn't from the
  catalog.

  Rule: Every description the help gives is in the reader's language

    Scenario Outline: The help of <command line> describes everything in the reader's language
      Given the environment sets "LANGUAGE" to "en-XA"
      When the owner runs "<command line>"
      Then the command exits with status 0
      And the help describes the command and everything it lists in the reader's language

      Examples:
        | command line           |
        | --help                 |
        | serve --help           |
        | import-profile --help  |
        | export-profile --help  |
        | set-password --help    |
        | pairing-token --help   |
        | pair --help            |
        | hub-status --help      |
        | unpair --help          |
        | install-service --help |
        | remove-service --help  |
        | check-keyring --help   |
        | mcp --help             |
        | mcp call --help        |
