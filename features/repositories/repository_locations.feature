@repositories
Feature: A repository reached at several locations
  A repository may list its locations in order. Before a job opens it the
  agent tries each in turn and uses the first that answers and holds the same
  repository, by the ID its entry records: a location that holds another
  repository is refused and never used. A location with no rule that can't
  be used is skipped without a warning, since its address may be someone
  else's on this network; a location that holds another repository, or whose
  secret can't be read, is a warning. Each location keeps its own options and
  credentials, and a job carries only the location it uses (I4). A location
  may carry a rule about the network link the machine is on: it is tried
  only when its rule holds, and a rule that can't be evaluated doesn't hold.
  The operation records which location it used and why each earlier one was
  skipped, by position and outcome, never by a network's name (decision 80).

  Background:
    Given a repository "nas" that records its repository ID, with the locations "lan" and "public", in that order
    And the plan "home" backs up to "nas"

  Rule: The first location that answers and holds the repository is used

    Scenario: The first location is used when it answers
      Given the location "lan" answers
      When the plan "home" runs
      Then the operation ends as succeeded
      And the operation used the location "lan"
      And the location "public" was never tried
      And the operation shows the location "public" as not tried

    Scenario: The next location is used when the first doesn't answer
      Given the location "lan" doesn't answer within 8 seconds
      And the location "public" answers
      When the plan "home" runs
      Then the operation ends as succeeded
      And the operation used the location "public"
      And the operation says the location "lan" was skipped because it didn't answer
      And the operation has no warnings

    Scenario: A location that holds another repository is refused and never used
      Given the location "lan" answers with a repository whose ID isn't the repository's
      And the location "public" answers
      When the plan "home" runs
      Then the operation used the location "public"
      And the operation ends as succeeded with warnings
      And the operation warns that the location "lan" held another repository
      And nothing was written to the other repository

    Scenario Outline: A location with no rule that can't be used is skipped without a warning
      Given the location "lan" <state>
      And the location "public" answers
      When the plan "home" runs
      Then the operation used the location "public"
      And the operation says the location "lan" was skipped because it <reason>
      And the operation has no warnings

      Examples:
        | state                                                | reason                                  |
        | answers and refuses its credentials                  | refused its credentials                 |
        | answers and holds no repository                      | held no repository                      |
        | answers and doesn't accept the repository's password | didn't accept the repository's password |

    Scenario: A reference of a location that doesn't resolve skips that location with a warning
      Given the location "lan" has a credential reference that doesn't resolve
      And the location "public" answers
      When the plan "home" runs
      Then the operation used the location "public"
      And the operation warns that the location "lan" couldn't be tried because a secret was unavailable
      And the warning names the source and never a value

  Rule: A location is tried only when its rule holds

    Scenario: A location whose rule doesn't hold is skipped
      Given the location "lan" is eligible only on a wired network
      And the machine is on Wi-Fi
      And the location "public" answers
      When the plan "home" runs
      Then the operation used the location "public"
      And the operation says the location "lan" was skipped because its rule didn't hold
      And the location "lan" was never probed
      And the operation has no warnings

    Scenario: A location whose rule holds is probed and used
      Given the location "lan" is eligible only on a wired network
      And the machine is on a wired network
      And the location "lan" answers
      When the plan "home" runs
      Then the operation used the location "lan"

    Scenario: A location whose rule held and that doesn't answer is a warning
      Given the location "lan" is eligible only on a wired network
      And the machine is on a wired network
      And the location "lan" doesn't answer
      And the location "public" answers
      When the plan "home" runs
      Then the operation used the location "public"
      And the operation ends as succeeded with warnings
      And the operation warns that the location "lan" was skipped though its rule held

    Scenario: A rule that can't be evaluated doesn't hold
      Given the location "lan" is eligible only on a wired network
      And the machine can't tell what kind of network it is on
      And the location "public" answers
      When the plan "home" runs
      Then the operation used the location "public"
      And the operation says the location "lan" was skipped because its rule couldn't be evaluated

    Scenario: A rule holds when any of its alternatives does
      Given the location "lan" is eligible on a wired network or on any Wi-Fi network
      And the machine is on Wi-Fi
      When the plan "home" runs
      Then the location "lan" is probed

    Scenario: A person's Test says a location's rule didn't hold, without probing it
      Given the location "lan" is eligible only on a wired network
      And the machine is on Wi-Fi
      When a person tests the locations of the repository "nas"
      Then the test says the location "lan" was skipped because its rule didn't hold
      And the location "lan" was never probed
      And the test says the location "public" answered

    Scenario: A location eligible only on a named Wi-Fi network is tried there
      Given the location "lan" is eligible only on the Wi-Fi network "Office"
      And the machine is on the Wi-Fi network "Office"
      And the location "lan" answers
      When the plan "home" runs
      Then the operation used the location "lan"

    Scenario: A location eligible only on a named Wi-Fi network is skipped on another one
      Given the location "lan" is eligible only on the Wi-Fi network "Office"
      And the machine is on the Wi-Fi network "Guest"
      And the location "public" answers
      When the plan "home" runs
      Then the operation used the location "public"

    Scenario: A location eligible on any Wi-Fi network except one is skipped on that one
      Given the location "lan" is eligible on any Wi-Fi network except "Guest"
      And the machine is on the Wi-Fi network "Guest"
      And the location "public" answers
      When the plan "home" runs
      Then the operation used the location "public"

    Scenario: A network name the machine can't read means the rule doesn't hold
      Given the location "lan" is eligible only on the Wi-Fi network "Office"
      And the machine can't read the Wi-Fi network's name
      And the location "public" answers
      When the plan "home" runs
      Then the operation used the location "public"

  Rule: A run whose locations' rules all fail to hold is skipped or fails

    Scenario: A scheduled run is skipped when every location has a rule and none is known to hold
      Given both locations are eligible only on a wired network
      And the machine is on Wi-Fi
      When the plan "home" is due
      Then the run of the plan "home" is skipped because no location's rule holds
      And no operation is recorded

    Scenario: A scheduled run still starts when a rule can't be evaluated, and fails where it is seen
      Given the plan "home" retries up to 3 times
      And both locations are eligible only on a wired network
      And the machine can't tell what kind of network it is on
      When the plan "home" is due
      Then the plan "home" runs
      And the operation ends as failed with the reason "no location's rule holds"
      And a retry of the plan "home" is scheduled

    Scenario: A run that waited in the queue while the network changed fails when it starts
      Given both locations are eligible only on a wired network
      And a run of the plan "home" is queued while the machine is on a wired network
      When the machine moves to Wi-Fi before the run starts
      Then the operation ends as failed with the reason "no location's rule holds"

    Scenario: A manual run fails when no location's rule holds
      Given both locations are eligible only on a wired network
      And the machine is on Wi-Fi
      When a person runs the plan "home" now
      Then the operation ends as failed with the reason "no location's rule holds"
      And the operation names each location and why its rule didn't hold

    Scenario: A rule on a repository of one location is honoured
      Given a repository "solo" with one location that is eligible only on a wired network
      And the machine is on Wi-Fi
      When a person runs a backup of "solo" now
      Then the operation ends as failed with the reason "no location's rule holds"
      And no location was probed

    Scenario: A repository of one location whose rule holds is opened without a probe
      Given a repository "solo" with one location that is eligible only on a wired network
      And the machine is on a wired network
      When a person runs a backup of "solo" now
      Then the operation ends as succeeded
      And no location was probed
      And the repository was opened at the location "solo" only

    Scenario: A repository of one location whose rule holds still refuses another repository
      Given a repository "solo" with one location that is eligible only on a wired network
      And the machine is on a wired network
      And the location "solo" answers with a repository whose ID isn't the repository's
      When a person runs a backup of "solo" now
      Then the operation ends as failed because its location holds another repository
      And no location was probed
      And the operation records no location

  Rule: A run that finds no usable location fails, and only a scheduled one is retried

    Scenario: A password no location accepts is reported as a password error
      Given both locations answer and don't accept the repository's password
      When a person runs the plan "home" now
      Then the operation ends as failed with a password error
      And the operation doesn't end with the reason "no location usable"

    Scenario: No location answers
      Given the plan "home" retries up to 3 times
      And neither location answers
      When the plan "home" runs as a scheduled run
      Then the operation ends as failed with the reason "no location usable"
      And the operation names each location and why it wasn't used
      And a retry of the plan "home" is scheduled

    Scenario: A manual run with no usable location fails and is not retried
      Given the plan "home" retries up to 3 times
      And neither location answers
      When a person runs the plan "home" now
      Then the operation ends as failed with the reason "no location usable"
      And no retry of the plan "home" is scheduled

  Rule: A job opens only the location the choice made, with only its secrets

    Scenario: A job opens only the location the choice made
      Given the location "lan" doesn't answer
      And the location "public" answers
      When a forget is previewed for the repository "nas"
      Then the plan's retention was applied at the location "public"
      And the repository was opened at the location "public" only
      And the location "lan" was never opened

    Scenario: A location the job didn't use never had its secrets in the job
      Given the location "public" carries the canary secret "canary-value" in its options
      And the location "lan" answers
      When the plan "home" runs
      Then the job's worker was given no secret of the location "public"
      And the canary appears in no log line, event, operation record or page

    Scenario: A repository with one location is opened without a probe
      Given a repository "solo" with one location
      When a backup of "solo" runs
      Then no location was probed
      And the operation records no location

    Scenario: The cache slot is the repository's, whichever location is used
      Given the location "public" is used for one backup and the location "lan" for the next
      Then both backups read the cache slot named by the repository ID
      And no probe took a cache slot

  Rule: The configuration keeps locations to one repository each

    # The configuration file's own reader refuses these, at the key it names,
    # so they run against the file as the daemon reads it.
    @e2e
    Scenario: Locations are refused on a repository that records no ID
      Given a repository "fresh" that records no repository ID
      When the configuration gives it a second location
      Then the configuration is refused at the table of that location
      And the message says to run a job once or to write the ID

    @e2e
    Scenario: Two locations of one repository may not be at one location
      When the configuration gives the repository "nas" a second location at the address of its first
      Then the configuration is refused at the second location's key

  Rule: A location that sends its credentials where an impostor could read them is warned about

    Scenario Outline: A location that sends its credentials where an impostor could read them is warned about
      Given the location "lan" is "<address>" with <credentials>
      When the repositories are listed
      Then the repository "nas" carries the warning "location-plain-http-credentials" for the location 1 with the cause "<cause>"
      And the warning names neither the address nor a credential

      Examples:
        | address                        | credentials                              | cause                 |
        | rest:http://lan.example.lan/r  | a user and a password in it              | plain-http            |
        | rest:http://lan.example.lan/r  | a credential option                      | plain-http            |
        | rest:https://lan.example.lan/r | the option http-insecure-tls set to true | unchecked-certificate |
        | opendal:s3                     | an http endpoint and a credential option | plain-endpoint        |
        | opendal:ftp                    | a credential option                      | plain-ftp             |

    Scenario: A location over HTTPS with credentials and a checked certificate isn't warned about
      Given the location "lan" is "rest:https://lan.example.lan/r" with a user and a password in it
      When the repositories are listed
      Then the repository "nas" carries no warning about its locations

    Scenario: The warning follows a reference once it is resolved
      Given the location "lan" is "rest:${LAN_URL}" with a user and a password in it
      And the variable "LAN_URL" holds an http address
      When the plan "home" runs
      Then the daemon's log warns that the location 1 sends its credentials over plain HTTP
      And the operation has no warnings

  Rule: The Repositories page notes what the locations' rules mean for a plan's runs

    Scenario: A repository whose every location has a rule is noted
      Given both locations are eligible only on a wired network
      When the repositories are listed
      Then the repository "nas" notes that every location has a rule

    Scenario: A last location with a rule is noted for a scheduled plan that doesn't retry
      Given the location "public" is eligible only on a wired network
      And the plan "home" is scheduled and doesn't retry a failed run
      When the repositories are listed
      Then the repository "nas" notes that its last location has a rule and the plan "home" doesn't retry

    Scenario: A plan that retries a failed run isn't noted
      Given the location "public" is eligible only on a wired network
      And the plan "home" is scheduled and retries up to 3 times
      When the repositories are listed
      Then the repository "nas" carries no note about its locations' rules

    @ui
    Scenario: The Repositories page says when a location may be tried
      Given the location "public" is eligible only on a wired network
      When I open the Repositories page
      Then the location "public" of the repository "nas" says it may be tried on a wired network

    @ui
    Scenario: The Repositories page says when this machine can't read the network's name
      Given the location "lan" is eligible only on the Wi-Fi network "Office"
      And the machine can't read the Wi-Fi network's name
      When I open the Repositories page
      Then the repository "nas" says this machine can't read the Wi-Fi network's name and the rule doesn't apply

  Rule: What the person sees names a location by its position, never by a secret

    Scenario: The network's name is never logged
      Given the machine is on the Wi-Fi network "canary-network"
      And the location "lan" is eligible only on the Wi-Fi network "Office"
      When the plan "home" runs
      Then "canary-network" appears in no log line, event or operation record

    @e2e
    Scenario: A secret of a location is never shown
      Given the location "lan" carries the canary secret "canary-value" in its option "application_key" as the reference "${LAN_KEY}"
      When the plan "home" runs
      Then the canary appears in no log line, event, operation record or page
      And the Repositories page shows the reference "${LAN_KEY}" and never its value

    @e2e
    Scenario: An export names the first location and says what it left out
      When the plan "home" is exported as a rustic profile
      Then the profile's repository is the location "lan"
      And the export says that 1 further location was left out

    @ui
    Scenario: The Repositories page lists the locations and tests each
      When I open the Repositories page
      Then the repository "nas" lists the location "lan" first and the location "public" second
      When I press "Test" for the repository "nas"
      Then each location shows whether it answers

    @ui
    Scenario: The Operations page names the location used
      Given the plan "home" ran and used the location "public"
      When I open the Operations page
      Then the row for the plan "home" says "Used location 2 of 2"
