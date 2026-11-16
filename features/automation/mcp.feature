@e2e
Feature: AI agents and scripts use Ladderback through MCP
  `ladderback mcp` speaks the Model Context Protocol on standard input and
  output, so an AI agent or a script reads Ladderback's state through tools
  rather than screens (§4.6). In connect mode it drives a running Ladderback
  through its API, like any other client, with an API token from a file
  only its owner can read when sign-in is on. Headless, it runs a session of
  its own on Ladderback's folders, which only one process may hold at a
  time; in a scratch folder, it runs one on a private folder of its own,
  seeded with a plan, and deletes it at the end. A client of the current
  protocol revision asks the server what it offers without a handshake; a
  client of the two revisions before it starts with one, and both are
  served. Standard output holds protocol messages only. A tool's refusal
  gives its code and arguments, then says what to do next in the words of
  the catalog, in the language of the environment the server runs in.

  `ladderback mcp call` runs tools once, without a client: the same tools,
  with the same checks and results, each call's outcome printed as one JSON
  line, a result or a refusal. With no mode named, it connects to a running
  Ladderback; headless and in a scratch folder it holds a session of its
  own, for as long as its calls take, so a script of calls shares one. A
  refused call's sentence is said on standard error.

  Scenario: A client of the current revision asks what the server offers
    Given Ladderback is serving on loopback
    And an MCP server connected to it
    When the MCP client asks the server what it offers
    Then the server offers the revisions "2025-06-18, 2025-11-25, 2026-07-28"
    And the server offers tools

  Scenario Outline: A client of an earlier revision starts with the handshake
    Given Ladderback is serving on loopback
    And an MCP server connected to it
    When the MCP client starts the handshake with the revision "<revision>"
    Then the server agrees to the revision "<revision>"

    Examples:
      | revision   |
      | 2025-06-18 |
      | 2025-11-25 |

  Scenario: The server lists its tools
    Given Ladderback is serving on loopback
    And an MCP server connected to it
    When the MCP client lists the tools
    Then the tools listed are "status, list_plans, list_repositories, list_snapshots, list_operations, get_operation, run_plan, wait_operation, cancel_operation, test_repository, preview_forget, preview_import, export_profile"

  # Some clients turn a tool's input schema into a function declaration of a
  # narrower schema language, with one type per field and no `format`
  # beyond a few of its own, and a strict one refuses a `minimum`: an
  # optional argument is one left out of `required`, never one whose type is
  # a list with null, and a bound is said in the argument's description.
  Scenario: Every argument of every tool has one type, no format and no minimum
    Given an MCP server running in a scratch folder
    When the MCP client lists the tools
    Then each argument in every tool's input schema has one type, no format and no minimum

  # The agent sees a tool's refusal and can call the tool again; a protocol
  # error some clients show only to the person, or retry as it was.
  Scenario Outline: An argument a tool can't take is refused as the tool's error, naming it
    Given an MCP server running in a scratch folder
    When the MCP client calls the tool "<tool>" with the arguments '<arguments>'
    Then the result is the error "invalid-arguments"
    And the error names the argument "<argument>" as "<problem>"

    Examples:
      | tool           | arguments         | argument | problem     |
      | wait_operation | {"id": "x"}       | id       | unknown-key |
      | run_plan       | {}                | plan     | missing     |
      | list_plans     | {"limit": "many"} | limit    | invalid     |

  # The code and its arguments stay as they are, for a client to branch on.
  # After them the refusal says, as the catalog says it, what to do next, for
  # the AI agent or the person it relays the refusal to.
  Scenario: A refusal says after its code what to do next
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the scopes "run" and the label "runner"
    And an MCP server connected to it with that API token in a file
    When the MCP client calls the tool "status"
    Then the result is the error "api-token-scope"
    And after its code the error is said in "en-US" as the catalog says it

  # An MCP client asks for no language: the sentence follows the locale of the
  # environment the server runs in, as the command line's own messages do.
  Scenario: A refusal is said in the language of the server's environment
    Given an MCP server running in a scratch folder in the language "en-XA"
    When the MCP client calls the tool "run_plan" with the arguments '{}'
    Then the result is the error "invalid-arguments"
    And after its code the error is said in "en-XA" as the catalog says it

  Scenario: The status names each build part by part
    Given Ladderback is serving on loopback
    And an MCP server connected to it
    When the MCP client calls the tool "status"
    Then the result names this checkout's build as the program's: its version, its commit and whether it had changes
    And the result names this checkout's build as the daemon's: its version, its commit and whether it had changes
    And the result says sign-in is off and the daemon answers on loopback
    And standard output held only protocol messages

  Scenario: The status says when no Ladderback answers
    Given an MCP server connected to an address where nothing answers
    When the MCP client calls the tool "status"
    Then the result is the error "daemon-unreachable"

  Scenario: An address that isn't a web address is refused
    When the owner runs "mcp --connect ftp://nas.example.com/"
    Then the command exits with status 2
    And standard error says "isn't an http or https address"

  Scenario: A headless server reads the configuration without a daemon
    Given an MCP server running headless with the plan "home"
    When the MCP client calls the tool "status"
    Then the result lists the plan "home"
    And the result names no daemon
    And the result says the tools reach Ladderback in this process

  Scenario: A headless server is refused while Ladderback serves from the same folders
    Given Ladderback is serving on loopback with the plan "home"
    When an MCP server is started headless on the same folders
    Then the MCP server is refused, naming the data folder and the process that holds it

  Scenario: A second headless server on the same folders is refused
    Given an MCP server running headless with the plan "home"
    When an MCP server is started headless on the same folders
    Then the MCP server is refused, naming the data folder and the process that holds it

  Scenario: A scratch server works in a private folder that is gone once it ends
    Given an MCP server running in a scratch folder
    When the MCP client calls the tool "status"
    Then the result lists the plan "scratch"
    And the scratch folder can be read by its owner only
    When the MCP client closes the session
    Then no scratch folder is left

  Scenario: A scratch folder a crashed session left is swept at the next start
    Given a scratch folder a crashed session left behind
    When an MCP server is started in a scratch folder
    Then the folder the crashed session left is gone

  Scenario: The next page of snapshots continues where the first ended
    Given Ladderback is serving on loopback with the plan "home"
    And an MCP server connected to it
    When the daemon has finished 2 backups of the plan "home"
    And the MCP client lists the snapshots of the repository "demo", 1 to a page
    And the MCP client asks for the next page
    Then the two pages list the repository's two snapshots, newest first
    And no page follows the second

  Scenario: A page asked for with too large a limit lists as many as a page holds
    Given Ladderback is serving on loopback with the plan "home"
    And an MCP server connected to it
    When the daemon has finished 2 backups of the plan "home"
    And the MCP client lists the operations with the limit 1000000
    Then the result lists 2 operations and no page follows

  Scenario: Snapshots are listed of one plan or of one host
    Given Ladderback is serving on loopback with the plan "home"
    And an MCP server connected to it
    When the daemon has finished 2 backups of the plan "home"
    And the MCP client lists the snapshots of the repository "demo" of the plan "home"
    Then the result lists 2 snapshots
    When the MCP client lists the snapshots of the repository "demo" of the plan "work"
    Then the result lists 0 snapshots
    When the MCP client lists the snapshots of the repository "demo" of the host "elsewhere.example"
    Then the result lists 0 snapshots

  Scenario: A read token from a file reaches a daemon with sign-in on
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the scopes "read" and the label "agent"
    And an MCP server connected to it with that API token in a file
    When the MCP client calls the tool "status"
    Then the result says the tools reach the daemon with an API token, and nothing in it says signed out
    And the result names this checkout's build as the daemon's: its version, its commit and whether it had changes

  Scenario: A daemon with sign-in on refuses a server without a token
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And an MCP server connected to it
    When the MCP client calls the tool "status"
    Then the result is the error "sign-in-required"

  Scenario: A token the daemon doesn't know is refused
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And an MCP server connected to it with a token nobody created in a file
    When the MCP client calls the tool "status"
    Then the result is the error "api-token-invalid"

  Scenario: A token without the scope to read is refused, naming the scope
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the scopes "run" and the label "runner"
    And an MCP server connected to it with that API token in a file
    When the MCP client calls the tool "status"
    Then the result is the error "api-token-scope"
    And the error names the scope "read"

  Scenario: A token file that isn't there is refused at start
    When the owner runs "mcp --connect --token-file missing-token"
    Then the command exits with status 1
    And standard error says "isn't there"

  Scenario: A token is never sent over plain HTTP to another machine
    Given an API token file in the configuration folder
    When the owner runs "mcp --connect http://nas.example.com:9898/"
    Then the command exits with status 1
    And standard error says "never over plain"

  Scenario: A token file other users can read is refused at start
    Given an API token file in the configuration folder that other users can read
    When the owner runs "mcp --connect"
    Then the command exits with status 1
    And standard error says "other users"

  Scenario: A headless server lists what the configuration warns of, and never a secret it holds
    Given the daemon's config file holds:
      """
      schema = 1

      [repositories.nas]
      repository = "rest:http://demo:canary-url-password@nas.example.lan:8000/"
      password = "canary-inline-password"

      [[plans]]
      id = "home"
      repository = "nas"
      sources = ["/home/user"]
      globs = ["!docker/data/app/temp"]
      """
    And an MCP server running headless on that configuration
    When the MCP client calls the tool "list_plans"
    Then the plan "home" carries the warning "glob-never-matches"
    When the MCP client calls the tool "list_repositories"
    Then the repository "nas" carries the warning "config-inline-secret"
    And the repository "nas" carries the warning "location-plain-http-credentials"
    And nothing the MCP server wrote shows "canary-url-password" or "canary-inline-password"

  # The engine masks every secret of the job in its own words before the
  # log keeps them (I4); the tool hands on the failure as the log has it.
  Scenario: A failed operation says why, without the repository's password
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's repository password in the configuration is "canary-not-a-secret", which isn't the repository's
    And the plan's repository folder is gone
    And an MCP server connected to it
    When the daemon starts a backup of the plan "home"
    And the MCP client asks for that operation once it has ended
    Then the result gives the operation's failure as "operation-failed", in the engine's own words
    And nothing the MCP server wrote shows "canary-not-a-secret"

  Scenario: An agent runs a plan through the daemon and waits for it to finish
    Given Ladderback is serving on loopback with the plan "home"
    And an MCP server connected to it
    When the MCP client runs the plan "home"
    And the MCP client waits for that operation
    Then the wait says the operation finished as "succeeded"
    And the daemon's operation log records that operation as started by "manual"

  Scenario: An agent runs a plan in a scratch folder, recorded as started through MCP
    Given an MCP server running in a scratch folder
    When the MCP client runs the plan "scratch"
    And the MCP client waits for that operation
    Then the wait says the operation finished as "succeeded"
    And the scratch session's operation log records that operation as started by "mcp"

  # A run names its operation `operation`, and each tool that takes an
  # operation takes it under that name, so an agent passes back what it was
  # given.
  Scenario Outline: A tool takes an operation under the name a run gives it
    Given an MCP server running in a scratch folder
    When the MCP client runs the plan "scratch"
    And the MCP client calls the tool "<tool>" with what the run returned
    Then the answer is about that operation

    Examples:
      | tool             |
      | get_operation    |
      | wait_operation   |
      | cancel_operation |

  # The file the backup waits on holds it running until it is let go.
  Scenario: A wait that ends before the operation does says it hasn't finished
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's source holds a file the backup waits on until it is let go
    And an MCP server connected to it
    When the MCP client runs the plan "home"
    And the MCP client waits 1 second for that operation
    Then the wait says the operation hasn't finished
    When the file the backup waits on is let go
    And the MCP client waits for that operation
    Then the wait says the operation finished as "succeeded"

  # The worker waiting on the file sees the cancel once the file is let go.
  Scenario: An agent cancels a backup it started
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's source holds a file the backup waits on until it is let go
    And an MCP server connected to it
    When the MCP client runs the plan "home"
    And the MCP client cancels that operation once it is running
    And the file the backup waits on is let go
    And the MCP client waits for that operation
    Then the wait says the operation finished as "cancelled"

  Scenario: Maintenance can't be cancelled through MCP
    Given Ladderback is serving on loopback with the plan "home"
    And an MCP server connected to it
    When the owner checks the repository and the check ends
    And the MCP client cancels that check
    Then the result is the error "api-token-scope"
    And the error names the scope "session"

  # The 128 GiB file keeps the backup running until the session ends.
  Scenario: A backup still running when a headless session ends is ended by its stop
    Given a new repository
    And a plan "home" backing up a folder with a 128 GiB file
    And an MCP server running headless on that plan
    When the MCP client runs the plan "home"
    And the MCP client sees that operation running
    And the MCP client closes the session
    Then the daemon's operation log records that operation as ended by the stop

  Scenario: An agent tests the daemon's repository and previews a forget of it
    Given Ladderback is serving on loopback with the plan "home"
    And the daemon's plan keeps the last 1 snapshot
    And the daemon has finished 1 backup of the plan "home"
    And an MCP server connected to it
    When the MCP client tests the repository "demo"
    Then the test finds the repository at its first location
    When the MCP client previews a forget of the repository "demo"
    Then the preview tells what a forget would let go of the plan "home", without a selection to start one

  Scenario: A scratch server tests its repository and previews a forget of it
    Given an MCP server running in a scratch folder
    And the MCP client has run the plan "scratch" to the end
    When the MCP client tests the repository "scratch"
    Then the test finds the repository at its first location
    When the MCP client previews a forget of the repository "scratch"
    Then the preview tells what a forget would let go of the plan "scratch", without a selection to start one

  # I4 where a worker opens the repository (§0): the password its file holds
  # opens it, a wrong one fails, and neither is ever shown or logged.
  Scenario: The tools that open a repository through the daemon never show or log its password
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's repository reads its password from a file
    And the daemon's plan keeps the last 1 snapshot
    And an MCP server connected to it
    And the MCP client has run the plan "home" to the end
    When the MCP client lists the snapshots of the repository "demo", 10 to a page
    Then the result lists 1 snapshots
    When the MCP client tests the repository "demo"
    Then the test finds the repository at its first location
    When the MCP client previews a forget of the repository "demo"
    Then the preview tells what a forget would let go of the plan "home", without a selection to start one
    When the repository's password file holds "canary-not-the-password", which isn't the repository's
    And the MCP client lists the snapshots of the repository "demo"
    Then the result is the error "engine-unavailable"
    When the MCP client tests the repository "demo"
    Then the test tells the first location's outcome as "password-not-accepted"
    When the MCP client previews a forget of the repository "demo"
    Then the result is the error "password-not-accepted"
    And nothing the MCP server or Ladderback wrote or logged shows "e2e-scratch-password" or "canary-not-the-password"

  Scenario: The tools that open a repository in a headless session never show or log its password
    Given an MCP server running headless with the plan "home"
    And the plan's repository reads its password from a file
    And the daemon's plan keeps the last 1 snapshot
    And the MCP client has run the plan "home" to the end
    When the MCP client lists the snapshots of the repository "demo", 10 to a page
    Then the result lists 1 snapshots
    When the MCP client tests the repository "demo"
    Then the test finds the repository at its first location
    When the MCP client previews a forget of the repository "demo"
    Then the preview tells what a forget would let go of the plan "home", without a selection to start one
    When the repository's password file holds "canary-not-the-password", which isn't the repository's
    And the MCP client lists the snapshots of the repository "demo"
    Then the result is the error "engine-unavailable"
    When the MCP client tests the repository "demo"
    Then the test tells the first location's outcome as "password-not-accepted"
    When the MCP client previews a forget of the repository "demo"
    Then the result is the error "password-not-accepted"
    And nothing the MCP server or Ladderback wrote or logged shows "e2e-scratch-password" or "canary-not-the-password"

  Scenario: A forget is previewed only of a repository maintained here
    Given Ladderback is serving on loopback with no repositories
    And the daemon's config file is edited by hand to hold:
      """
      schema = 1

      [repositories.nas]
      repository = "/srv/demo-repo"
      password-env = "DEMO_PASSWORD"
      """
    And an MCP server connected to it
    When the MCP client previews a forget of the repository "nas"
    Then the result is the error "maintenance-not-owner"

  Scenario: A profile's preview names the profiles it uses rather than reading them
    Given Ladderback is serving on loopback with the plan "home"
    And an MCP server connected to it
    When the MCP client previews importing as "nas" the profile:
      """
      [global]
      use-profiles = ["base"]

      [repository]
      repository = "/srv/restic-repo"
      password-file = "/home/demo/.restic-password"

      [[backup.snapshots]]
      name = "laptop"
      sources = ["/home/demo"]
      """
    Then the preview adds the repository "nas" and the plan "laptop"
    And the preview carries the notice "profile-used-not-read"

  Scenario: A profile that isn't TOML is refused without its text
    Given an MCP server running in a scratch folder
    When the MCP client previews importing as "nas" the profile:
      """
      [repository
      password = "canary-profile-password"
      """
    Then the result is the error "profile-invalid"
    And nothing the MCP server wrote shows "canary-profile-password"

  Scenario: A plan whose password the configuration holds is never exported
    Given Ladderback is serving on loopback with the plan "home"
    And the plan's repository password in the configuration is "canary-inline-password", which isn't the repository's
    And an MCP server connected to it
    When the MCP client exports the plan "home" as a profile
    Then the result is the error "inline-secret"
    And nothing the MCP server wrote shows "canary-inline-password"

  Scenario: A scratch server exports its plan as a profile that reads the password from a file
    Given an MCP server running in a scratch folder
    When the MCP client exports the plan "scratch" as a profile
    Then the exported profile reads the repository's password from a file

  Scenario: A one-shot call prints the tool's result as one JSON line
    Given Ladderback is serving on loopback with the plan "home"
    When an agent runs "mcp call list_plans limit=10 --connect {daemon}"
    Then the command exits with status 0
    And standard output has 1 line
    And line 1 says the tool "list_plans" answered, listing the plan "home"

  Scenario Outline: An argument a one-shot call can't take is refused as over MCP, naming it
    Given Ladderback is serving on loopback
    When an agent runs 'mcp call list_plans <arguments> --connect {daemon}'
    Then the command exits with status 1
    And standard output has 1 line
    And line 1 says the tool "list_plans" refused with "invalid-arguments", naming the argument "<argument>" as "<problem>"

    Examples:
      | arguments               | argument | problem     |
      | limit=many              | limit    | invalid     |
      | --args {"limit":"many"} | limit    | invalid     |
      | plan=home               | plan     | unknown-key |

  Scenario: A refused one-shot call says on standard error what to do next
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    When an agent runs "mcp call status --connect {daemon}"
    Then the command exits with status 1
    And standard output has 1 line
    And line 1 says the tool "status" refused with "sign-in-required"
    And standard error says the refusal of line 1 as the catalog says it

  Scenario Outline: A one-shot call line that can't be read is refused before any call
    When an agent runs "mcp call <call line>"
    Then the command exits with status 2
    And standard output is empty
    And standard error says "<words>"

    Examples:
      | call line                    | words               |
      | list_plans limit             | isn't written as    |
      | list_plans limit=1 limit=2   | more than once      |
      | list_plans --args [1]        | one JSON object     |
      | list_plans limit=1 --args {} | cannot be used with |
      | restore                      | no tool named       |
      | status --headless --scratch  | cannot be used with |
      | status --json                | '--json'            |
      | status --connect not-a-url   | isn't an http       |

  Scenario: A token without the scope to run is refused for a run, and never shown
    Given Ladderback is serving on loopback with sign-in on and the admin password "demo-admin-pass"
    And the admin has signed in over HTTP with "demo-admin-pass"
    And the admin has created an API token over HTTP with the scopes "read" and the label "agent"
    And that API token in a token file
    When an agent runs "mcp call run_plan plan=home --connect {daemon} --token-file {token file}"
    Then the command exits with status 1
    And standard output has 1 line
    And line 1 says the tool "run_plan" refused with "api-token-scope", naming the scope "run"
    And nothing the command printed holds the token

  Scenario: A one-shot call never sends a token over plain HTTP to another machine, nor shows it
    Given a token nobody created in a token file
    When an agent runs "mcp call status --connect http://nas.example.com:9898/ --token-file {token file}"
    Then the command exits with status 1
    And standard output is empty
    And standard error says "never over plain"
    And nothing the command printed holds the token

  Scenario: A headless one-shot call is refused while Ladderback serves from the same folders
    Given Ladderback is serving on loopback with the plan "home"
    When an agent runs "mcp call status --headless"
    Then the command exits with status 1
    And standard output is empty
    And the call is refused, naming the data folder and the process that holds it

  Scenario Outline: A run on its own is refused before it starts when the call's own session would stop it
    When an agent runs "mcp call run_plan plan=home <session>"
    Then the command exits with status 2
    And standard output is empty
    And standard error says "wait_operation"
    And no scratch folder is left

    Examples:
      | session    |
      | --headless |
      | --scratch  |

  Scenario: A script of calls shares one scratch session, so a run can be waited for
    Given an agent runs a script of calls from its standard input in a scratch folder
    When the script calls "run_plan" with '{"plan": "scratch"}'
    And the script calls "wait_operation" with the operation the run returned
    And the script ends
    Then the command exits with status 0
    And standard output has 2 lines
    And line 2 says the tool "wait_operation" answered that the operation finished as "succeeded"
    And no scratch folder is left

  Scenario Outline: A failed call stops a script of calls, unless it is to keep going
    Given a file of calls "calls.jsonl" holding:
      """
      {"tool": "run_plan", "arguments": {"plan": "nowhere"}}
      {"tool": "list_plans"}
      """
    When an agent runs "mcp call --scratch --script {scratch}/calls.jsonl <keep going>"
    Then the command exits with status 1
    And standard output has <lines>
    And line 1 says the tool "run_plan" refused with "plan-unknown"
    And no scratch folder is left

    Examples:
      | keep going   | lines   |
      |              | 1 line  |
      | --keep-going | 2 lines |

  Scenario: A script's refused calls say on standard error what to do next, in the order of the calls
    Given a file of calls "calls.jsonl" holding:
      """
      {"tool": "run_plan", "arguments": {"plan": "nowhere"}}
      {"tool": "run_plan", "arguments": {"plan": "elsewhere"}}
      """
    When an agent runs "mcp call --scratch --script {scratch}/calls.jsonl --keep-going"
    Then the command exits with status 1
    And standard output has 2 lines
    And line 2 says the tool "run_plan" refused with "plan-unknown"
    And standard error says the refusals of lines 1 and 2, in that order
    And no scratch folder is left

  Scenario: A script's lines that aren't calls are said by their numbers, never their text
    Given a file of calls "calls.jsonl" holding:
      """
      ["status"]

      {"tool": "erase_everything", "arguments": {"password": "canary-script-password"}}
      {"tool": "list_plans"}
      """
    When an agent runs "mcp call --scratch --script {scratch}/calls.jsonl --keep-going"
    Then the command exits with status 1
    And standard output has 1 line
    And standard error says "line 1 of the script isn't a call"
    And standard error says "line 3 of the script names no tool that exists"
    And standard error never says "erase_everything"
    And standard error never says "canary-script-password"
    And no scratch folder is left

  Scenario: A script of calls that can't be read is named by its path
    Given a folder "calls.d" where a file of calls would be
    When an agent runs "mcp call --scratch --script {scratch}/calls.d"
    Then the command exits with status 1
    And standard output is empty
    And standard error says "cannot read the script of calls"
    And standard error says "calls.d"
    And no scratch folder is left
