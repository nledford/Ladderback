### Ladderback's own command-line messages. The program prints each after its
### name and a colon (`ladderback: listening on …`); `clap`'s help and usage
### errors are not here and stay in English.

# The daemon is up. Scripts and the UI tests read the address after "listening on".
# $url (text): where the web UI answers, such as http://127.0.0.1:9898.
cli-listening = listening on { $url }

# The daemon is up but the operating system would not say where.
# $detail (text): the operating system's message. It is shown as it came.
cli-listening-unknown = listening (address unknown: { $detail })

# The daemon could not take its address.
# $address (text): the address it was asked to listen on, such as 127.0.0.1:9898.
# $detail (text): the operating system's message. It is shown as it came.
cli-cannot-listen = cannot listen on { $address }: { $detail }

# The daemon could not start its async runtime.
# $detail (text): the operating system's message. It is shown as it came.
cli-cannot-start-runtime = cannot start the async runtime: { $detail }

# The web server stopped with an error.
# $detail (text): the server's own message. It is shown as it came.
cli-server-error = server error: { $detail }

# Sign-in is off, so the daemon only listens on loopback addresses.
# $address (text): the address it was asked to listen on, such as 192.168.1.5:9898.
# `127.0.0.1` and `::1` are addresses, typed as they are: keep them string literals.
cli-refuse-non-loopback = refusing to listen on { $address }: sign-in is off, so { -brand-name } only listens on loopback addresses ({ "127.0.0.1" }, { "::1" })

# `--allowed-host` was given a name that no Host header could ever match, which would lock the web UI out.
# $name (text): the value as it was typed. It is shown as it is.
# `--allowed-host`, `nas.example.com`, `192.168.1.5` and `[fd00::1]` are typed as they are: keep them string literals.
cli-invalid-allowed-host = “{ $name }” is not a host name or an address without a port, so { "--allowed-host" } cannot take it. Give a name such as { "nas.example.com" }, or an address such as { "192.168.1.5" } or { "[fd00::1]" }.

# The daemon could not say where its own program file is, so it cannot start
# workers.
# $detail (text): the operating system's message. It is shown as it came.
cli-cannot-find-own-path = cannot find this program's own path: { $detail }

# The platform gives the daemon no configuration folder to read.
cli-no-config-dir = no configuration folder: set LADDERBACK_CONFIG_DIR, or run as a user with a home directory

# The platform gives the daemon no data folder for its operation log.
cli-no-data-dir = no data folder: set LADDERBACK_DATA_DIR, or run as a user with a home directory

# The daemon could not make its data folder, or open the operation log in it.
# $path (text): the folder or database file, as the user wrote it.
# $detail (text): the operating system's or the store's own message. It is shown as it came.
cli-cannot-open-store = cannot open the operation log at { $path }: { $detail }

# The operations an earlier run left queued or running could not be ended in the operation log. The daemon did not start, because serving over them would keep a repository held for good.
# $path (text): the database file, as the user wrote it.
# $detail (text): the store's own message. It is shown as it came.
cli-cannot-recover-operations = cannot end the operations an earlier run left unfinished in the operation log at { $path }: { $detail }

# Another process already holds the data folder, so the daemon did not start.
# $path (text): the data folder, as the user wrote it.
# $pid (text): the ID of the process that holds it.
cli-data-dir-in-use = the data folder { $path } is in use by process { $pid }: stop that process, or start this one with a configuration folder and a data folder of its own ({ "LADDERBACK_CONFIG_DIR" } and { "LADDERBACK_DATA_DIR" })

# As cli-data-dir-in-use, when the lock file names no process.
# $path (text): the data folder, as the user wrote it.
cli-data-dir-in-use-unnamed = the data folder { $path } is in use by another process: stop that process, or start this one with a configuration folder and a data folder of its own ({ "LADDERBACK_CONFIG_DIR" } and { "LADDERBACK_DATA_DIR" })

# The lock on the data folder could not be taken, so the daemon did not start. A file system that does not support locks gives this.
# $path (text): the lock file.
# $detail (text): the operating system's own message. It is shown as it came.
cli-cannot-lock-data-dir = cannot lock the data folder through { $path }: { $detail }

# The configuration file could not be read.
# $file (text): the path of the configuration file.
# $detail (text): the operating system's own message. It is shown as it came.
cli-config-unavailable = cannot read the configuration { $file }: { $detail }

# The configuration file is longer than any configuration needs.
# $file (text): the path of the configuration file.
# $limit (bytes): the most bytes the daemon reads.
cli-config-too-large = the configuration { $file } is larger than { $limit }, so it was not read

# The configuration file doesn't say which version of the format it is in.
# $file (text): the path of the configuration file.
cli-config-schema-missing = the configuration { $file } does not say which version of the format it is in, so the daemon did not start

# The configuration file is in a newer version of the format than this build reads.
# $file (text): the path of the configuration file.
# $found (text): the version the file says it is in, such as 999999.
# $supported (text): the newest version this build reads.
cli-config-schema-newer = the configuration { $file } is in schema version { $found }, newer than the newest this build reads ({ $supported }), so the daemon did not start

# The configuration file is not a valid configuration.
# $file (text): the path of the configuration file.
# $where (text): the setting at fault and where in the file it is, when the reader could tell; otherwise "an earlier rule".
# $detail (text): what is wrong, as the reader reported it.
cli-config-invalid = the configuration { $file } is not valid at { $where }: { $detail }

# An import saved the repository of a profile.
# $repository (text): the name the repository was saved under.
# $file (text): the profile's path, as it was given.
cli-profile-imported = imported the repository { $repository } from { $file }

# An import saved one plan of a profile.
# $plan (text): the plan's ID.
cli-profile-added-plan = added the plan { $plan }

# The name given for the imported repository is not a name the configuration takes.
# $name (text): the name, as it was given.
cli-profile-bad-repository-name = { $name } is not a repository name: use lowercase letters, digits and dashes

# The profile's file name has nothing in it to make a repository name of.
# `--repo-name` is an option, typed as it is: keep it a string literal.
# $file (text): the profile's path, as it was given.
cli-profile-no-repository-name = cannot make a repository name of the file name of { $file }: give one with { "--repo-name" }

# An export needs the agent's ID, which is kept in the data folder, and could not read it.
# $path (text): the data folder.
# $detail (text): the operating system's own message. It is shown as it came.
cli-profile-no-agent-id = cannot read the agent's ID in { $path }: { $detail }

# A command could not write what it had to say, and ended with status 1: the disk is full, say. A reader that has gone is not this case: the command ends quietly with 141.
# $detail (text): the operating system's message. It is shown as it came.
cli-cannot-write-output = cannot write the result: { $detail }
