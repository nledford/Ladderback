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
cli-profile-no-agent-id = cannot read the agent's ID in { $path }: { $detail }

# A command could not write what it had to say, and ended with status 1: the disk is full, say. A reader that has gone is not this case: the command ends quietly with 141.
# $detail (text): the operating system's message. It is shown as it came.
cli-cannot-write-output = cannot write the result: { $detail }

## The service (operations §12): `ladderback install-service` and `ladderback remove-service`.

# What `install-service` does, in its `--help`.
# `install-service` is the command, typed as it is: keep it a string literal.
cli-help-install-service = Install this agent as a service of your user: the service manager runs it in the background, now and from each login on. On Linux that is a systemd user unit, on macOS a launchd agent.

# What `remove-service` does, in its `--help`.
# `install-service` is the command, typed as it is: keep it a string literal.
cli-help-remove-service = Stop the service that { "install-service" } installed, and remove its service file.

# The service file was written and the service manager started the service.
# $file (text): the service file's path.
cli-service-installed = installed the service { $file }: it runs now, and from each login on

# A service was installed already: its file was written again, and the service manager started it again.
# $file (text): the service file's path.
cli-service-replaced = replaced the service { $file } and started it again

# Linux only, after an install: a user's services stop when the user logs out, unless the user lingers.
# `loginctl enable-linger` is a command, typed as it is: keep it a string literal.
cli-service-linger = to keep it running while you are logged out, run { "loginctl enable-linger" }

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
