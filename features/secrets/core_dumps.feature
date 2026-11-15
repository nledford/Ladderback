@e2e
Feature: The daemon and its workers keep the secrets in their memory to themselves
  The daemon holds resolved repository passwords and its sign-in secrets in
  memory, and a worker holds the secrets of its job. A process that crashes
  may leave a core dump of its memory on disk, and another process of the
  same user may attach a debugger to a running one and read its memory: both
  outside every rule of invariant I4. So the daemon as it starts, before it
  resolves any secret, and each worker before it reads its job, set their
  core-dump limit to 0 (Linux and macOS) and, on Linux, make themselves
  non-dumpable, which also refuses a debugger and reads of their memory by
  other processes of the same user (decision 98). A process that can't logs
  a warning by its code and goes on.

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
