Feature: Secrets stay out of what is reported
  Whatever a repository's settings resolve to can reach a log, an error or a
  screen through the repository string, the options or the engine's own
  messages (invariant I4). A resolved repository lists every value its
  references gave, so the engine hides each one wherever it reports text, and
  the debug forms of the settings and of the resolved repository hide the
  values themselves and keep the names that help to diagnose a failure.

  Background:
    Given the repository is "/srv/demo/repo"
    And the repository's password is "demo-password"

  Rule: What the references gave is listed for the engine to hide

    Scenario: A variable's value is listed
      Given the environment variable "DEMO_BUCKET" is "demo-bucket-77"
      And the repository is "s3:s3.example.net/${DEMO_BUCKET}"
      When the repository is resolved
      Then the access lists the secret "demo-bucket-77"
      And the access lists 1 secret

    Scenario: A file's contents are listed without the newline that ends them
      Given the file "/home/demo/token" holds "demo-token-42\n"
      And the option "token" is "${file:/home/demo/token}"
      When the repository is resolved
      Then the access lists the secret "demo-token-42"
      And the access lists 1 secret

    Scenario: A value in an option whose name doesn't say it is a secret is listed
      Given the environment variable "DEMO_ENDPOINT" is "https://s3.example.net/demo"
      And the option "endpoint" is "${DEMO_ENDPOINT}"
      When the repository is resolved
      Then the access lists the secret "https://s3.example.net/demo"

    Scenario: A value used in several places is listed once
      Given the environment variable "DEMO_REGION" is "eu-demo-1"
      And the repository is "s3:s3.example.net/${DEMO_REGION}"
      And the option "region" is "${DEMO_REGION}"
      When the repository is resolved
      Then the access lists 1 secret

    Scenario: Values in the hot and cold fields are listed
      Given the environment variable "DEMO_HOT_BUCKET" is "demo-hot-bucket"
      And the environment variable "DEMO_HOT_REGION" is "eu-hot-1"
      And the environment variable "DEMO_COLD_REGION" is "eu-cold-1"
      And the hot repository is "opendal:s3:${DEMO_HOT_BUCKET}"
      And the hot option "region" is "${DEMO_HOT_REGION}"
      And the cold option "region" is "${DEMO_COLD_REGION}"
      When the repository is resolved
      Then the access lists the secret "demo-hot-bucket"
      And the access lists the secret "eu-hot-1"
      And the access lists the secret "eu-cold-1"
      And the access lists 3 secrets

    Scenario: A repository with no references lists nothing
      Given the option "region" is "eu-demo-1"
      When the repository is resolved
      Then the access lists 0 secrets

    Scenario: The password is not listed among what the references gave
      Given the environment variable "DEMO_PASSWORD" is "env-password"
      And the repository's password is in the environment variable "DEMO_PASSWORD"
      When the repository is resolved
      Then the resolved password is "env-password"
      And the access doesn't list the secret "env-password"

  Rule: The debug form of the access hides every secret and keeps the names

    Scenario: A variable's value in the repository string is hidden
      Given the environment variable "DEMO_BUCKET" is "demo-bucket-77"
      And the repository is "s3:backup.example.net/${DEMO_BUCKET}/home"
      When the repository is resolved
      Then the debug form of the access shows no "demo-bucket-77"
      And the debug form of the access still shows "backup.example.net"

    Scenario: The password is hidden
      Given the environment variable "DEMO_PASSWORD" is "env-password"
      And the repository's password is in the environment variable "DEMO_PASSWORD"
      When the repository is resolved
      Then the debug form of the access shows no "env-password"

    Scenario: The password in the URL of the repository is hidden
      Given the repository is "rest:https://demo:hunter2@backup.example.net/repo"
      When the repository is resolved
      Then the debug form of the access shows no "hunter2"
      And the debug form of the access still shows "backup.example.net"

    Scenario: The value of every option is hidden and its name is kept
      Given the option "endpoint" is "https://demo:hunter2@s3.example.net"
      And the option "region" is "eu-demo-1"
      And the option "secret_access_key" is "demo-secret-key"
      When the repository is resolved
      Then the debug form of the access shows no "hunter2"
      And the debug form of the access shows no "eu-demo-1"
      And the debug form of the access shows no "demo-secret-key"
      And the debug form of the access still shows "endpoint"
      And the debug form of the access still shows "secret_access_key"

    Scenario: What the references gave is hidden, whichever option holds it
      Given the environment variable "DEMO_ENDPOINT" is "https://s3.example.net/demo"
      And the option "endpoint" is "${DEMO_ENDPOINT}"
      When the repository is resolved
      Then the debug form of the access shows no "https://s3.example.net/demo"
      And the debug form of the access still shows "endpoint"

    Scenario: The hot repository and the hot and cold options are hidden in the access too
      Given the hot repository is "rest:https://demo:hunter4@hot.example.net/repo"
      And the hot option "endpoint" is "https://demo:hunter2@hot-s3.example.net"
      And the hot option "region" is "eu-hot-1"
      And the cold option "secret_access_key" is "demo-cold-key"
      And the cold option "region" is "eu-cold-1"
      When the repository is resolved
      Then the debug form of the access shows no "hunter4"
      And the debug form of the access shows no "hunter2"
      And the debug form of the access shows no "eu-hot-1"
      And the debug form of the access shows no "demo-cold-key"
      And the debug form of the access shows no "eu-cold-1"
      And the debug form of the access still shows "hot.example.net"
      And the debug form of the access still shows "endpoint"
      And the debug form of the access still shows "secret_access_key"

    Scenario: A credential written in an rclone connection string is hidden in either half, quotes and all
      Given the repository is "rclone::sftp,host=nas.example.net,pass='CANARY-rclone-pass,with:colon':/backups"
      And the hot repository is "rclone::b2,account=demo-account,key=CANARY-rclone-b2-key:demo-bucket"
      When the repository is resolved
      Then the debug form of the access shows no "CANARY-rclone"
      And the debug form of the access shows no "with:colon"
      And the debug form of the access still shows "nas.example.net"
      And the debug form of the access still shows "demo-bucket"

  Rule: The debug form of the resolved repository hides the hot and cold fields too

    Scenario: The hot repository and the hot and cold options are hidden
      Given the environment variable "DEMO_HOT_BUCKET" is "demo-hot-bucket"
      And the environment variable "DEMO_HOT_REGION" is "eu-hot-1"
      And the environment variable "DEMO_COLD_REGION" is "eu-cold-1"
      And the hot repository is "opendal:s3:${DEMO_HOT_BUCKET}"
      And the hot option "region" is "${DEMO_HOT_REGION}"
      And the cold option "region" is "${DEMO_COLD_REGION}"
      When the repository is resolved
      Then the debug form of the resolved repository shows no "demo-hot-bucket"
      And the debug form of the resolved repository shows no "eu-hot-1"
      And the debug form of the resolved repository shows no "eu-cold-1"
      And the debug form of the resolved repository still shows "region"

    Scenario: Inline hot and cold options are hidden
      Given the hot option "endpoint" is "https://demo:hunter2@hot.example.net"
      And the cold option "endpoint" is "https://demo:hunter3@cold.example.net"
      And the hot repository is "rest:https://demo:hunter4@hot.example.net/repo"
      When the repository is resolved
      Then the debug form of the resolved repository shows no "hunter2"
      And the debug form of the resolved repository shows no "hunter3"
      And the debug form of the resolved repository shows no "hunter4"
      And the debug form of the resolved repository still shows "endpoint"

    Scenario: The access inside is hidden as well
      Given the environment variable "DEMO_PASSWORD" is "env-password"
      And the environment variable "DEMO_BUCKET" is "demo-bucket-77"
      And the repository's password is in the environment variable "DEMO_PASSWORD"
      And the repository is "s3:backup.example.net/${DEMO_BUCKET}"
      When the repository is resolved
      Then the debug form of the resolved repository shows no "env-password"
      And the debug form of the resolved repository shows no "demo-bucket-77"
      And the debug form of the resolved repository still shows "backup.example.net"

  Rule: The debug form of the settings hides what they hold

    Scenario: An inline password is hidden
      Given the repository's password is "inline-password"
      When the repository is resolved
      Then the debug form of the settings shows no "inline-password"

    Scenario: The password in the URL of the repository is hidden
      Given the repository is "rest:https://demo:hunter2@backup.example.net/repo"
      Then the debug form of the settings shows no "hunter2"
      And the debug form of the settings still shows "backup.example.net"

    Scenario: The value of every option is hidden and its name is kept
      Given the option "endpoint" is "https://demo:hunter2@s3.example.net"
      And the option "sas_token" is "demo-sas-token"
      And the hot option "region" is "eu-hot-1"
      And the cold option "region" is "eu-cold-1"
      Then the debug form of the settings shows no "hunter2"
      And the debug form of the settings shows no "demo-sas-token"
      And the debug form of the settings shows no "eu-hot-1"
      And the debug form of the settings shows no "eu-cold-1"
      And the debug form of the settings still shows "endpoint"
      And the debug form of the settings still shows "sas_token"

    Scenario: The hot repository is hidden
      Given the hot repository is "rest:https://demo:hunter4@hot.example.net/repo"
      Then the debug form of the settings shows no "hunter4"

    Scenario: A credential written in an rclone connection string is hidden
      Given the repository is "rclone::azureblob,account=demo-account,sas_url='CANARY-sas''url':demo-container"
      Then the debug form of the settings shows no "CANARY-sas"
      And the debug form of the settings shows no "url'"
      And the debug form of the settings still shows "demo-container"

    Scenario: A password in the settings that names a source shows the source, not a value
      Given the repository's password is in the file "/home/demo/password"
      Then the debug form of the settings still shows "/home/demo/password"
