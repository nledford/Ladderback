Feature: Import a rustic profile
  A person who backs up with rustic already has a profile: the repository,
  what to back up, how much history to keep, hooks. `ladderback import-profile`
  reads that file and adds the repository and a plan for each snapshot entry
  to the agent's configuration (technical design §6.2). A profile is input
  nobody vetted, so the import takes only what Ladderback can honour exactly,
  says what it left out, and refuses what it could only guess at: the
  configuration changes whole, or not at all.

  Rule: The import adds a profile's repository and plans to the configuration, or nothing

    Scenario: A profile's repository and plans are added
      Given a profile holding the plan "home"
      And a profile holding the plan "docs"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the configuration now holds the repository "nas"
      And the configuration now holds the plan "home"
      And the configuration now holds the plan "docs"

    Scenario: A plan the configuration already has is refused
      Given the configuration holds the repository "cloud"
      And the configuration holds the plan "home" for the repository "cloud"
      And a profile holding the plan "home"
      When the owner imports the profile as the repository "nas"
      Then the import is refused because the plan "home" is already taken
      And the configuration is unchanged

    Scenario: Two plans of one profile with one ID are refused
      Given a profile holding the plan "data"
      And a profile holding the plan "data"
      When the owner imports the profile as the repository "nas"
      Then the import is refused because the plan "data" is already taken
      And the configuration is unchanged

    Scenario: A repository alias the configuration already has is refused
      Given the configuration holds the repository "nas"
      And a profile holding the plan "home"
      When the owner imports the profile as the repository "nas"
      Then the import is refused because the repository "nas" is already taken
      And the configuration is unchanged

    Scenario: A profile for a repository the configuration has under that name adds its plans to it
      Given the configuration holds the repository "nas" at the profile's location
      And the configuration holds the plan "home" for the repository "nas"
      And a profile holding the plan "docs"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the configuration now holds 1 repository
      And the configuration now holds the plan "home" for the repository "nas"
      And the configuration now holds the plan "docs" for the repository "nas"

    Scenario: A profile for a repository the configuration has under another name is refused
      Given the configuration holds the repository "nas" at the profile's location
      And a profile holding the plan "home"
      When the owner imports the profile as the repository "backup"
      Then the import is refused because the repository is configured as "nas"
      And the configuration is unchanged

    Scenario: A profile for a repository the configuration has under that name, set up otherwise, keeps the configuration's settings and says which
      Given the configuration holds the repository "nas" at the profile's location, maintained by the hub
      And a profile holding the plan "docs"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the configuration's repository "nas" is as it was
      And the import notes that the repository "nas" kept its own "maintenance"

    Scenario: A profile for another bucket under a name the configuration has is refused
      Given the configuration holds the repository "nas" at the profile's location with the option "bucket" set to "photos"
      And a profile holding the plan "home"
      When the owner imports the profile as the repository "nas"
      Then the import is refused because the repository "nas" is already taken
      And the configuration is unchanged

    Scenario: A profile for a repository the configuration has under that name, with a credential only the configuration has, adds its plans to it
      Given the configuration holds the repository "nas" at the profile's location with the option "secret_access_key" set to "${AWS_SECRET_ACCESS_KEY}"
      And a profile holding the plan "docs"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the configuration's repository "nas" is as it was
      And the import notes that the repository "nas" kept its own "options"

    Scenario: A profile for a repository the configuration has under another name, with a credential only the configuration has, is refused
      Given the configuration holds the repository "nas" at the profile's location with the option "secret_access_key" set to "${AWS_SECRET_ACCESS_KEY}"
      And a profile holding the plan "home"
      When the owner imports the profile as the repository "backup"
      Then the import is refused because the repository is configured as "nas"
      And the configuration is unchanged

    # On a drive (Dropbox, Google Drive, OneDrive, Yandex Disk) only a
    # credential names the account, so it is part of where the repository is.
    Scenario: A profile for a drive under a name the configuration has, with another credential, is refused
      Given the configuration holds the repository "nas" at "opendal:dropbox" with the option "access_token" set to "${DROPBOX_TOKEN}"
      And the profile's repository is at "opendal:dropbox" with the option "access_token" set to "not-a-real-token"
      And a profile holding the plan "home"
      When the owner imports the profile as the repository "nas"
      Then the import is refused because the repository "nas" is already taken
      And the configuration is unchanged

    Scenario: A profile for a drive the configuration has with another credential is added under a new name
      Given the configuration holds the repository "nas" at "opendal:dropbox" with the option "access_token" set to "${DROPBOX_TOKEN}"
      And the profile's repository is at "opendal:dropbox" with the option "access_token" set to "not-a-real-token"
      And a profile holding the plan "home"
      When the owner imports the profile as the repository "work"
      Then the import succeeds
      And the configuration now holds the repository "work"
      And the configuration now holds the plan "home" for the repository "work"

    Scenario: A profile that cannot be read changes nothing
      Given the configuration holds the repository "cloud"
      And a profile that cannot be read
      When the owner imports the profile as the repository "nas"
      Then the import is refused because the profile cannot be read
      And the configuration is unchanged

    Scenario: What the profile could not carry over is handed back
      Given a profile holding the plan "home"
      And the profile could not carry over the key "copy"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the import notes that the key "copy" was left out

  Rule: The agent takes the host every imported plan agrees on

    Scenario: Plans that agree on a host give it to the agent
      Given a profile holding the plan "home" on the host "demo-laptop"
      And a profile holding the plan "docs" on the host "demo-laptop"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the agent's hostname is now "demo-laptop"
      And the plan "home" has no host of its own
      And the plan "docs" has no host of its own

    Scenario: A host the agent already has is not repeated on the plans
      Given the agent's hostname is "demo-laptop"
      And a profile holding the plan "home" on the host "demo-laptop"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the agent's hostname is now "demo-laptop"
      And the plan "home" has no host of its own

    Scenario: A host that is not the agent's stays with its plan
      Given the agent's hostname is "demo-desktop"
      And a profile holding the plan "home" on the host "demo-laptop"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the agent's hostname is now "demo-desktop"
      And the plan "home" has the host "demo-laptop"

    Scenario: Plans that disagree on a host each keep their own
      Given a profile holding the plan "home" on the host "demo-laptop"
      And a profile holding the plan "docs" on the host "demo-desktop"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the agent has no hostname of its own
      And the plan "home" has the host "demo-laptop"
      And the plan "docs" has the host "demo-desktop"

    Scenario: An agent that already has plans keeps the host they were made under
      Given the configuration holds the repository "cloud"
      And the configuration holds the plan "work" for the repository "cloud"
      And a profile holding the plan "home" on the host "demo-laptop"
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the agent has no hostname of its own
      And the plan "home" has the host "demo-laptop"

  Rule: The profile's global hooks run after the agent's own

    Scenario: Imported global hooks are appended
      Given the agent's hooks run "agent-hook" before everything
      And a profile holding the plan "home"
      And the profile's global hooks run "profile-hook" before everything
      When the owner imports the profile as the repository "nas"
      Then the import succeeds
      And the agent's hooks run "agent-hook" and then "profile-hook" before everything

  @e2e
  Rule: A plan gets what rustic would back up with

    Scenario: A snapshot entry inherits what the backup section sets and replaces its lists
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [backup]
        git-ignore = true
        exclude-if-present = [".nobackup", "CACHEDIR.TAG"]

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]

        [[backup.snapshots]]
        name = "docs"
        sources = ["/home/demo/docs"]
        exclude-if-present = [".skip"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                                    | value                         |
        | repositories.nas.repository            | "{scratch}/repo"              |
        | repositories.nas.password-file         | "{scratch}/password"          |
        | plans.home.repository                  | "nas"                         |
        | plans.home.sources                     | ["/home/demo"]                |
        | plans.home.git-ignore                  | true                          |
        | plans.home.exclude-if-present          | [".nobackup", "CACHEDIR.TAG"] |
        | plans.docs.git-ignore                  | true                          |
        | plans.docs.exclude-if-present          | [".skip"]                     |

    Scenario: A repository alias is the profile's file name, made into a slug
      Given a rustic profile "Laptop Profile.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "Laptop Profile.toml"
      Then the import succeeds
      And the saved config has:
        | key                                    | value            |
        | repositories.laptop-profile.repository | "{scratch}/repo" |

    Scenario: The owner can name the repository instead
      Given a rustic profile "Laptop Profile.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "Laptop Profile.toml" as the repository "nas"
      Then the import succeeds
      And the saved config has:
        | key                         | value            |
        | repositories.nas.repository | "{scratch}/repo" |

    Scenario: A profile's own value beats the profile it uses
      Given a rustic profile "base.toml" holding:
        """
        [repository]
        repository = "{scratch}/used"
        password-file = "{scratch}/password"
        """
      And a rustic profile "nas.toml" holding:
        """
        [global]
        use-profiles = ["base"]

        [repository]
        repository = "{scratch}/own"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                            | value                |
        | repositories.nas.repository    | "{scratch}/own"      |
        | repositories.nas.password-file | "{scratch}/password" |

    Scenario: A profile used by another is found in rustic's config folder before the working folder
      Given a rustic profile "base.toml" holding:
        """
        [backup]
        exclude-if-present = [".from-the-working-folder"]
        """
      And a rustic profile "base.toml" in rustic's config folder holding:
        """
        [backup]
        exclude-if-present = [".from-the-config-folder"]
        """
      And a rustic profile "nas.toml" holding:
        """
        [global]
        use-profiles = ["base"]

        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                           | value                       |
        | plans.home.exclude-if-present | [".from-the-config-folder"] |

    Scenario: A profile used by another that cannot be found is noted
      Given a rustic profile "nas.toml" holding:
        """
        [global]
        use-profiles = ["ghost"]

        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import notes that the profile "ghost" wasn't found

    Scenario: A used profile is a file name, never a path
      Given a rustic profile "nas.toml" holding:
        """
        [global]
        use-profiles = ["../outside"]

        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "use-profiles"
      And no config file was written

    Scenario: Profiles that use each other are refused
      Given a rustic profile "one.toml" holding:
        """
        [global]
        use-profiles = ["two"]
        """
      And a rustic profile "two.toml" holding:
        """
        [global]
        use-profiles = ["one"]
        """
      When the owner imports the profile "one.toml"
      Then the import is refused naming "use-profiles"
      And no config file was written

  @e2e
  Rule: A plan's ID comes from what names it

    Scenario: A name, else a label, else the folder it backs up
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "Home Dir"
        label = "Ignored Label"
        sources = ["/home/demo"]

        [[backup.snapshots]]
        label = "Work Laptop Docs"
        sources = ["/home/demo/work"]

        [[backup.snapshots]]
        sources = ["/srv/demo data/"]

        [[backup.snapshots]]
        sources = ["/"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                              | value               |
        | plans.home-dir.sources           | ["/home/demo"]      |
        | plans.work-laptop-docs.label     | "Work Laptop Docs"  |
        | plans.demo-data.sources          | ["/srv/demo data/"] |
        | plans.plan.sources               | ["/"]               |

    Scenario: Two entries that get one ID are refused
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        sources = ["/home/demo/data"]

        [[backup.snapshots]]
        sources = ["/srv/demo/data"]
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "data"
      And no config file was written

    Scenario: A plan the configuration already has is refused, and the file stays as it was
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
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "home"
      And the config file is exactly as it was written

    Scenario: A host every entry agrees on becomes the agent's
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [backup]
        host = "demo-laptop"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]

        [[backup.snapshots]]
        name = "docs"
        sources = ["/home/demo/docs"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key            | value         |
        | agent.hostname | "demo-laptop" |
      And the saved config doesn't set "plans.home.host"
      And the saved config doesn't set "plans.docs.host"

    Scenario: An entry's source must be an absolute path
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["docs"]
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "sources"
      And no config file was written

    Scenario: Tags Ladderback keeps for itself are dropped
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        tags = ["laptop", "ladderback-plan:old"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key             | value      |
        | plans.home.tags | ["laptop"] |
      And the import notes that the tag "ladderback-plan:old" was dropped

  @e2e
  Rule: Retention, pruning and grouping follow the forget section

    Scenario: Every plan gets the profile's retention
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [forget]
        keep-last = 3
        keep-daily = 7

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]

        [[backup.snapshots]]
        name = "docs"
        sources = ["/home/demo/docs"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                              | value |
        | plans.home.retention.keep-last   | 3     |
        | plans.home.retention.keep-daily  | 7     |
        | plans.docs.retention.keep-last   | 3     |
        | plans.docs.retention.keep-daily  | 7     |

    Scenario Outline: A rule that keeps particular snapshots is refused, not ignored
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [forget]
        keep-last = 3
        <rule>

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "<key>"
      And no config file was written

      Examples:
        | rule                    | key       |
        | keep-tags = ["precious"] | keep-tags |
        | keep-ids = ["abcd1234"]  | keep-ids  |

    Scenario: A snapshot filter beside retention is refused, because rustic's forget reaches fewer snapshots than Ladderback's
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [forget]
        keep-last = 3

        [snapshot-filter]
        filter-hosts = ["demo-laptop"]

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "snapshot-filter.filter-hosts"
      And no config file was written

    Scenario: Prune is not copied; the repository asks for local maintenance with a warning
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [forget]
        keep-last = 3
        prune = true

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import warns that pruning will run from one place
      And the saved config has:
        | key                            | value   |
        | repositories.nas.maintenance   | "local" |
      And the saved config doesn't set "plans.home.retention.prune"

    Scenario: Without prune the repository asks for no maintenance
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [forget]
        keep-last = 3

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config doesn't set "repositories.nas.maintenance"

    Scenario: The forget section's grouping is the repository's, before the global one
      Given a rustic profile "nas.toml" holding:
        """
        [global]
        group-by = "tags"

        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [forget]
        group-by = "host,paths"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                         | value        |
        | repositories.nas.group-by   | "host,paths" |

    Scenario: The backup section's grouping is left out, and the import says so
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [backup]
        group-by = "host"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import notes that the key "backup.group-by" was left out
      And the saved config doesn't set "repositories.nas.group-by"

  @e2e
  Rule: Hooks keep the order rustic runs them in

    Scenario: Hooks land on the agent, the repository and each plan
      Given a rustic profile "nas.toml" holding:
        """
        [global.hooks]
        run-before = ["echo agent"]

        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [repository.hooks]
        run-before = ["echo repository"]

        [backup.hooks]
        run-before = ["echo backup-before"]
        run-after = ["echo backup-after"]

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]

        [backup.snapshots.hooks]
        run-before = ["echo entry-before"]
        run-after = ["echo entry-after"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import warns that backup hooks now run once per plan
      And the saved config has:
        | key                                  | value                                |
        | hooks.run-before                     | ["echo agent"]                       |
        | repositories.nas.hooks.run-before    | ["echo repository"]                  |
        | plans.home.hooks.run-before          | ["echo backup-before", "echo entry-before"] |
        | plans.home.hooks.run-after           | ["echo entry-after", "echo backup-after"]   |

  @e2e
  Rule: What Ladderback does not carry over is said, and what it cannot honour is refused

    Scenario: Sections of other purposes are left out one notice each
      Given a rustic profile "nas.toml" holding:
        """
        [global]
        log-level = "debug"

        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [copy]
        targets = ["/srv/copy"]

        [webdav]
        address = "localhost:8000"

        [snapshot-filter]
        filter-hosts = ["demo-laptop"]

        [global.env]
        DEMO_VARIABLE = "demo"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import notes that these keys were left out:
        | key             |
        | log-level       |
        | copy            |
        | webdav          |
        | snapshot-filter |
      And the import warns that the profile's environment was left out
      And the saved config file doesn't hold "DEMO_VARIABLE"

    Scenario: A name or sources on the backup section can't be a plan and are refused
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [backup]
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "backup.sources"
      And no config file was written

    Scenario: A key rustic does not know is refused where it is written
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [forget]
        keep-last = 3
        keep-lots = 7
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "forget.keep-lots"
      And the refusal points at line 7 of the profile
      And no config file was written

    Scenario: A profile that is not TOML is refused
      Given a rustic profile "nas.toml" holding:
        """
        [repository
        repository = "{scratch}/repo"
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "nas.toml"
      And no config file was written

    Scenario: A profile larger than the cap is refused unread
      Given a rustic profile "nas.toml" larger than 1 MiB
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "nas.toml"
      And no config file was written

    Scenario: A profile that does not exist is refused
      When the owner imports the profile "missing.toml"
      Then the import is refused naming "missing.toml"
      And no config file was written

    Scenario: A profile that has rustic substitute variables in the profiles it uses is refused
      Given a rustic profile "nas.toml" holding:
        """
        [global]
        profile-substitute-env = true

        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "global.profile-substitute-env"
      And no config file was written

  @e2e
  Rule: Secrets stay references, and rustic's variables fill only what the profile leaves out

    Scenario: An inline password is kept with a warning, and never shown
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password = "CANARY-inline-password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import warns that the secret "repository.password" is written in the config
      And the import never shows "CANARY-inline-password"

    Scenario: A password in the repository's location is kept with a warning that names the key, and never shown
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "rest:https://demo:CANARY-location-password@backup.example.com/nas"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import warns that the location in "repository.repository" holds a password written in the config
      And the import never shows "CANARY-location-password"

    Scenario: A credential in an rclone location is kept with a warning that sends it to rclone's own configuration
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "rclone::sftp,host=nas.example.com,pass=CANARY-rclone-pass:/backups"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import warns that the location in "repository.repository" holds a credential that belongs in rclone's own configuration
      And the import never shows "CANARY-rclone-pass"

    Scenario Outline: A location with no password written in it gets no warning
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "<location>"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import gives no warning about "repository.repository"

      Examples:
        | location                                 |
        | rest:https://backup.example.com/nas      |
        | rest:https://demo@backup.example.com/nas |
        | rclone:demo-remote:backups/token=2024    |

    Scenario: A profile with nowhere to read the password from is refused
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import is refused naming "password"
      And no config file was written

    Scenario: A key file is not carried over, and the password source stays
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"
        key-file = "{scratch}/key"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the import notes that the key "repository.key-file" was left out
      And the saved config has:
        | key                            | value                |
        | repositories.nas.password-file | "{scratch}/password" |

    Scenario: A reference in a repository value is written so that it stays text
      Given a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "s3:${file:/etc/passwd}/bucket"
        password-file = "{scratch}/password"

        [repository.options]
        endpoint = "https://${HOME}/path"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                                  | value                           |
        | repositories.nas.repository          | "s3:$${file:/etc/passwd}/bucket" |
        | repositories.nas.options.endpoint    | "https://$${HOME}/path"          |

    Scenario: Rustic's password variable becomes a reference when the profile has no source
      Given rustic's environment sets "RUSTIC_PASSWORD" to "CANARY-environment-password"
      And a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                           | value             |
        | repositories.nas.password-env | "RUSTIC_PASSWORD" |
      And the saved config file doesn't hold "CANARY-environment-password"
      And the import never shows "CANARY-environment-password"

    Scenario: A password source the profile names beats rustic's variable
      Given rustic's environment sets "RUSTIC_PASSWORD" to "CANARY-environment-password"
      And a rustic profile "nas.toml" holding:
        """
        [repository]
        repository = "{scratch}/repo"
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                            | value                |
        | repositories.nas.password-file | "{scratch}/password" |
      And the saved config doesn't set "repositories.nas.password-env"

    Scenario: Rustic's repository and option variables fill what the profile leaves out, as references
      Given rustic's environment sets "RUSTIC_REPOSITORY" to "/srv/demo-repo"
      And rustic's environment sets "RUSTIC_REPO_OPT_ENDPOINT" to "https://demo.example.lan"
      And a rustic profile "nas.toml" holding:
        """
        [repository]
        password-file = "{scratch}/password"

        [[backup.snapshots]]
        name = "home"
        sources = ["/home/demo"]
        """
      When the owner imports the profile "nas.toml"
      Then the import succeeds
      And the saved config has:
        | key                               | value                       |
        | repositories.nas.repository       | "${RUSTIC_REPOSITORY}"      |
        | repositories.nas.options.endpoint | "${RUSTIC_REPO_OPT_ENDPOINT}" |
      And the saved config file doesn't hold "/srv/demo-repo"
