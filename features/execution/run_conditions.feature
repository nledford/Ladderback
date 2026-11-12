Feature: Conditions before a scheduled run
  Before a plan's scheduled run starts, the agent checks the conditions the
  plan sets: a plan may ask to run only on AC power. The agent also notices
  when the machine wakes from sleep, so that a run missed while it slept can
  catch up, and a catch-up run after a wake first waits a while for the
  network (scheduling-and-config §5). A condition the machine can't report
  never holds a backup back: a missed backup costs more than some battery.

  Which runs catch up after a wake is the schedule's (`NextRun`), and the
  scheduler that starts them is #274; these scenarios drive the checks those
  runs go through.

  Scenario: A plan that asks for AC power doesn't run on battery
    Given the plan "home" asks to run only on AC power
    And the machine is on battery
    When the plan "home" is due
    Then the run of the plan "home" is skipped because the machine is on battery

  Scenario: A plan that asks for AC power runs on AC power
    Given the plan "home" asks to run only on AC power
    And the machine is on AC power
    When the plan "home" is due
    Then the plan "home" runs

  Scenario: A plan that doesn't ask for AC power runs on battery
    Given the plan "home" doesn't ask for AC power
    And the machine is on battery
    When the plan "home" is due
    Then the plan "home" runs

  Scenario: A plan that doesn't ask for AC power runs without asking what powers the machine
    Given the plan "home" doesn't ask for AC power
    And the machine can't tell what powers it
    When the plan "home" is due
    Then the plan "home" runs

  Scenario: A machine that can't tell what powers it runs the plan
    Given the plan "home" asks to run only on AC power
    And the machine can't tell what powers it
    When the plan "home" is due
    Then the plan "home" runs, though the machine couldn't tell what powers it

  Scenario: The agent notices that the machine woke from sleep
    Given the agent watches for wakes
    When the machine sleeps for 3 hours
    Then within 30 seconds of waking the agent notices that it slept for 3 hours

  Scenario: A machine that stays awake doesn't wake
    Given the agent watches for wakes
    When 10 minutes pass with the machine awake
    Then the agent notices no wake

  Scenario Outline: Only a sleep of more than 2 minutes is a wake
    Given the agent watches for wakes
    When the machine sleeps for <asleep>
    Then the agent <notices>

    Examples:
      | asleep             | notices                                      |
      | 2 minutes          | notices no wake                              |
      | 2 minutes 1 second | notices that it slept for 2 minutes 1 second |

  Scenario: A catch-up run after a wake waits for the network
    Given the network is down
    And the network comes up 40 seconds after the machine wakes
    When the machine wakes and a catch-up run waits for the network
    Then the run goes ahead within 5 seconds of the network coming up

  Scenario: A catch-up run waits at most 2 minutes for the network
    Given the network is down
    And the network stays down
    When the machine wakes and a catch-up run waits for the network
    Then the run goes ahead 2 minutes after the wake, without the network

  Scenario: A catch-up run doesn't wait when the network is up
    Given the network is up
    When the machine wakes and a catch-up run waits for the network
    Then the run goes ahead at once

  Scenario: A catch-up run doesn't wait when the machine can't tell whether it is online
    Given the machine can't tell whether it is online
    When the machine wakes and a catch-up run waits for the network
    Then the run goes ahead at once, though the machine couldn't tell whether it is online
