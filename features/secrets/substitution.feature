Feature: Secret references in a repository's settings
  A repository's settings never have to hold a secret. `${VAR}` stands for an
  environment variable and `${file:/absolute/path}` for a file's contents, in
  the repository strings and in the values of the options. They are resolved
  when a job starts, each reference is read once per job, and a secret that
  can't be read fails the job naming where it was looked for, never what was
  found (invariant I4). The password comes from its own source.

  Background:
    Given the repository is "/srv/demo/repo"
    And the repository's password is "demo-password"

  Rule: References are replaced where the settings allow them

    Scenario: A variable in the repository string
      Given the environment variable "DEMO_BUCKET" is "demo-backups"
      And the repository is "s3:s3.example.net/${DEMO_BUCKET}/home"
      When the repository is resolved
      Then the resolved repository is "s3:s3.example.net/demo-backups/home"

    Scenario: Several references and the text around them in one field
      Given the environment variable "DEMO_USER" is "demo"
      And the environment variable "DEMO_PASS" is "pa55word"
      And the repository is "rest:https://${DEMO_USER}:${DEMO_PASS}@backup.example.net/${DEMO_USER}/repo"
      When the repository is resolved
      Then the resolved repository is "rest:https://demo:pa55word@backup.example.net/demo/repo"

    Scenario: A variable in the value of an option
      Given the environment variable "DEMO_REGION" is "eu-demo-1"
      And the option "region" is "${DEMO_REGION}"
      When the repository is resolved
      Then the resolved option "region" is "eu-demo-1"

    Scenario: A variable in the hot repository
      Given the environment variable "DEMO_HOT_BUCKET" is "demo-hot"
      And the hot repository is "opendal:s3:${DEMO_HOT_BUCKET}"
      When the repository is resolved
      Then the resolved hot repository is "opendal:s3:demo-hot"

    Scenario: A variable in the value of a hot option
      Given the environment variable "DEMO_REGION" is "eu-demo-1"
      And the hot option "region" is "${DEMO_REGION}"
      When the repository is resolved
      Then the resolved hot option "region" is "eu-demo-1"

    Scenario: A variable in the value of a cold option
      Given the environment variable "DEMO_REGION" is "eu-demo-2"
      And the cold option "region" is "${DEMO_REGION}"
      When the repository is resolved
      Then the resolved cold option "region" is "eu-demo-2"

    Scenario: The name of an option is not a template
      Given the environment variable "DEMO_NAME" is "region"
      And the option "${DEMO_NAME}" is "eu-demo-1"
      When the repository is resolved
      Then the resolved option "${DEMO_NAME}" is "eu-demo-1"
      And the resolved options are 1
      And the variable "DEMO_NAME" was read 0 times

    Scenario: An inline password is not a template
      Given the repository's password is "${DEMO_PASSWORD}"
      When the repository is resolved
      Then the resolved password is "${DEMO_PASSWORD}"
      And the variable "DEMO_PASSWORD" was read 0 times

    Scenario: A doubled dollar sign writes a reference literally
      Given the environment variable "DEMO_REGION" is "eu-demo-1"
      And the option "note" is "keep $${DEMO_REGION} as it is"
      When the repository is resolved
      Then the resolved option "note" is "keep ${DEMO_REGION} as it is"
      And the variable "DEMO_REGION" was read 0 times

  Rule: A reference gives the value as its source holds it

    Scenario: A variable's value is kept as it is
      Given the environment variable "DEMO_NOTE" is " demo note\n"
      And the option "note" is "${DEMO_NOTE}"
      When the repository is resolved
      Then the resolved option "note" is " demo note\n"

    Scenario: A variable that is set to nothing gives nothing
      Given the environment variable "DEMO_EMPTY" is empty
      And the repository is "s3:s3.example.net/demo${DEMO_EMPTY}"
      When the repository is resolved
      Then the resolved repository is "s3:s3.example.net/demo"

    Scenario: A file gives its contents without the newline that ends it
      Given the file "/home/demo/token" holds "demo-token\n"
      And the option "token" is "${file:/home/demo/token}"
      When the repository is resolved
      Then the resolved option "token" is "demo-token"

    Scenario: A file gives its contents without a carriage return and newline
      Given the file "/home/demo/token" holds "demo-token\r\n"
      And the option "token" is "${file:/home/demo/token}"
      When the repository is resolved
      Then the resolved option "token" is "demo-token"

    Scenario: A file loses only one newline
      Given the file "/home/demo/token" holds "demo-token\n\n"
      And the option "token" is "${file:/home/demo/token}"
      When the repository is resolved
      Then the resolved option "token" is "demo-token\n"

    Scenario: A file keeps its inner lines and a carriage return that ends no line
      Given the file "/home/demo/key" holds "line one\nline two\r"
      And the option "key" is "${file:/home/demo/key}"
      When the repository is resolved
      Then the resolved option "key" is "line one\nline two\r"

    Scenario: A file in a reference is in the hot repository too
      Given the file "/home/demo/bucket" holds "demo-hot\n"
      And the hot repository is "opendal:s3:${file:/home/demo/bucket}"
      When the repository is resolved
      Then the resolved hot repository is "opendal:s3:demo-hot"

  Rule: A reference that can't be read fails the job and names where it was looked for

    Scenario: A variable that isn't set fails and is never read as empty
      Given the environment variable "DEMO_BUCKET" is not set
      And the repository is "s3:s3.example.net/${DEMO_BUCKET}"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the failure names the variable "DEMO_BUCKET"
      And the problem is that it is missing

    Scenario: A variable that isn't set fails for the option that holds it
      Given the option "region" is "${DEMO_REGION}"
      When the repository is resolved
      Then the resolution fails for the field "options.region"
      And the failure names the variable "DEMO_REGION"
      And the problem is that it is missing

    Scenario: A file that isn't there fails and names its path
      Given the cold option "token" is "${file:/home/demo/missing-token}"
      When the repository is resolved
      Then the resolution fails for the field "options-cold.token"
      And the failure names the file "/home/demo/missing-token"
      And the problem is that it is missing

    Scenario: A file that isn't text fails
      Given the file "/home/demo/token" holds bytes that aren't text
      And the option "token" is "${file:/home/demo/token}"
      When the repository is resolved
      Then the resolution fails for the field "options.token"
      And the failure names the file "/home/demo/token"
      And the problem is that it isn't text

    Scenario: A file larger than the limit fails
      Given the file "/home/demo/token" holds 65537 bytes of text
      And the option "token" is "${file:/home/demo/token}"
      When the repository is resolved
      Then the resolution fails for the field "options.token"
      And the failure names the file "/home/demo/token"
      And the problem is that it is larger than 65536 bytes

    Scenario: A variable larger than the limit fails
      Given the environment variable "DEMO_BIG" holds 65537 bytes of text
      And the repository is "${DEMO_BIG}"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the failure names the variable "DEMO_BIG"
      And the problem is that it is larger than 65536 bytes

  Rule: A field that isn't a valid template fails before anything is read

    Scenario Outline: A malformed reference fails for the field that holds it
      Given the environment variable "DEMO_BUCKET" is "demo-backups"
      And the repository is "s3:<field text>"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the field is invalid because of its <kind> reference at byte 3
      And the variable "DEMO_BUCKET" was read 0 times

      Examples:
        | field text                   | kind         |
        | ${DEMO_BUCKET                | unterminated |
        | ${}${DEMO_BUCKET}            | empty        |
        | ${1BUCKET}${DEMO_BUCKET}     | misnamed     |
        | ${DEMO-BUCKET}${DEMO_BUCKET} | misnamed     |
        | ${file:}${DEMO_BUCKET}       | pathless     |
        | ${file:demo/key}             | relative     |
        | ${${DEMO_BUCKET}}            | nested       |

    Scenario: A malformed reference in an option fails for that option
      Given the option "endpoint" is "https://${DEMO_HOST"
      When the repository is resolved
      Then the resolution fails for the field "options.endpoint"
      And the field is invalid because of its unterminated reference at byte 8

    Scenario: A field with more references than the limit fails
      Given the environment variable "DEMO_X" is "a"
      And the repository is 17 references to the variable "DEMO_X"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the field is invalid because it has more than 16 references

    Scenario: A field with as many references as the limit is resolved
      Given the environment variable "DEMO_X" is "a"
      And the repository is 16 references to the variable "DEMO_X"
      When the repository is resolved
      Then the resolved repository is "aaaaaaaaaaaaaaaa"

    Scenario: A field longer than the limit fails
      Given the repository is 65537 bytes of text
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the field is invalid because it is longer than 65536 bytes

    Scenario: A field as long as the limit is resolved
      Given the repository is 65536 bytes of text
      When the repository is resolved
      Then the resolved repository has 65536 bytes

  Rule: A field never expands to more than 64 KiB

    Scenario: A reference that fills the limit is resolved
      Given the environment variable "DEMO_BIG" holds 65536 bytes of text
      And the repository is "${DEMO_BIG}"
      When the repository is resolved
      Then the resolved repository has 65536 bytes

    Scenario: A reference and the text after it that pass the limit fail
      Given the environment variable "DEMO_BIG" holds 65536 bytes of text
      And the repository is "${DEMO_BIG}x"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the references expand to more than 65536 bytes

    Scenario: Text and a reference that pass the limit fail
      Given the environment variable "DEMO_BIG" holds 30000 bytes of text
      And the repository is 40000 bytes of text followed by "${DEMO_BIG}"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the references expand to more than 65536 bytes

    Scenario: Several references that pass the limit together fail
      Given the environment variable "DEMO_BIG" holds 40000 bytes of text
      And the option "note" is "${DEMO_BIG}${DEMO_BIG}"
      When the repository is resolved
      Then the resolution fails for the field "options.note"
      And the references expand to more than 65536 bytes

    Scenario: Several references that fill the limit together are resolved
      Given the environment variable "DEMO_HALF" holds 32768 bytes of text
      And the option "note" is "${DEMO_HALF}${DEMO_HALF}"
      When the repository is resolved
      Then the resolved option "note" has 65536 bytes

  Rule: The password comes from its own source

    Scenario: A password written in the settings
      Given the repository's password is "inline-password"
      When the repository is resolved
      Then the resolved password is "inline-password"

    Scenario: A password in an environment variable is kept as it is
      Given the environment variable "DEMO_PASSWORD" is "env-password\n"
      And the repository's password is in the environment variable "DEMO_PASSWORD"
      When the repository is resolved
      Then the resolved password is "env-password\n"

    Scenario: A password in a file is the first line of it
      Given the file "/home/demo/password" holds "file-password\r\nnot the password\n"
      And the repository's password is in the file "/home/demo/password"
      When the repository is resolved
      Then the resolved password is "file-password"

    Scenario: A password from a command is the first line of what it prints
      Given the command "pass show demo/repo" prints "command-password\nnot the password\n"
      And the repository's password is printed by the command "pass show demo/repo"
      When the repository is resolved
      Then the resolved password is "command-password"

    Scenario: A password in the keyring
      Given the keyring holds "keyring-password" under "ladderback/demo"
      And the repository's password is in the keyring under "ladderback/demo"
      When the repository is resolved
      Then the resolved password is "keyring-password"

  Rule: A password that can't be read fails the job and names its source

    Scenario: A password variable that isn't set
      Given the repository's password is in the environment variable "DEMO_PASSWORD"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the variable "DEMO_PASSWORD"
      And the problem is that it is missing

    Scenario: A password file that isn't there
      Given the repository's password is in the file "/home/demo/password"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the file "/home/demo/password"
      And the problem is that it is missing

    Scenario: A password file that is a directory
      Given the file "/home/demo/password" is a directory
      And the repository's password is in the file "/home/demo/password"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the file "/home/demo/password"
      And the problem is that it can't be reached

    Scenario: A password file that isn't text
      Given the file "/home/demo/password" holds bytes that aren't text
      And the repository's password is in the file "/home/demo/password"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the file "/home/demo/password"
      And the problem is that it isn't text

    Scenario: A password file larger than the limit
      Given the file "/home/demo/password" holds 65537 bytes of text
      And the repository's password is in the file "/home/demo/password"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the file "/home/demo/password"
      And the problem is that it is larger than 65536 bytes

    Scenario: A keyring entry that holds nothing
      Given the repository's password is in the keyring under "ladderback/demo"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the keyring entry "ladderback/demo"
      And the problem is that it is missing

    Scenario: A command that fails is told by its status and what it said
      Given the command "vault-reader --token demo-argument" prints "demo-stdout" and fails with status 2 saying "no such entry\nsecond line"
      And the repository's password is printed by the command "vault-reader --token demo-argument"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the program "vault-reader"
      And the problem is that the command exited with status 2 saying "no such entry"
      And the failure doesn't mention "demo-stdout"
      And the failure doesn't mention "demo-argument"

    Scenario: A command that stops partway through quoting what it printed shows none of it
      Given the command "vault-reader" prints "CANARY-cut-password" and fails with status 1 saying "rejected CANARY-cut"
      And the repository's password is printed by the command "vault-reader"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the problem is that the command exited with status 1 saying "rejected ***"

    Scenario: A command that never ends
      Given the command "pass show demo/repo" never ends
      And the repository's password is printed by the command "pass show demo/repo"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the program "pass"
      And the problem is that the command took too long

    Scenario: A command that can't be started
      Given the repository's password is printed by the command "pass show demo/repo"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the program "pass"
      And the problem is that it can't be reached

    Scenario: A command that prints bytes that aren't text
      Given the command "pass show demo/repo" prints bytes that aren't text
      And the repository's password is printed by the command "pass show demo/repo"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the program "pass"
      And the problem is that it isn't text

    Scenario Outline: A command that prints nothing gives no password
      Given the command "pass show demo/repo" prints "<output>"
      And the repository's password is printed by the command "pass show demo/repo"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the program "pass"
      And the problem is that it is missing

      Examples:
        | output |
        |        |
        | \\n    |
        | \\r\\n |
        | \\n\\n |

    Scenario: A command that prints more than the limit
      Given the command "pass show demo/repo" prints 65537 bytes of text
      And the repository's password is printed by the command "pass show demo/repo"
      When the repository is resolved
      Then the resolution fails for the field "password"
      And the failure names the program "pass"
      And the problem is that it is larger than 65536 bytes

  Rule: A reference never reaches the arguments of a process
    rustic starts rclone with the repository string and the `rclone-command`
    option as arguments, and runs the post-create and post-delete commands of
    a local repository. A resolved secret there would show in `ps` (I4), so
    such a field with a reference is refused before anything is read.

    Scenario: A reference in an rclone repository is refused
      Given the environment variable "DEMO_PASS" is "demo-obscured"
      And the repository is "rclone::sftp,host=nas.example.net,pass=${DEMO_PASS}:backups"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the reference is refused because it would reach the arguments of a process
      And nothing was read

    Scenario: A reference in an rclone hot repository is refused
      Given the environment variable "DEMO_PASS" is "demo-obscured"
      And the hot repository is "rclone::sftp,pass=${DEMO_PASS}:backups"
      When the repository is resolved
      Then the resolution fails for the field "repo-hot"
      And the reference is refused because it would reach the arguments of a process
      And nothing was read

    Scenario: A reference that supplies the rclone prefix is refused
      Given the environment variable "DEMO_REPO" is "rclone::sftp,host=nas.example.net,pass=demo-obscured:backups"
      And the repository is "${DEMO_REPO}"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the reference is refused because it would reach the arguments of a process

    Scenario: A reference that supplies the rclone prefix of the hot repository is refused
      Given the environment variable "DEMO_SCHEME" is "rclone:"
      And the hot repository is "${DEMO_SCHEME}nas:backups"
      When the repository is resolved
      Then the resolution fails for the field "repo-hot"
      And the reference is refused because it would reach the arguments of a process

    Scenario Outline: A reference in an option that is run as a command is refused
      Given the environment variable "DEMO_TOKEN" is "demo-token"
      And the option "<name>" is "backup-notify --token ${DEMO_TOKEN}"
      When the repository is resolved
      Then the resolution fails for the field "options.<name>"
      And the reference is refused because it would reach the arguments of a process
      And nothing was read

      Examples:
        | name                |
        | rclone-command      |
        | post-create-command |
        | post-delete-command |

    Scenario Outline: A reference in a hot or cold option that is run as a command is refused
      Given the environment variable "DEMO_TOKEN" is "demo-token"
      And the <kind> "post-create-command" is "backup-notify --token ${DEMO_TOKEN}"
      When the repository is resolved
      Then the resolution fails for the field "<field>"
      And the reference is refused because it would reach the arguments of a process
      And nothing was read

      Examples:
        | kind        | field                         |
        | hot option  | options-hot.post-create-command  |
        | cold option | options-cold.post-create-command |

    Scenario: A refusal comes before a password that can't be read
      Given the repository's password is in the environment variable "DEMO_PASSWORD"
      And the repository is "rclone::sftp,pass=${DEMO_PASS}:backups"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the reference is refused because it would reach the arguments of a process
      And nothing was read

    Scenario: The first refused field is the one reported
      Given the option "rclone-command" is "rclone ${DEMO_FLAG}"
      And the cold option "post-create-command" is "notify ${DEMO_FLAG}"
      And the hot repository is "rclone:${DEMO_REMOTE}"
      When the repository is resolved
      Then the resolution fails for the field "repo-hot"

    Scenario: The first refused option is the one reported, by name
      Given the option "rclone-command" is "rclone ${DEMO_FLAG}"
      And the option "post-create-command" is "notify ${DEMO_FLAG}"
      When the repository is resolved
      Then the resolution fails for the field "options.post-create-command"

    Scenario: An rclone repository without a reference is resolved
      Given the repository is "rclone:nas:backups/home"
      And the option "rclone-command" is "rclone serve restic --stdio"
      When the repository is resolved
      Then the resolved repository is "rclone:nas:backups/home"
      And the resolved option "rclone-command" is "rclone serve restic --stdio"

    Scenario: A doubled dollar sign in an rclone repository is no reference
      Given the repository is "rclone:nas:$${DEMO_DIR}"
      When the repository is resolved
      Then the resolved repository is "rclone:nas:${DEMO_DIR}"

    Scenario: A reference in an option that is not run as a command is resolved
      Given the environment variable "DEMO_REGION" is "eu-demo-1"
      And the repository is "rclone:nas:backups"
      And the option "region" is "${DEMO_REGION}"
      When the repository is resolved
      Then the resolved option "region" is "eu-demo-1"

    Scenario: A reference in a repository that is not rclone's is resolved
      Given the environment variable "DEMO_BUCKET" is "demo-backups"
      And the repository is "opendal:s3:${DEMO_BUCKET}"
      When the repository is resolved
      Then the resolved repository is "opendal:s3:demo-backups"

    Scenario: A repository whose scheme only starts like rclone's is resolved
      Given the environment variable "DEMO_BUCKET" is "demo-backups"
      And the repository is "rclones:${DEMO_BUCKET}"
      When the repository is resolved
      Then the resolved repository is "rclones:demo-backups"

  Rule: An rclone repository holds no more than 16 credentials
    Each credential written in an rclone connection string is a secret to
    hide in every text a job shows, where rclone's errors may quote one on
    its own (I4). A location with more is refused before anything is read.

    Scenario: An rclone repository with more credentials than the limit is refused
      Given the repository's password is in the environment variable "DEMO_PASSWORD"
      And the repository is an rclone connection string with 17 credentials
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the field is refused because it holds more than 16 credentials
      And nothing was read

    Scenario: An rclone hot repository with more credentials than the limit is refused
      Given the repository is "/srv/demo-cold"
      And the hot repository is an rclone connection string with 17 credentials
      When the repository is resolved
      Then the resolution fails for the field "repo-hot"
      And the field is refused because it holds more than 16 credentials
      And nothing was read

  Rule: The first field that fails is the one reported

    Scenario Outline: Fields are tried in a fixed order
      Given the repository's password is in the environment variable "DEMO_PASSWORD"
      And the repository is "s3:s3.example.net/${DEMO_REPOSITORY}"
      And the hot repository is "opendal:s3:${DEMO_HOT_REPOSITORY}"
      And the option "region" is "${DEMO_REGION}"
      And the hot option "region" is "${DEMO_HOT_REGION}"
      And the cold option "region" is "${DEMO_COLD_REGION}"
      And the environment variables "<already set>" are set
      When the repository is resolved
      Then the resolution fails for the field "<field>"
      And the failure names the variable "<variable>"

      Examples:
        | already set                                                                          | field                | variable             |
        |                                                                                      | password             | DEMO_PASSWORD        |
        | DEMO_PASSWORD                                                                        | repository           | DEMO_REPOSITORY      |
        | DEMO_PASSWORD, DEMO_REPOSITORY                                                       | repo-hot             | DEMO_HOT_REPOSITORY  |
        | DEMO_PASSWORD, DEMO_REPOSITORY, DEMO_HOT_REPOSITORY                                  | options.region       | DEMO_REGION          |
        | DEMO_PASSWORD, DEMO_REPOSITORY, DEMO_HOT_REPOSITORY, DEMO_REGION                     | options-hot.region   | DEMO_HOT_REGION      |
        | DEMO_PASSWORD, DEMO_REPOSITORY, DEMO_HOT_REPOSITORY, DEMO_REGION, DEMO_HOT_REGION    | options-cold.region  | DEMO_COLD_REGION     |

    Scenario: Options are tried by name
      Given the option "region" is "${DEMO_REGION}"
      And the option "endpoint" is "${DEMO_ENDPOINT}"
      When the repository is resolved
      Then the resolution fails for the field "options.endpoint"
      And the failure names the variable "DEMO_ENDPOINT"

    Scenario: A malformed field fails before a later field's secret is read
      Given the repository is "s3:${DEMO_BUCKET"
      And the option "region" is "${DEMO_REGION}"
      When the repository is resolved
      Then the resolution fails for the field "repository"
      And the variable "DEMO_REGION" was read 0 times
      And only the password was read

  Rule: Each reference is read once per job

    Scenario: A variable used in several fields is read once
      Given the environment variable "DEMO_REGION" is "eu-demo-1"
      And the repository is "s3:s3.example.net/${DEMO_REGION}/${DEMO_REGION}"
      And the hot repository is "opendal:s3:${DEMO_REGION}"
      And the option "region" is "${DEMO_REGION}"
      And the hot option "region" is "${DEMO_REGION}"
      And the cold option "region" is "${DEMO_REGION}"
      When the repository is resolved
      Then the resolved repository is "s3:s3.example.net/eu-demo-1/eu-demo-1"
      And the resolved cold option "region" is "eu-demo-1"
      And the variable "DEMO_REGION" was read 1 time

    Scenario: A file used in several fields is read once
      Given the file "/home/demo/token" holds "demo-token\n"
      And the option "token" is "${file:/home/demo/token}"
      And the hot option "token" is "${file:/home/demo/token}"
      When the repository is resolved
      Then the resolved hot option "token" is "demo-token"
      And the file "/home/demo/token" was read 1 time

    Scenario: Two variables are each read once
      Given the environment variable "DEMO_USER" is "demo"
      And the environment variable "DEMO_HOST" is "backup.example.net"
      And the repository is "rest:https://${DEMO_USER}@${DEMO_HOST}/${DEMO_USER}"
      When the repository is resolved
      Then the variable "DEMO_USER" was read 1 time
      And the variable "DEMO_HOST" was read 1 time

  Rule: A file that others can read comes with a warning

    Scenario: A group-readable file in a reference
      Given the file "/home/demo/token" with mode 640 holds "demo-token"
      And the option "token" is "${file:/home/demo/token}"
      When the repository is resolved
      Then the resolved option "token" is "demo-token"
      And the resolution warns once that others can read the file "/home/demo/token"

    Scenario: A world-readable password file
      Given the file "/home/demo/password" with mode 604 holds "demo-password"
      And the repository's password is in the file "/home/demo/password"
      When the repository is resolved
      Then the resolution warns once that others can read the file "/home/demo/password"

    Scenario: A file read twice warns once
      Given the file "/home/demo/password" with mode 644 holds "demo-password"
      And the repository's password is in the file "/home/demo/password"
      And the option "token" is "${file:/home/demo/password}"
      And the cold option "token" is "${file:/home/demo/password}"
      When the repository is resolved
      Then the resolution warns once that others can read the file "/home/demo/password"

    Scenario: A file only its owner can read gives no warning
      Given the file "/home/demo/token" with mode 600 holds "demo-token"
      And the option "token" is "${file:/home/demo/token}"
      When the repository is resolved
      Then the resolution warns about nothing

    Scenario Outline: A file where the runtime sets permissions gives no warning
      Given the environment variable "CREDENTIALS_DIRECTORY" is "/run/credentials/demo.service"
      And the file "<path>" with mode 644 holds "demo-token"
      And the option "token" is "${file:<path>}"
      When the repository is resolved
      Then the resolution warns about nothing

      Examples:
        | path                                |
        | /run/secrets/demo-token             |
        | /var/run/secrets/demo-token         |
        | /run/credentials/demo.service/token |

    Scenario Outline: A file that only looks like it is where the runtime sets permissions gives a warning
      Given the environment variable "CREDENTIALS_DIRECTORY" is "<credentials>"
      And the file "<path>" with mode 644 holds "demo-token"
      And the option "token" is "${file:<path>}"
      When the repository is resolved
      Then the resolution warns once that others can read the file "<path>"

      Examples:
        | credentials                   | path                             |
        | /run/credentials/demo.service | /run/credentials/other/token     |
        | /run/credentials/demo.service | /run/secrets/../../home/demo/key |
        | /run/credentials/demo.service | /run/secretsdemo/token           |
        | /                             | /home/demo/token                 |

    Scenario Outline: A link out of where the runtime sets permissions gives a warning
      Given the environment variable "CREDENTIALS_DIRECTORY" is "/run/credentials/demo.service"
      And the file "/home/demo/token" with mode 644 holds "demo-token"
      And the file "<path>" is a link to "/home/demo/token"
      And the option "token" is "${file:<path>}"
      When the repository is resolved
      Then the resolved option "token" is "demo-token"
      And the resolution warns once that others can read the file "<path>"

      Examples:
        | path                                |
        | /run/secrets/demo-token             |
        | /var/run/secrets/demo-token         |
        | /run/credentials/demo.service/token |

    Scenario: A link that stays where the runtime sets permissions gives no warning
      Given the environment variable "CREDENTIALS_DIRECTORY" is "/run/credentials/demo.service"
      And the file "/run/credentials/demo.service/data/token" with mode 644 holds "demo-token"
      And the file "/run/credentials/demo.service/token" is a link to "/run/credentials/demo.service/data/token"
      And the option "token" is "${file:/run/credentials/demo.service/token}"
      When the repository is resolved
      Then the resolved option "token" is "demo-token"
      And the resolution warns about nothing
