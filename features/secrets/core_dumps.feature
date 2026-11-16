@e2e
Feature: The daemon, its workers and the MCP server keep the secrets in their memory to themselves
  The daemon holds resolved repository passwords and its sign-in secrets in
  memory, and a worker holds the secrets of its job. An MCP server holds the
  same secrets as the daemon when it runs a session of its own, headless or
  in a scratch folder, and the API token it read from its file when it is
  connected to a running Ladderback, and a one-shot command, `ladderback mcp
  call`, holds the same for its calls. A process that crashes may leave a
  core dump of its memory on disk, and another process of the same user may
  attach a debugger to a running one and read its memory: both outside
  every rule of invariant I4. So the daemon, the MCP server and a one-shot
  command as they start, before they resolve any secret or read a token or
  a script, and each worker before it reads its job, set their core-dump
  limit to 0 (Linux and macOS) and, on Linux, make themselves non-dumpable,
  which also refuses a debugger and reads of their memory by other
  processes of the same user (decision 98). A process that can't logs a
  warning by its code and goes on.

  The limits and the flag are read from `/proc`, which only Linux has: macOS
  has no way to read another process's limits and no dumpable flag, so there
  these steps are skipped and the platform adapter's tests check the limit
  of their own process.

  Scenario: The daemon leaves no core dump and refuses a debugger
    Given Ladderback is serving on loopback
    Then the daemon's core-dump limit is 0
    And other processes can't read the daemon's memory

  Scenario: A worker leaves no core dump and refuses a debugger
    Given a worker is running its job
    Then the worker's core-dump limit is 0
    And other processes can't read the worker's memory

  Scenario: An MCP server with a session of its own leaves no core dump and refuses a debugger
    Given an MCP server running in a scratch folder
    Then the MCP server's core-dump limit is 0
    And other processes can't read the MCP server's memory

  Scenario: An MCP server connected to Ladderback leaves no core dump and refuses a debugger
    Given Ladderback is serving on loopback
    And an MCP server connected to it
    When the MCP client asks the server what it offers
    Then the MCP server's core-dump limit is 0
    And other processes can't read the MCP server's memory

  Scenario: A one-shot command running a script of calls leaves no core dump and refuses a debugger
    Given an agent runs a script of calls from its standard input in a scratch folder
    When the script calls "list_plans" with '{}'
    Then the one-shot command's core-dump limit is 0
    And other processes can't read the one-shot command's memory
