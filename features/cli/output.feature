@e2e
Feature: The command line's output
  What `ladderback` prints is read on a terminal, through a screen reader, on
  a terminal that cannot draw, and by scripts through a pipe (§4.5). So
  colour is never the only signal; `--plain` asks for text with no colour and
  nothing to redraw, a fact to a line; and a command that reports state has
  `--json`, which prints one JSON document on one line of standard output for
  a script to parse. The exit status is the same with and without it: 0 when
  the command did what it was asked, 1 when it refused, 2 when the command
  line was not understood, which the parser says before any command runs and
  leaves standard output empty.

  `import-profile` reports what it added, so it has `--json`. `export-profile`
  writes the profile itself on standard output, and `serve` is the daemon,
  whose standard error is its log: neither has a state to report, and neither
  takes `--json`.

  In the examples, a rustic profile is the file `nas.toml` in the folder the
  command runs in, and "colour is forced" means `CLICOLOR_FORCE` is set, which
  makes the programs that honour it colour even a pipe. The commands here
  always run with output that is not a terminal, so what is coloured is
  coloured because it was forced.

  Rule: --json prints one JSON document on one line of standard output

    Scenario: An import says what it added
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "docs"
        sources = ["/home/demo/docs"]

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner runs "import-profile nas.toml --json"
      Then the command exits with status 0
      And standard output is one line holding one JSON document
      And the document has:
        | key        | value           |
        | repository | "nas"           |
        | plans      | ["docs","home"] |
        | notices    | []              |
      And the document has no key "error"
      And standard error is empty

    Scenario: What an import left out is in the document as a code with its arguments
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [copy]
        targets = ["/srv/copy"]

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner runs "import-profile nas.toml --json"
      Then the command exits with status 0
      And standard output is one line holding one JSON document
      And the document has:
        | key                | value                          |
        | repository         | "nas"                          |
        | plans              | ["home"]                       |
        | notices.0.code     | "profile-key-ignored"          |
        | notices.0.args.key | {"type":"text","value":"copy"} |
      And standard error is empty

    Scenario: A secret written in the profile is named by the document and never shown
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password = "CANARY-inline-password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner runs "import-profile nas.toml --json"
      Then the command exits with status 0
      And the document has:
        | key                  | value                                         |
        | notices.0.code       | "inline-secret"                               |
        | notices.0.args.field | {"type":"text","value":"repository.password"} |
      And the import never shows "CANARY-inline-password"

    Scenario: A refused import is a document with the refusal's code, and the status is 1
      Given a config file holding:
        """
        schema = 1

        [repositories.cloud]
        repository = "/srv/demo-cloud"
        password-env = "DEMO_PASSWORD"

        [[plans]]
        id = "home"
        repository = "cloud"
        sources = ["/home/demo"]
        """
      And a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner runs "import-profile nas.toml --json"
      Then the command exits with status 1
      And standard output is one line holding one JSON document
      And the document has:
        | key             | value                          |
        | error.code      | "plan-id-taken"                |
        | error.args.plan | {"type":"text","value":"home"} |
      And the document has no key "repository"
      And standard error is empty
      And the config file is exactly as it was written

    Scenario: A refusal that points into the profile says where, and in the reader's own words
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [forget]
        keep-last = 3
        keep-lots = 7
        """
      When the owner runs "import-profile nas.toml --json"
      Then the command exits with status 1
      And the document has:
        | key             | value                                      |
        | error.code      | "profile-invalid"                          |
        | error.args.key  | {"type":"text","value":"forget.keep-lots"} |
        | error.args.line | {"type":"text","value":"7"}                |
      And the document has some text at "error.detail"
      And standard error is empty

    Scenario Outline: A repository name that cannot be a name is refused as a document too
      When the owner runs "<command line>"
      Then the command exits with status 1
      And standard output is one line holding one JSON document
      And the document has:
        | key        | value                      |
        | error.code | "repository-alias-invalid" |
      And standard error is empty

      Examples:
        | command line                                          |
        | import-profile nas.toml --repo-name Not_A_Name --json |
        | import-profile ./---.toml --json                      |

    Scenario: A profile that is not there is refused as a document, naming the file
      When the owner runs "import-profile missing.toml --json"
      Then the command exits with status 1
      And the document has:
        | key             | value                                  |
        | error.code      | "profile-unreadable"                   |
        | error.args.file | {"type":"text","value":"missing.toml"} |
      And standard error is empty

    Scenario: Plain output leaves a document as it is
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner runs "import-profile nas.toml --json --plain"
      Then the command exits with status 0
      And standard output is one line holding one JSON document
      And the document has:
        | key        | value    |
        | repository | "nas"    |
        | plans      | ["home"] |

    Scenario Outline: A document is the same in every language
      Given the environment sets "LANGUAGE" to "<language>"
      And a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [copy]
        targets = ["/srv/copy"]

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner runs "import-profile nas.toml --json"
      Then the command exits with status 0
      And the document has:
        | key                | value                          |
        | repository         | "nas"                          |
        | plans              | ["home"]                       |
        | notices.0.code     | "profile-key-ignored"          |
        | notices.0.args.key | {"type":"text","value":"copy"} |

      Examples:
        | language |
        | en-US    |
        | en-XA    |
        | ar-XB    |

  Rule: --plain is text with no colour and nothing to redraw, a fact to a line

    Scenario: An import says its facts one to a line, with no colour, though colour is forced
      Given colour is forced on
      And a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [copy]
        targets = ["/srv/copy"]

        [[backup.snapshots]]
        name = "docs"
        sources = ["/home/demo/docs"]

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner runs "import-profile nas.toml --plain"
      Then the command exits with status 0
      And standard output has 3 lines
      And standard error has 1 line
      And the output is plain text

    Scenario Outline: --plain goes before or after the command's name
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner runs "<command line>"
      Then the command exits with status 0

      Examples:
        | command line                    |
        | --plain import-profile nas.toml |
        | import-profile nas.toml --plain |

    Scenario: The daemon's log is plain on request, though colour is forced
      Given colour is forced on
      When the daemon is started with "--listen 127.0.0.1:0 --plain"
      Then the output is plain text

    Scenario Outline: The help and the usage errors are plain on request, though colour is forced
      Given colour is forced on
      When the owner runs "<command line>"
      Then the command exits with status <status>
      And the output is plain text

      Examples:
        | command line           | status |
        | --plain --help         | 0      |
        | import-profile --plain | 2      |
        | --plain --bogus        | 2      |

  Rule: NO_COLOR, and output that is not a terminal, leave a command's output with no colour

    The first two scenarios are controls: forced colour does colour the help,
    the usage errors and the daemon's log, so the ones after them can fail.

    Scenario Outline: Forced colour colours the help and the usage errors
      Given colour is forced on
      When the owner runs "<command line>"
      Then the command exits with status <status>
      And the output has colour

      Examples:
        | command line | status |
        | --help       | 0      |
        | --bogus      | 2      |

    Scenario: Forced colour colours the daemon's log
      Given colour is forced on
      When the daemon is started with "--listen 127.0.0.1:0"
      Then the output has colour

    Scenario Outline: NO_COLOR beats forced colour in the help and the usage errors
      Given colour is forced on
      And NO_COLOR is set
      When the owner runs "<command line>"
      Then the command exits with status <status>
      And the output is plain text

      Examples:
        | command line   | status |
        | --help         | 0      |
        | import-profile | 2      |
        | --bogus        | 2      |

    Scenario: NO_COLOR beats forced colour in the daemon's log
      Given colour is forced on
      And NO_COLOR is set
      When the daemon is started with "--listen 127.0.0.1:0"
      Then the output is plain text

    Scenario: An import has no colour under NO_COLOR, though colour is forced
      Given colour is forced on
      And NO_COLOR is set
      And a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [copy]
        targets = ["/srv/copy"]

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner runs "import-profile nas.toml"
      Then the command exits with status 0
      And the output is plain text

    Scenario: A terminal type alone colours nothing that goes to a pipe
      Given the environment sets "TERM" to "xterm-256color"
      When the owner runs "--help"
      Then the command exits with status 0
      And the output is plain text

    Scenario: A terminal type alone colours nothing the daemon logs to a file
      Given the environment sets "TERM" to "xterm-256color"
      When the daemon is started with "--listen 127.0.0.1:0"
      Then the output is plain text

  Rule: Only a command that reports state takes --json

    Scenario Outline: A command with no state to report refuses --json before it runs
      When the owner runs "<command line>"
      Then the command exits with status 2
      And standard output is empty

      Examples:
        | command line                      |
        | export-profile --plan home --json |
        | serve --json                      |

  Rule: A command that cannot start says why in words, on standard error, and the status is 1

    Scenario: An import with no place for the configuration
      Given there is no home folder and no configuration folder is named
      When the owner runs "import-profile nas.toml"
      Then the command exits with status 1
      And standard output is empty
      And standard error says "no configuration folder"

    Scenario Outline: An import under a name that cannot be a name
      When the owner runs "<command line>"
      Then the command exits with status 1
      And standard output is empty
      And standard error says "<words>"

      Examples:
        | command line                                   | words                                          |
        | import-profile nas.toml --repo-name Not_A_Name | Not_A_Name is not a repository name            |
        | import-profile ./---.toml                      | cannot make a repository name of the file name |
