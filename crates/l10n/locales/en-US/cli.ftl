### Ladderback's own command-line messages. The program prints each after its
### name and a colon (`ladderback: listening on …`); `clap`'s help and usage
### errors are not here and stay in English, but for the descriptions of the
### service commands (`cli-help-*`).

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

# Sign-in is off and the daemon listens on an address beyond loopback, so anyone who can reach that address can use it. Printed by the daemon itself, never as a log record, so no log setting hides it.
# $address (text): the address it listens on, such as 0.0.0.0:9898.
# `ladderback set-password` is typed as it is: keep it a string literal.
cli-warn-network-sign-in-off = warning: sign-in is off and { -brand-name } answers on the network at { $address }, so anyone who can reach that address can run and change backups. To turn sign-in on, run { "ladderback set-password" } and start { -brand-name } again.

# Sign-in is off and the daemon listens on loopback only, but a host name beyond loopback is allowed, so a reverse proxy can bring anyone who reaches that name to it. One line for each such name. Printed by the daemon itself, never as a log record, so no log setting hides it.
# $host (text): the allowed host name, such as backup.example. It is shown as it was given.
# `ladderback set-password` is typed as it is: keep it a string literal.
cli-warn-sign-in-off-behind-proxy = warning: sign-in is off and { $host } is an allowed host name, so anyone who can reach { $host } through the proxy can run and change backups. To turn sign-in on, run { "ladderback set-password" } and start { -brand-name } again.

# The daemon was asked to listen beyond loopback, with sign-in on or off, but no host name was configured, so no browser on the network could use the web UI.
# $address (text): the address it was asked to listen on, such as 0.0.0.0:9898.
# `--allowed-host` and `nas.example.com` are typed as they are: keep them string literals.
cli-refuse-no-allowed-host = refusing to listen on { $address }: beyond loopback, { -brand-name } needs the host name browsers reach it by. Give one with { "--allowed-host" }, such as { "--allowed-host nas.example.com" }.

# The admin password file is there but can't be read, or isn't a password hash, so the daemon does not start: it never runs with sign-in off by mistake.
# $path (text): the file. It is shown as it is.
# $detail (text): the platform's message. It is shown as it came.
cli-admin-password-unreadable = cannot read the admin password file { $path } ({ $detail }). Set the password again with { "ladderback set-password" }, or remove the file to turn sign-in off.

# `ladderback set-password` asks for the new admin password at a terminal; what is typed is not shown. Printed without the program's name, and the answer follows on the same line.
cli-set-password-prompt = New admin password:

# `ladderback set-password` asks for the new admin password a second time, to catch a typing mistake no one could see.
cli-set-password-prompt-again = Type the new admin password again:

# The two passwords typed at the terminal differ.
# `ladderback set-password` is typed as it is: keep it a string literal.
cli-set-password-mismatch = the two passwords differ, so the admin password was not changed. Run { "ladderback set-password" } again.

# The new admin password is too short.
# $min (count): the fewest characters an admin password has, such as 8.
cli-set-password-too-short = the admin password was not changed: it needs at least { $min } characters.

# The new admin password is longer than any admin password may be.
# $max (count): the most bytes an admin password has, such as 1024.
cli-set-password-too-long = the admin password was not changed: it may be at most { $max } bytes long.

# The password could not be read from the terminal or standard input: it was not UTF-8 text, was too long to be a password, or the terminal failed.
# $detail (text): the operating system's message. It is shown as it came.
cli-set-password-unreadable = cannot read the new admin password: { $detail }

# The admin password's hash could not be written to the data folder.
# $path (text): the file. It is shown as it is.
# $detail (text): the platform's message. It is shown as it came.
cli-set-password-unwritable = cannot keep the admin password in { $path }: { $detail }

# The admin password is set, and sign-in is on from now. A daemon already running with sign-in on uses the new password at once and signs everyone out; one running with sign-in off turns sign-in on only when it starts again.
cli-set-password-done = admin password set. Every session has ended: sign in again with the new password. A daemon running with sign-in off turns it on when it next starts.

# The admin password is set, but the sessions Ladderback keeps for after a restart could not be forgotten. They have ended all the same: none is let in again, and Ladderback forgets them when it next starts.
# $path (text): the database file. It is shown as it is.
# $detail (text): the database's message. It is shown as it came.
cli-set-password-sessions-left = the sessions kept in { $path } could not be forgotten: { $detail }. They have ended all the same, and Ladderback forgets them when it next starts.

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

# As cli-data-dir-in-use, on Linux: the holder may be the installed service, which systemd starts again once its process is killed, so the message names how to stop it for good.
# $path (text): the data folder, as the user wrote it.
# $pid (text): the ID of the process that holds it.
cli-data-dir-in-use-systemd = the data folder { $path } is in use by process { $pid }: stop that process, or start this one with a configuration folder and a data folder of its own ({ "LADDERBACK_CONFIG_DIR" } and { "LADDERBACK_DATA_DIR" }). If that process is the installed service, its service manager starts it again once it is killed: stop it with { "systemctl --user stop ladderback" }, or remove it with { "ladderback remove-service" }

# As cli-data-dir-in-use-systemd, when the lock file names no process.
# $path (text): the data folder, as the user wrote it.
cli-data-dir-in-use-unnamed-systemd = the data folder { $path } is in use by another process: stop that process, or start this one with a configuration folder and a data folder of its own ({ "LADDERBACK_CONFIG_DIR" } and { "LADDERBACK_DATA_DIR" }). If that process is the installed service, its service manager starts it again once it is killed: stop it with { "systemctl --user stop ladderback" }, or remove it with { "ladderback remove-service" }

# As cli-data-dir-in-use, on macOS: the holder may be the installed service, which launchd starts again once its process is killed, so the message names how to stop it for good.
# $path (text): the data folder, as the user wrote it.
# $pid (text): the ID of the process that holds it.
cli-data-dir-in-use-launchd = the data folder { $path } is in use by process { $pid }: stop that process, or start this one with a configuration folder and a data folder of its own ({ "LADDERBACK_CONFIG_DIR" } and { "LADDERBACK_DATA_DIR" }). If that process is the installed service, its service manager starts it again once it is killed: stop it with { "launchctl bootout gui/$(id -u)/io.github.nledford.ladderback" }, or remove it with { "ladderback remove-service" }

# As cli-data-dir-in-use-launchd, when the lock file names no process.
# $path (text): the data folder, as the user wrote it.
cli-data-dir-in-use-unnamed-launchd = the data folder { $path } is in use by another process: stop that process, or start this one with a configuration folder and a data folder of its own ({ "LADDERBACK_CONFIG_DIR" } and { "LADDERBACK_DATA_DIR" }). If that process is the installed service, its service manager starts it again once it is killed: stop it with { "launchctl bootout gui/$(id -u)/io.github.nledford.ladderback" }, or remove it with { "ladderback remove-service" }

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

# The configuration gives a repository further locations, and the repository records no ID.
# $file (text): the path of the configuration file.
# $where (text): the setting that is missing and where in the file the first further location is.
cli-config-locations-need-id = the configuration { $file } gives a repository further locations but no { "repository-id" }, at { $where }, so the daemon did not start. Run a job on it once with its first location only, which records the ID, or write the ID the Repositories page shows as { "repository-id" }.

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
cli-profile-no-agent-id = cannot read the { -agent }'s ID in { $path }: { $detail }

# A command could not write what it had to say, and ended with status 1: the disk is full, say. A reader that has gone is not this case: the command ends quietly with 141.
# $detail (text): the operating system's message. It is shown as it came.
cli-cannot-write-output = cannot write the result: { $detail }

## The service (operations §12): `ladderback install-service` and `ladderback remove-service`.

# What `install-service` does, in its `--help`.
# `install-service` is the command, typed as it is: keep it a string literal.
cli-help-install-service = Install this { -agent } as a service of your user: the service manager runs it in the background, now and from each login on. On Linux that is a systemd user unit, on macOS a launchd agent.

# What `remove-service` does, in its `--help`.
# `install-service` is the command, typed as it is: keep it a string literal.
cli-help-remove-service = Stop the service that { "install-service" } installed, and remove its service file.

## `ladderback check-keyring` (#892): reads each keyring entry the configuration names, once, with this program.

# What `check-keyring` does, in its `--help`.
cli-help-check-keyring = Read, with this program, each keyring entry the configuration's repositories and notification channels take a secret from, once, and say whether it could be read, never what it holds. On macOS the keychain asks once for each new build before it lets the build read an entry: run this after an update, while you are there to answer.

# A repository's keyring entry was read. Nothing it holds is kept or shown.
# $repository (text): the repository's name in the configuration, such as nas. It is shown as it is.
# $entry (text): the keyring entry, as the configuration names it (service/name). It is shown as it is.
cli-check-keyring-read = { $repository }: read the keyring entry { $entry }

# A repository's keyring entry could not be read, so a backup of the repository would be refused too.
# $repository (text): the repository's name in the configuration, such as nas. It is shown as it is.
# $entry (text): the keyring entry, as the configuration names it (service/name). It is shown as it is.
# $code (text): the refusal's code, such as secret-keyring-unavailable. It is shown as it is.
# $reason (text): the refusal's own message, one or more whole sentences.
cli-check-keyring-refused = { $repository }: could not read the keyring entry { $entry } ({ $code }). { $reason }

# A notification channel's keyring entry (its address, token or password) was read. Nothing it holds is kept or shown.
# $channel (text): the channel's name in the configuration, such as alerts. It is shown as it is.
# $entry (text): the keyring entry, as the configuration names it (service/name). It is shown as it is.
cli-check-keyring-channel-read = notification channel { $channel }: read the keyring entry { $entry }

# A notification channel's keyring entry could not be read, so a notification through the channel would not be sent either.
# $channel (text): the channel's name in the configuration, such as alerts. It is shown as it is.
# $entry (text): the keyring entry, as the configuration names it (service/name). It is shown as it is.
# $code (text): the refusal's code, such as secret-keyring-unavailable. It is shown as it is.
cli-check-keyring-channel-refused = notification channel { $channel }: could not read the keyring entry { $entry } ({ $code })

# No repository or notification channel of the configuration takes a secret from the keyring, so there was nothing to read.
cli-check-keyring-none = no { -repository } or notification channel takes a secret from the keyring: there is nothing to read

# The service file was written and the service manager started the service.
# $file (text): the service file's path.
cli-service-installed = installed the service { $file }: it runs now, and from each login on

# A service was installed already: its file was written again, and the service manager started it again.
# $file (text): the service file's path.
cli-service-replaced = replaced the service { $file } and started it again

# Linux only, after an install, when loginctl doesn't say the user lingers already: a user's services stop when the user logs out, unless the user lingers.
# `loginctl enable-linger` is a command, typed as it is: keep it a string literal.
cli-service-linger = to keep it running while you are logged out, run { "loginctl enable-linger" }

# macOS only, after an install: nothing can ask macOS for Full Disk Access, so the person grants it to the installed program in System Settings.
# $program (text): the path of the program the service runs.
cli-service-full-disk-access = to back up Mail, Messages, Safari and other apps' data, turn on { -full-disk-access } for { $program } in System Settings, under Privacy & Security; without it, backups read less. The start page says whether it is missing

# Linux only, after an install, when repositories read their password from the keyring: a service that lingers starts at boot, before the login that starts the Secret Service, so it can't read the password until someone logs in.
# $count (number): how many repositories read their password from the keyring.
# `password-file` is a setting and `chmod 600` a command, typed as they are: keep them string literals.
cli-service-keyring-before-login = { $count ->
    [one] a repository reads its password from the keyring, which a service that runs while you are logged out can't read before you log in: to back it up while you are logged out, keep its password in a file only you can read ({ "chmod 600" }) and name it with { "password-file" }
   *[other] { $count } repositories read their passwords from the keyring, which a service that runs while you are logged out can't read before you log in: to back them up while you are logged out, keep their passwords in files only you can read ({ "chmod 600" }) and name them with { "password-file" }
}

# The service manager stopped the service, and its file was removed.
# $file (text): the service file's path.
cli-service-removed = removed the service { $file }: it no longer runs

# There was no service file, so nothing was asked of the service manager.
# $file (text): where the service file would be.
cli-service-not-installed = no service to remove: { $file } is not there

# The service file is kept under the home folder, and there is none.
cli-service-no-home = no home folder to keep the service file in: run as a user with a home directory

# The command was run as root, as under `sudo`. A user service is the user's: as root it would be root's service, with root's files.
cli-service-as-root = a user service is not installed or removed as root: run this command as the user the service is for, without sudo

# This system has no service manager Ladderback can install a service with.
cli-service-unsupported = this system has no service manager { -brand-name } knows: it installs a service with systemd on Linux and with launchd on macOS

# The service file would name this program's path, and that path cannot be
# written into one. One message for each reason, since a command-line message
# shows each argument as it is given.
# $path (text): the program's path, with any control character shown as an escape.
cli-service-program-relative = cannot install the service: its file cannot name this program's path, { $path }, which is relative. Run { -brand-name } from a path without one.

# `..` is typed as it is: keep it a string literal.
# $path (text): the program's path, with any control character shown as an escape.
cli-service-program-steps-up = cannot install the service: its file cannot name this program's path, { $path }, which steps up a folder with { ".." }. Run { -brand-name } from a path without one.

# $path (text): the program's path, with any control character shown as an escape.
cli-service-program-control = cannot install the service: its file cannot name this program's path, { $path }, which holds a control character. Run { -brand-name } from a path without one.

# $path (text): the program's path, with any control character shown as an escape.
cli-service-program-quote = cannot install the service: its file cannot name this program's path, { $path }, which holds a quote or a backslash. Run { -brand-name } from a path without one.

# $path (text): the program's path, with any control character shown as an escape.
cli-service-program-not-text = cannot install the service: its file cannot name this program's path, { $path }, which is not text. Run { -brand-name } from a path without one.

# The service file could not be written, read or removed.
# $file (text): the service file's path.
# $detail (text): the operating system's own message. It is shown as it came.
cli-service-file-error = cannot change the service file { $file }: { $detail }

# The service manager's program could not be started: it is not installed, or not on the PATH.
# $command (text): the command that was run, as a person would type it.
# $detail (text): the operating system's own message. It is shown as it came.
cli-service-manager-missing = cannot run { $command }: { $detail }

# The service manager's program ended with an error.
# $command (text): the command that was run, as a person would type it.
# $said (text): what it printed, in its own words. It is shown as it came.
cli-service-manager-refused = { $command } failed: { $said }

# The service manager's program did not end within the time it is given, and was stopped.
# $command (text): the command that was run, as a person would type it.
# $limit (duration): how long it was given.
cli-service-manager-silent = { $command } did not answer within { $limit }, so { -brand-name } stopped it

# The service manager was asked to stop the service, and still had it when Ladderback stopped waiting: nothing more was done, so an install started no new service, and the service file is as it was. The old service is no longer running once the manager has stopped it.
# $command (text): the command that asked the manager to stop the service, as a person would type it.
# $limit (duration): how long Ladderback waited.
cli-service-still-stopping = { $command } had not stopped the service after { $limit }, so { -brand-name } went no further and left the service file as it was. Once the service has stopped, run this command again.

# A file is at the service file's place that `install-service` did not write: a unit written by hand, or a link such as the one `systemctl mask` makes. Neither command replaces nor removes it.
# $file (text): the service file's path.
cli-service-not-ours = { $file } is not a service file { -brand-name } wrote, so { -brand-name } leaves it as it is

## The MCP server (components §4.6): `ladderback mcp`.

# `ladderback mcp --connect` was given an address it can't use, and did not start. The address isn't repeated: it may hold a password.
cli-mcp-connect-invalid = the address after { "--connect" } isn't an http or https address of a host, such as { "http://127.0.0.1:9898" } or { "https://nas.example.com" }, with no user name, password, query or fragment

# The MCP server's client of the daemon's API could not start, so `ladderback mcp` did not start.
# $detail (text): the HTTP library's own message. It is shown as it came.
cli-mcp-cannot-start = cannot start the MCP server's client of the daemon: { $detail }

# `ladderback mcp --scratch` could not make or seed its scratch folder, so it did not start. Nothing of the folder is left.
# $path (text): the temporary folder the scratch folder was to be made in.
# $detail (text): the operating system's or the configuration's own message. It is shown as it came.
cli-mcp-scratch-unavailable = cannot make a scratch folder in { $path }: { $detail }

# `ladderback mcp --connect --token-file` named a file that isn't there, so it did not start. Without the flag, a missing default token file means no token is sent.
# $file (text): the token file's path, as it was given. It is shown as it is.
cli-mcp-token-file-missing = the token file { $file } that { "--token-file" } names isn't there

# `ladderback mcp --connect` could not use its token file, so it did not start. The token itself is never shown.
# $file (text): the token file's path. It is shown as it is.
# $detail (text): what is wrong, in the operating system's or the program's own words, such as that other users can read it. It is shown as it came.
cli-mcp-token-file-unreadable = cannot use the token file { $file }: { $detail }

# `ladderback mcp --connect` has an API token but its address is plain HTTP to another machine, where anyone on the way could read the token, so it did not start.
cli-mcp-token-over-plain-http = an { -api-token } is sent only to an { "https" } address or one on this computer, never over plain { "http" } to another machine: connect to the daemon through { "https" }, or without a token

## One-shot calls (components §4.6): `ladderback mcp call`.

# `ladderback mcp call` was given the name of a tool that doesn't exist, so it called nothing.
# $tool (text): the name given. It is shown as it is.
cli-mcp-call-no-tool = there is no tool named { $tool }: { "ladderback mcp" } lists its tools to a client, such as { "status" } and { "list_plans" }

# `ladderback mcp call` was given an argument after the tool's name that isn't written as NAME=VALUE, so it called nothing. The argument isn't repeated: it may hold a password.
# $position (count): which argument after the tool's name, counted from 1.
cli-mcp-call-not-a-pair = the argument at position { $position } after the tool's name isn't written as { "NAME=VALUE" }, so no tool was called

# `ladderback mcp call` was given the same argument twice, so it called nothing.
# $name (text): the argument's name, as it was given.
cli-mcp-call-repeated = the argument { $name } is given more than once, so no tool was called

# `ladderback mcp call --args` was given something other than one JSON object, so it called nothing. What was given isn't repeated: it may hold a password.
cli-mcp-call-not-an-object = the arguments after { "--args" } aren't one JSON object, such as { "{\"limit\": 10}" }, so no tool was called

# `ladderback mcp call run_plan` was given on its own with --headless or --scratch, so it called nothing: the session the command starts ends after the call, and its end stops the backup the call would start.
cli-mcp-call-run-alone = a run started on its own here would stop when this command's session ends, before it finished, so no tool was called: give { "run_plan" } and then { "wait_operation" } in a script with { "--script" }, or call a running Ladderback with { "--connect" }

# `ladderback mcp call --script` could not read its script of calls, so it called nothing more.
# $file (text): the script's path as it was given, or "-" for standard input. It is shown as it is.
# $detail (text): the operating system's own message. It is shown as it came.
cli-mcp-call-script-unreadable = cannot read the script of calls { $file }: { $detail }

# A line of the script of calls isn't a call, so it was not made and counts as a failed call. The line isn't repeated: it may hold a password.
# $line (count): the line's number in the script, counted from 1.
cli-mcp-call-line-not-a-call = line { $line } of the script isn't a call: one JSON object, with the tool's name under { "tool" } and its arguments, an object, under { "arguments" }

# A line of the script of calls names a tool that doesn't exist, so it was not made and counts as a failed call. The name isn't repeated: the line may hold a password.
# $line (count): the line's number in the script, counted from 1.
cli-mcp-call-line-no-tool = line { $line } of the script names no tool that exists: { "ladderback mcp" } lists its tools to a client, such as { "status" } and { "list_plans" }

# A line of the script of calls is longer than a call may be, so it was not read and counts as a failed call.
# $line (count): the line's number in the script, counted from 1.
# $limit (bytes): the longest line read.
cli-mcp-call-line-too-long = line { $line } of the script is longer than { $limit }, so it was not read

# `ladderback pairing-token` writes the hub's store from its own process, and the daemon holds the data folder.
# $path (text): the data folder. It is shown as it is.
# `ladderback serve --hub` is typed as it is: keep it a string literal.
cli-pairing-token-daemon-running = the daemon is running on the data folder { $path }. Stop it, make the token, then start it again with { "ladderback serve --hub" }.

# The address given for the hub isn't an http:// or https:// address of at most 200 bytes.
# $address (text): the address as given. It is shown as it came.
# `https://name` and `https://name:port` are examples of what a person types, with `name` standing for the hub's name: keep them string literals.
cli-pairing-token-bad-address = { $address } isn't a { -hub } address: give it as { "https://name" } or { "https://name:port" }, as { -agent(form: "other") } reach the { -hub }.

# The token's address is loopback, which another machine can't reach.
# `--address` is typed as it is: keep it a string literal.
cli-pairing-token-loopback = The address is loopback: another machine can't reach it. Use { "--address" } with the address { -agent(form: "other") } will use.

# The token's address is plain http:// on another machine, which `ladderback pair` refuses because it would show the token to the network.
# `http://`, `https://` and `--address` are typed as they are: keep them string literals.
cli-pairing-token-plain = The address starts with { "http://" } and isn't this machine: an { -agent } won't pair over it, as it would show the token to the network. Use { "--address" } with the { -hub }'s { "https://" } address.

# The hub key's fingerprint, printed before the token so the person has something to compare with the agent's screen.
# $fingerprint (text): the fingerprint, in groups of four characters.
cli-pairing-token-hub-key = The { -hub } key: { $fingerprint }

# Printed after the token: it is one-time, its token ID, and when it lapses.
# $id (text): the token ID, 8 hex digits.
# $expires (text): when the token lapses, as an RFC 3339 instant in UTC.
# `ladderback pairing-token --revoke` is typed as it is: keep it a string literal.
cli-pairing-token-made = This token works once. Token ID { $id }, usable until { $expires }. Revoke it with { "ladderback pairing-token --revoke" } { $id }.

# `ladderback pairing-token --list` found no unused token.
cli-pairing-token-none = no unused pairing tokens

# `ladderback pairing-token --revoke` revoked the token.
# $id (text): the token ID.
cli-pairing-token-revoked = token { $id } is revoked: it can't pair any machine now.

# There is no unused token with this ID: it was used, lapsed, revoked or never made.
# $id (text): the token ID as given.
# `ladderback pairing-token --list` is typed as it is: keep it a string literal.
cli-pairing-token-no-such = there is no unused token { $id }. See the unused ones with { "ladderback pairing-token --list" }.

# The life asked for is outside 5 minutes to 24 hours.
# `--ttl` is typed as it is: keep it a string literal.
cli-pairing-token-bad-ttl = a token lives 5 to 1440 minutes: give { "--ttl" } in that range.

# The hub keeps at most a few unused tokens at once.
# `ladderback pairing-token --revoke` is typed as it is: keep it a string literal.
cli-pairing-token-too-many = there are too many unused tokens. Use one, wait for one to lapse, or revoke one with { "ladderback pairing-token --revoke" }.

# A hub command failed for a reason the system gave.
# $detail (text): the system's message. It is shown as it came.
cli-hub-command-failed = the command failed: { $detail }

# `ladderback pair` asks for the token at a terminal; what is pasted is not shown. Printed without the program's name, and the paste follows on the same line.
cli-pair-prompt = Paste the pairing token:

# What was pasted isn't a pairing token, or its checksum doesn't match, as when a chat app changed the case of its letters.
cli-pair-unreadable-token = this isn't a pairing token, or it was changed on the way (letters' case matters). Copy it again from the { -hub }.

# The hostname to pair under isn't one the hub takes.
# $name (text): the hostname. It is shown as it came.
# `--hostname` is typed as it is: keep it a string literal.
cli-pair-bad-hostname = { $name } can't name this machine to the { -hub }. Give another with { "--hostname" }.

# What `pair` read from a token whose address is checked against this machine's trust store, before it asks; nothing is sent yet.
# $count (count): how many characters were read.
# $address (text): the hub's address.
# $fingerprint (text): the hub key's fingerprint.
cli-pair-read-ca = { $count ->
    [one] Read { $count } character.
   *[other] Read { $count } characters.
} The { -hub } is at { $address }, its key is { $fingerprint }, and its certificate is checked by this machine's trust store. Compare the fingerprint with the one the { -hub } shows.

# What `pair` read from a token that pins the hub's own certificate, before it asks; nothing is sent yet.
# $count (count): how many characters were read.
# $address (text): the hub's address.
# $fingerprint (text): the hub key's fingerprint.
cli-pair-read-pinned = { $count ->
    [one] Read { $count } character.
   *[other] Read { $count } characters.
} The { -hub } is at { $address }, its key is { $fingerprint }, and its certificate is pinned by the token. Compare the fingerprint with the one the { -hub } shows.

# `pair` asks before it sends anything. Printed without the program's name, and the answer follows on the same line.
# `[y/N]` is what the person types to answer, and only `y` or `Y` says yes: keep it a string literal.
cli-pair-confirm = Pair? { "[y/N]" }

# The person didn't answer y.
cli-pair-not-confirmed = Not paired: nothing was sent.

# Standard input is the token, so nobody can answer the question.
# `--yes` is typed as it is: keep it a string literal.
cli-pair-no-terminal = No terminal to ask on. Compare the { -hub } key above with the { -hub }'s own screen, then run again with { "--yes" }.

# The hub took the pairing and holds the agent pending until a person approves it there.
# $fingerprint (text): this machine's own key's fingerprint, to compare on the hub.
cli-pair-pending = Paired. The { -hub } holds this machine pending until someone approves it there. This machine's key: { $fingerprint }. Compare it with the { -hub }'s screen.
