@e2e
Feature: A worker and its backend never show a secret
  A backend that can't connect says what it called, and the URL it called
  holds what the repository's settings held. The settings go through the
  resolver, the daemon masks every value they gave wherever the backend
  quotes it, and a worker starts from an environment with nothing in it that
  a secret came from (invariant I4). The backends here are on a port where
  nothing listens, so the first thing that fails is the connection.

  Rule: What the backend quotes is masked

    Scenario: A secret read from a file is masked where the backend quotes it
      Given an S3 repository on a port where nothing listens
      And the file "{scratch}/endpoint-secret" holds "CANARY-endpoint-file"
      And the repository's option "endpoint" is "http://127.0.0.1:9/${file:{scratch}/endpoint-secret}"
      When the repository is identified
      Then the operation ends as failed because the connection to "127.0.0.1:9" was refused
      And the failure shows "***"
      And nothing the engine reported or logged shows "CANARY-endpoint-file"

    Scenario: A SAS token written in the settings is masked where the backend quotes it
      Given an Azure Blob repository on a port where nothing listens
      And the repository's option "sas_token" is "sv=CANARY-sas-token"
      When the repository is identified
      Then the operation ends as failed because the connection to "127.0.0.1:9" was refused
      And the failure shows "***"
      And nothing the engine reported or logged shows "CANARY-sas-token"

    Scenario: The password in the URL of an endpoint is masked where the backend quotes it
      Given an S3 repository on a port where nothing listens
      And the repository's option "endpoint" is "http://demo:CANARY-endpoint-password@127.0.0.1:9"
      When the repository is identified
      Then the operation ends as failed because the connection to "127.0.0.1:9" was refused
      And the failure shows "***"
      And nothing the engine reported or logged shows "CANARY-endpoint-password"

  Rule: A worker starts from a scrubbed environment

    Scenario: A worker isn't given the variable a secret came from
      Given the daemon's environment has the variable "DEMO_REPO_SECRET" set to "CANARY-from-environment"
      And an S3 repository on a port where nothing listens
      And the repository's option "region" is "${DEMO_REPO_SECRET}"
      And a worker that writes down what it was given
      When the repository is identified
      Then the worker was given the job with "CANARY-from-environment"
      And the worker was given the variable "PATH"
      But the worker was given no variable "DEMO_REPO_SECRET"
      And the worker was given no "CANARY-from-environment" in its arguments or its environment
