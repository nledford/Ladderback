### Words and helpers every surface shares, and the messages of the API's error
### and warning codes (technical design §8.4). A code's message is named after
### its name on the wire: `error-<code>` or `warning-<code>`.
###
### What a person types or looks up elsewhere, such as an address, an option or
### a header, is a string literal: `{ "--allowed-host" }`. It is no text of a
### language, so no translation changes it, and a pseudolocale leaves it as it is.

# The product's name. A message that says it uses this term.
-brand-name = Ladderback

## Words of the glossary (technical design §3.1). A message that says one
## uses its term, so a translation says it one way everywhere. A term that
## has more than one form takes `form`: "one" for one of it, "other" for
## more than one, as the sentence means, not as a count.

# The repository's index: the list of what each pack holds.
-index = index

# A rustic snapshot.
-snapshot = { $form ->
   *[one] snapshot
    [other] snapshots
}

# A pack: a file of the repository's data.
-pack = { $form ->
   *[one] pack
    [other] packs
}

# A rustic repository.
-repository = repository

# What lets a program other than a browser, such as MCP connect mode, use the API while sign-in is on.
-api-token = { $form ->
   *[one] API token
    [other] API tokens
}

# Ladderback's own log lines since the daemon started, the newest of them, which the Log page shows.
-recent-log = recent log

# macOS's grant that opens a program's way into Mail, Messages, Safari and other programs' data. Use the name System Settings shows for it in this language.
-full-disk-access = Full Disk Access

# The folders a plan dry run may read, set by a person on the Settings page. A plan dry run counts what a draft plan would back up without backing anything up.
-dry-run-allowlist = dry-run allowlist

# A computer, virtual machine or container running Ladderback, whatever it runs on; the hub is one too (technical design §3.1; §16, decision 133). Not an AI agent, which is a program that acts through MCP or the API.
-machine = { $form ->
   *[one] machine
    [other] machines
}

# The machine that runs with `--hub`: it holds the registry of the machines that pair with it, who owns what, and their history (technical design §3.1).
-hub = hub

## Sizes and durations. The program splits the value and picks the form; the
## catalog arranges the parts, so word order stays with the translator.

# A byte size. The program picks the IEC unit and rounds the number to one decimal.
# $value (number): the size, counted in $unit.
# $unit (text): the unit the program picked: b, kib, mib, gib, tib or pib.
size = { NUMBER($value, maximumFractionDigits: 1) } { $unit ->
    [b] B
    [kib] KiB
    [mib] MiB
    [gib] GiB
    [tib] TiB
   *[pib] PiB
  }

# A length of time, in its two largest units, never rounded up.
# $form (text): which parts follow: ms, s, min-s, h-min or d-h.
# $milliseconds (number): the whole milliseconds, for the ms form.
# $seconds (number): the whole seconds, for the s and min-s forms.
# $minutes (number): the whole minutes, for the min-s and h-min forms.
# $hours (number): the whole hours, for the h-min and d-h forms.
# $days (number): the whole days, for the d-h form.
duration = { $form ->
    [ms] { $milliseconds } ms
    [s] { $seconds } s
    [min-s] { $minutes } min { $seconds } s
    [h-min] { $hours } h { $minutes } min
   *[d-h] { $days } d { $hours } h
  }

## Error codes

# The worker refused the job the daemon gave it.
# $detail (text): why, in the worker's own words. It is shown as it came.
error-invalid-job-spec = The worker refused the job it was given, so nothing ran.
    .with-detail = The worker refused the job it was given, so nothing ran: { $detail }

# The engine reported a failure.
# $detail (text): the engine's own message. It is shown as it came, in English.
error-operation-failed = The operation failed.
    .with-detail = The operation failed: { $detail }

error-deadline-passed = The operation passed its deadline, so { -brand-name } stopped it. Run it again.

# The engine's process could not be started, or refused the job.
# $detail (text): the message of the platform, or of the engine adapter when it refused the job. It is shown as it came.
error-engine-unavailable = { -brand-name } could not run this operation through its engine, so the operation did not run. Either the engine could not be started or it refused the job: check that { -brand-name }'s program file is in place and that the user { -brand-name } runs as may start it, or that the plan is within what the engine accepts.
    .with-detail = { -brand-name } could not run this operation through its engine, so the operation did not run ({ $detail }). Either the engine could not be started or it refused the job: check that { -brand-name }'s program file is in place and that the user { -brand-name } runs as may start it, or that the plan is within what the engine accepts.

# The engine's process stopped without reporting a result.
# $exit_code (text): its exit status. The sender leaves it out when none is known, and the program then supplies the text none.
error-engine-exited = { $exit_code ->
    [none] The engine stopped without reporting a result or an exit status, so the outcome of the operation is unknown. Check the repository, then run the operation again.
   *[other] The engine stopped with exit status { $exit_code } before it reported a result, so the outcome of the operation is unknown. Check the repository, then run the operation again.
  }

# The daemon could not take the engine's report at that point of the operation.
# $transition (text): the step of the operation that was refused, such as report-progress. It is a technical name and is shown as sent.
error-protocol-violated = { -brand-name } could not accept the engine's report (step “{ $transition }”), so it stopped the operation. Run it again, and report this if it happens twice.

# The engine's process did not start the operation in time: it never took its job, or took it and said nothing.
error-engine-start-timed-out = The engine did not start the operation in time, so it never ran. Run it again.

# The engine stopped reporting, but its process stayed alive.
error-engine-exit-timed-out = The engine stopped reporting, so { -brand-name } ended its process. Check the repository, then run the operation again.

# A before-hook whose `on-failure` is `error` failed, so the operation stopped before it began. What the hook wrote is never shown.
# $level (text): whose run-before list holds the hook: agent (this machine's), repository or plan. The sender leaves it out for a failure an earlier version kept, and the program then supplies the text none.
# $position (text): the hook's place in that list, counted from 1. Left out with $level.
# $ended (text): how it ended: exit, signal, timeout or not-started. The sender leaves it out for a failure an earlier version kept without it, and the program then supplies the text none.
# $exit_code (text): its exit status, for an exit; otherwise none.
# `run-before` and `on-failure` are configuration keys and `warn` a value of one, typed as they are: keep them string literals.
error-hook-failed =
    { $level ->
        [agent] { $position ->
            [none] A hook of this { -machine }'s { "run-before" } list failed, so { -brand-name } stopped the operation before it began.
           *[other] Hook { $position } of this { -machine }'s { "run-before" } list failed, so { -brand-name } stopped the operation before it began.
        }
        [repository] { $position ->
            [none] A hook of the repository's { "run-before" } list failed, so { -brand-name } stopped the operation before it began.
           *[other] Hook { $position } of the repository's { "run-before" } list failed, so { -brand-name } stopped the operation before it began.
        }
        [none] A before-hook failed, so { -brand-name } stopped the operation before it began.
       *[plan] { $position ->
            [none] A hook of the plan's { "run-before" } list failed, so { -brand-name } stopped the operation before it began.
           *[other] Hook { $position } of the plan's { "run-before" } list failed, so { -brand-name } stopped the operation before it began.
        }
    } { $ended ->
        [signal] A signal ended it.
        [timeout] It ran past its time limit, or couldn't start within it, and was stopped.
        [not-started] It could not be started.
        [none] It ended with no exit status.
       *[exit] { $exit_code ->
            [none] It exited with no status.
           *[other] It exited with status { $exit_code }.
        }
    } Check the hook's command, or set its { "on-failure" } to { "warn" } to go on when it fails.

# The daemon stopped while the operation was queued or running: a crash, a kill or a power cut.
error-daemon-stopped = { -brand-name } stopped before this operation ended, so the outcome of the operation is unknown. Check the repository, then run the operation again.

# The daemon was told to stop while the operation was running: a service stop, a restart, the computer shutting down. No notification is sent of it.
error-daemon-shut-down = { -brand-name } was shut down while this operation was running, so the operation stopped before it finished. Run the operation again if you still need it.

# A queued backup's turn came while a newer run of its plan was already going, so it gave way to that run and never ran.
error-superseded = A newer run of this plan was already going when this backup's turn came, so this backup didn't run. Nothing was written.

# A queued backup's plan was deleted while the backup waited for its keyring to be unlocked, so it never ran.
error-plan-removed = This backup's plan was deleted while the backup waited for the keyring to be unlocked, so the backup didn't run. Nothing was written.

# A queued backup's plan now backs up to another repository than the one the backup was queued for, so it never ran.
error-plan-moved = This backup's plan now backs up to another { -repository } than the one this backup was queued for, so this backup didn't run. Nothing was written.

# The configuration couldn't be read or used when a queued backup that waited for its keyring was to run, so the backup never ran.
error-config-unusable = { -brand-name } couldn't read or use its configuration file when this backup, which waited for the keyring to be unlocked, was to run, so the backup didn't run. Nothing was written. { -brand-name }'s log says what was wrong with the file: fix it, then run the backup again.

# The request named a host the daemon does not answer for.
# `localhost`, `127.0.0.1` and `--allowed-host` are typed as they are: keep them string literals.
error-host-not-allowed = { -brand-name } refused this request because it names a host that { -brand-name } does not answer for. Open { -brand-name } at { "localhost" } or { "127.0.0.1" }, or allow the host with { "--allowed-host" }.

error-origin-not-allowed = { -brand-name } refused this request because it came from a different site. Open { -brand-name } at its own address and try again.

# A request that changes something was not sent as JSON.
# `Content-Type: application/json` is a header, typed as it is: keep it a string literal.
error-json-required = { -brand-name } refused this request because it was not sent as JSON. Send a request that changes anything with the header { "Content-Type: application/json" }.

# Sign-in is on and the request carries no live session: it was never signed in, or its session ended (unused for 12 hours, 30 days old, signed out, or the admin password was set anew). A program, such as an MCP client, gets it too when it sends no API token.
error-sign-in-required = Sign in to { -brand-name } with the admin password to go on. A program goes on with an { -api-token } instead, such as one { "ladderback mcp" } reads from the file { "--token-file" } names. A session ends after 12 hours unused, after 30 days, and when the admin password is set again.

# The password given at sign-in is not the admin password.
error-sign-in-failed = That is not the admin password. Check it and try again.

# A sign-in came too soon after failed ones from the same address, so the password was not checked.
# $seconds (count): how many seconds are left to wait.
error-sign-in-wait = { $seconds ->
    [one] Wait 1 second before signing in again: sign-ins from this address failed, so the password was not checked.
   *[other] Wait { $seconds } seconds before signing in again: sign-ins from this address failed, so the password was not checked.
  }

# A changed request of a signed-in browser didn't carry its session's token, which keeps other sites from sending requests in its name.
error-csrf-token-invalid = { -brand-name } refused this request because it did not carry the token of its session. Reload the page and try again.

# The admin password could not be checked, or no session could be started, so nobody was signed in.
# $detail (text): the platform's message. It is shown as it came.
error-sign-in-unavailable = { -brand-name } could not check the admin password, so nobody was signed in. Check the admin password file in the data folder, or set the password again with { "ladderback set-password" }.
    .with-detail = { -brand-name } could not check the admin password ({ $detail }), so nobody was signed in. Check the admin password file in the data folder, or set the password again with { "ladderback set-password" }.

# A program's API token is unknown, revoked, of an admin password set since, or has a wrong secret. It counted as a failed sign-in.
error-api-token-invalid = { -brand-name } doesn't know this { -api-token }: it may have been revoked, or the admin password set again. Create a new token in { -brand-name }'s Settings and give it to the program in place of this one; { "ladderback mcp" } reads it from the file { "--token-file" } names.

# A good API token was sent to a route its permissions don't open.
# $permission (text): what the route needs, one of { "backups" }, { "backup-runs" }, { "previews" }, { "configuration" }, { "snapshot-contents" }, { "daemon-log" }, { "repository-check" } or { "session" }.
error-api-token-permission = { $permission ->
    [session] This { -api-token } can't do that: only a browser signed in to { -brand-name } can, never a program with a token.
    [backups] This { -api-token } can't do that: it needs the permission Backups, to see plans, repositories, snapshots, operations and status. Add it to the token in { -brand-name }'s Settings.
    [backup-runs] This { -api-token } can't do that: it needs the permission Backup runs, to start and cancel backups. Add it to the token in { -brand-name }'s Settings.
    [previews] This { -api-token } can't do that: it needs the permission Previews, to see what forget or an import would do. Add it to the token in { -brand-name }'s Settings.
    [configuration] This { -api-token } can't do that: it needs the permission Configuration, to propose configuration changes. Add it to the token in { -brand-name }'s Settings.
    [snapshot-contents] This { -api-token } can't do that: it needs the permission Snapshot contents, to see file and folder names inside snapshots. Add it to the token in { -brand-name }'s Settings.
    [daemon-log] This { -api-token } can't do that: it needs the permission Daemon log, to read { -brand-name }'s log. Add it to the token in { -brand-name }'s Settings.
    [repository-check] This { -api-token } can't do that: it needs the permission Repository check, to start a repository check. Add it to the token in { -brand-name }'s Settings.
   *[other] This { -api-token } can't do that: it needs a permission it doesn't have. Add it to the token in { -brand-name }'s Settings.
  }

# An AI agent without an API token asked for something that what AI agents may do without a token, the configuration's [without-token] table, doesn't allow, or for something only a person's browser may do.
# $permission (text): the permission it needs, one of { "backups" }, { "backup-runs" }, { "previews" }, { "configuration" }, { "snapshot-contents" }, { "daemon-log" } or { "repository-check" }; or { "session" } for what only a browser may do.
error-without-token-permission = { $permission ->
    [session] AI agents can't do that: only a person using { -brand-name } in a browser can.
    [backups] AI agents without a token may not see plans, repositories, snapshots, operations or status here: that needs the permission Backups. A person can allow it in the table { "[without-token]" } of { -brand-name }'s configuration file, or give the agent an { -api-token } that has it.
    [backup-runs] AI agents without a token may not start or cancel backups here: that needs the permission Backup runs. A person can allow it in the table { "[without-token]" } of { -brand-name }'s configuration file, or give the agent an { -api-token } that has it.
    [previews] AI agents without a token may not preview what forget or an import would do here: that needs the permission Previews. A person can allow it in the table { "[without-token]" } of { -brand-name }'s configuration file, or give the agent an { -api-token } that has it.
    [configuration] AI agents without a token may not propose configuration changes here: that needs the permission Configuration. A person can allow it in the table { "[without-token]" } of { -brand-name }'s configuration file, or give the agent an { -api-token } that has it.
    [snapshot-contents] AI agents without a token may not see file and folder names inside snapshots here: that needs the permission Snapshot contents. A person can allow it in the table { "[without-token]" } of { -brand-name }'s configuration file, or give the agent an { -api-token } that has it.
    [daemon-log] AI agents without a token may not read { -brand-name }'s log here: that needs the permission Daemon log. A person can allow it in the table { "[without-token]" } of { -brand-name }'s configuration file, or give the agent an { -api-token } that has it.
    [repository-check] AI agents without a token may not start a repository check here: that needs the permission Repository check. A person can allow it in the table { "[without-token]" } of { -brand-name }'s configuration file, or give the agent an { -api-token } that has it.
   *[other] AI agents without a token may not do that here. A person can allow it in the table { "[without-token]" } of { -brand-name }'s configuration file, or give the agent an { -api-token } that has it.
  }

# The token routes were asked for while no admin password is set.
error-sign-in-off = Sign-in is off, so there are no { -api-token(form: "other") }. To turn sign-in on, run { "ladderback set-password" } and start { -brand-name } again.

# A restart was asked for in Settings, but no service manager runs Ladderback, so nothing would start it again once it stopped.
error-restart-unavailable = { -brand-name } can't restart from here: no service manager runs it, so nothing would start it again. Install it as a service with { "ladderback install-service" }, or stop it and start it again yourself.

# Maintenance was asked for while a restart waits for the running operations to finish; the daemon starts no new operation until it restarts.
error-restart-pending = { -brand-name } is waiting to restart once the running operations finish, so it starts no maintenance now. Run it again once { -brand-name } is back, or cancel the restart in Settings.

# An API token's label is empty, blank, holds a control character, or is too long.
# $max (count): the most characters a label may have.
error-api-token-label = Give the token a label of 1 to { $max } characters, so you can tell it from the others.

# An API token was asked for with no permission.
error-api-token-no-permission = Choose at least one permission for the token: what the program may do.

# As many API tokens as are allowed are kept already.
# $limit (count): the most tokens kept.
error-api-token-limit = { -brand-name } keeps at most { $limit } { -api-token(form: "other") }. Revoke one you no longer use, then create the new one.

# The API token to revoke is not kept, as when it was revoked already.
# $token (text): the token's ID.
error-api-token-unknown = No { -api-token } { $token } is kept: it may have been revoked already. Reload the page to see the tokens kept now.

# The API tokens could not be read or kept.
error-api-tokens-unavailable = { -brand-name } could not read or keep its { -api-token(form: "other") }. Its log says why; check the data folder and try again.

# A new admin password, set in Settings, is shorter than the rule allows, so it was not set.
# $min (count): the fewest characters an admin password may have.
error-admin-password-too-short = { $min ->
    [one] Choose a new password of at least 1 character.
   *[other] Choose a new password of at least { $min } characters.
  }

# A new admin password, set in Settings, is longer than the rule allows, so it was not set.
# $max (count): the most bytes an admin password may hold.
error-admin-password-too-long = { $max ->
    [one] Choose a shorter new password: it may hold at most 1 byte.
   *[other] Choose a shorter new password: it may hold at most { $max } bytes.
  }

# Changing the admin password needs the current one, and what was typed is not it. It counted as a failed sign-in.
error-current-password-wrong = That is not the current admin password. Check it and try again.

# The admin password could not be checked or kept, so it was not set.
error-admin-password-unavailable = { -brand-name } could not check or keep the admin password, so it was not set. Its log says why; check the data folder and try again.

# The database could not keep or forget sessions: a sign-in started none, the session length was not changed, or a sign-out ended its sessions only in the running daemon. Shown wherever that can happen, the sign-in form included, so its last sentence speaks of any session signed out, not of the request that was refused.
error-sessions-unavailable = { -brand-name } could not keep or forget sessions in its database. Its log says why; check the data folder and try again. Any session you signed out has ended all the same, while { -brand-name } runs.

# `ladderback mcp --connect` got no answer from the daemon's API.
error-daemon-unreachable = No { -brand-name } answered at the address { "ladderback mcp" } connects to. Check that the daemon is running and that { "--connect" } names its address.

# `ladderback mcp --connect` sent a request that changes something, and its answer was lost on the way back.
error-daemon-answer-lost = The request was sent to { -brand-name }, but no answer came back, so whether it took effect is unknown. It wasn't sent again: list the operations to see whether it did.

# `ladderback mcp --connect` read an answer from the daemon larger than the most it reads, and left it unread.
# $limit (bytes): the most it reads, as a size such as 8 MiB.
# $operation (text): the ID of the operation whose detail was too large. The sender leaves it out for any other answer, a list of snapshots for one, and the program then supplies the text none.
error-daemon-answer-too-large = { $operation ->
    [none] { -brand-name } answered with more than { $limit }, the most { "ladderback mcp" } reads, so the answer was left unread.
   *[other] { -brand-name } answered with more than { $limit }, the most { "ladderback mcp" } reads, so operation { $operation } was left unread. An operation with that many warnings can be read in the web UI.
  }

# The configuration file could not be read or written.
# $file (text): the path of the configuration file. It is shown as it is.
# $detail (text): the platform's message. It is shown as it came.
error-config-unavailable = { -brand-name } could not read or write its configuration file { $file }. Check that the file and its folder exist and that the user { -brand-name } runs as may read and write them, then try again.
    .with-detail = { -brand-name } could not read or write its configuration file { $file } ({ $detail }). Check that the file and its folder exist and that the user { -brand-name } runs as may read and write them, then try again.

# The configuration file is larger than any configuration needs, so it was not read.
# $file (text): the path of the configuration file. It is shown as it is.
# $limit (bytes): the largest size { -brand-name } reads, as a size such as 1 MiB.
error-config-too-large = { -brand-name } did not read its configuration file { $file }, because it is larger than { $limit }. A configuration is far smaller, so check that the file is the right one, and cut it down.

# The configuration file is not a valid configuration.
# $file (text): the path of the configuration file. It is shown as it is.
# $line (text): the line of the file where the problem was found. The sender leaves it out when none is known, and the program then supplies the text none.
# $detail (text): which setting is wrong and why. It is shown as it came, in English.
error-config-invalid = { $line ->
    [none] { -brand-name } did not accept its configuration file { $file }. Correct the file, then try again.
   *[other] { -brand-name } did not accept its configuration file { $file }: the problem is at line { $line }. Correct the file, then try again.
  }
    .with-detail = { $line ->
        [none] { -brand-name } did not accept its configuration file { $file } ({ $detail }). Correct the file, then try again.
       *[other] { -brand-name } did not accept its configuration file { $file } (line { $line }: { $detail }). Correct the file, then try again.
      }

# The configuration file does not say which version of the format it is in.
# $file (text): the path of the configuration file. It is shown as it is.
error-config-schema-missing = { -brand-name } did not read its configuration file { $file }, because the file does not say which version of the format it is in. Add the schema version the file was written for, or restore the file from a backup.

# The configuration file is in a newer version of the format than this build reads.
# $file (text): the path of the configuration file. It is shown as it is.
# $found (text): the version of the format the file is in.
# $supported (text): the newest version of the format this version of { -brand-name } reads.
error-config-schema-newer = { -brand-name } did not read its configuration file { $file }, because the file is in version { $found } of the format and this version of { -brand-name } reads up to version { $supported }. Update { -brand-name }, or restore an older copy of the file.

# An error this version of the program has no message for, such as one from a machine running a newer version.
# $detail (text): the sender's own message. It is shown as it came.
error-unknown = An unexpected error occurred.
    .with-detail = An unexpected error occurred: { $detail }

# The request names a plan the configuration does not have.
# $plan (text): the plan's ID, such as home.
error-plan-unknown = There is no plan { $plan }. Add it to the configuration, or choose a plan that exists.

# A backup of a plan was started by hand while a backup of the same plan was already waiting for its turn, so no other was queued. The waiting one backs up the files as they are when it starts.
# $plan (text): the plan's ID, such as home.
error-plan-run-waiting = A backup of plan { $plan } is already waiting for its turn, so no other was started. It will back up the files as they are when it starts.

# A plan sent from the Plans page is not one the configuration file would load, so nothing was saved.
# $field (text): the plan's setting at fault, as its key in the configuration file, such as schedule.cron, id, repository or sources.
# $problem (text): what is wrong with it, one of { "missing" }, { "unknown-key" }, { "wrong-type" }, { "invalid" }, { "taken" }, { "unknown-repository" } or { "no-sources" }.
error-plan-invalid = { $problem ->
    [missing] The plan needs its { $field }, so it was not saved. Fill it in, then save again.
    [unknown-key] A plan has no setting { $field }, so it was not saved. Remove it, then save again.
    [wrong-type] The plan's { $field } is not the kind of value that setting takes, so the plan was not saved. Correct it, then save again.
    [taken] Another plan already has this { $field }, so the plan was not saved. Choose another, then save again.
    [unknown-repository] The plan's { $field } is not a { -repository } { -brand-name } has, so the plan was not saved. Choose one that exists, then save again.
    [no-sources] The plan's { $field } are empty, so it would back up nothing and was not saved. Add a folder or a file to back up, then save again.
   *[invalid] The plan's { $field } is not a value that setting takes, so the plan was not saved. Correct it, then save again.
  }

# A plan was not deleted, because a notification rule names it: the configuration would no longer load.
# $plan (text): the plan's ID, such as home.
# $rule (text): the notification rule's name, such as failures.
error-plan-in-use = The notification rule { $rule } names the plan { $plan }, so the plan was not deleted. Remove the plan from that rule, then delete it again.

# A backup, or a plan dry run, was not started: the plan's sources hold the data folder, which holds a secret key file, and the folder's path is not valid UTF-8, so the key files can't be left out of it.
error-key-files-not-left-out = This was not started: the plan's backup would take the secret key files in the data folder, and { -brand-name } can't leave them out because the folder's path isn't valid UTF-8. Move the data folder to a path in UTF-8, or take the folder that holds it out of the plan's sources.

# A proposal of configuration changes holds no change, so nothing was kept.
error-proposal-empty = The proposal holds no change, so it was not kept. Propose at least one change.

# A change of a proposal is not one the configuration takes, so nothing was kept.
# $change (count): the change's place in the proposal, from 1.
# $field (text): the plan's setting at fault, as its key in the configuration file, such as id, repository or sources.
# $problem (text): what is wrong with it, one of { "missing" }, { "unknown-key" }, { "wrong-type" }, { "invalid" }, { "taken" }, { "unknown-repository" }, { "no-sources" }, { "unknown-plan" }, { "in-use" } or { "hooks" }.
error-proposal-invalid = { $problem ->
    [unknown-plan] Change { $change } of the proposal names a plan the configuration doesn't have, so the proposal was not kept. Correct it, then propose again.
    [in-use] Change { $change } of the proposal removes a plan a notification rule names, so the proposal was not kept. Correct it, then propose again.
    [hooks] Change { $change } of the proposal gives a plan hooks it doesn't have, so the proposal was not kept. A proposal can't add, change or remove hooks yet: propose the plan with the hooks it has, or none for a new plan.
   *[other] The { $field } of change { $change } of the proposal is not one the configuration takes, so the proposal was not kept. Correct it, then propose again.
  }

# A proposal is larger than a proposal may be, so nothing was kept.
# $what (text): what is over its limit, { "changes" } or { "rationale" }.
# $limit (count): how many changes, or characters of rationale, a proposal may hold.
error-proposal-too-large = { $what ->
    [rationale] The proposal's rationale is longer than { $limit } characters, so the proposal was not kept. Shorten it, then propose again.
   *[other] The proposal holds more than { $limit } changes, so it was not kept. Split it, then propose again.
  }

# The proposer has as many proposals pending as it may.
# $limit (count): how many proposals one proposer may have pending.
error-proposal-limit = { $limit } proposals are pending already, so this one was not kept. Withdraw one, or wait until one expires, then propose again.

# The request names a proposal the asker can't see, or that isn't kept.
# $proposal (text): the proposal's ID.
error-proposal-unknown = There is no proposal { $proposal }. Choose a proposal from the list of proposals.

# A proposal that is not pending was asked to be withdrawn, approved or rejected.
# $status (text): the proposal's status, one of { "applying" }, { "applied" }, { "failed" }, { "rejected" }, { "withdrawn" }, { "expired" } or { "stale" }.
error-proposal-not-pending = { $status ->
    [applying] The proposal is being applied, so nothing more was done to it.
    [applied] The proposal has been applied, so nothing more was done to it.
    [failed] The proposal failed to apply, so nothing more was done to it.
    [rejected] The proposal was rejected, so nothing more was done to it.
    [withdrawn] The proposal was withdrawn, so nothing more was done to it.
    [expired] The proposal has expired, so nothing more was done to it.
   *[other] The proposal has gone stale, as a change of it no longer applies, so nothing more was done to it.
  }

# The configuration file changed since the page showed the proposal, so nothing was applied.
error-proposal-changed = The configuration file changed since this proposal was shown, so nothing was applied. Review the proposal again, then approve it if it still looks right.

# The proposal holds a change that could delete snapshots, which is approved only with the forget preview the person saw, and the approval brought none.
error-proposal-needs-forget-preview = This proposal holds a change that could delete snapshots, so it was not approved. Show what it would forget first, then approve it from that preview if it still looks right.

# What the proposal would forget, read again as it was approved, isn't what the forget preview showed, or a repository it touches couldn't be read; nothing was applied.
error-proposal-forget-preview-changed = What this proposal would forget has changed since you previewed it, or a repository it touches couldn't be read again, so nothing was applied. Show what it would forget again, then approve it if it still looks right.

# The proposer made as many proposals in the last hour as it may.
# $seconds (count): how long until it may propose again.
error-proposal-rate = { $seconds ->
    [one] This proposer made as many proposals in the last hour as it may, so this one was not kept. Propose again in 1 second.
   *[other] This proposer made as many proposals in the last hour as it may, so this one was not kept. Propose again in { $seconds } seconds.
  }

# A person rejected one of the proposer's proposals a short while ago.
# $seconds (count): how long until it may propose again.
error-proposal-after-rejection = { $seconds ->
    [one] A person rejected one of this proposer's proposals a moment ago, so this one was not kept. Read the reasons given, then propose again in 1 second.
   *[other] A person rejected one of this proposer's proposals a moment ago, so this one was not kept. Read the reasons given, then propose again in { $seconds } seconds.
  }

# A rejection's reasons don't fit its proposal.
# $problem (text): { "count" } when the number of reasons isn't the number of changes, { "too-long" } when one reason is too long.
# $change (count): with { "count" }, how many changes the proposal has; with { "too-long" }, the change whose reason is too long, from 1.
error-proposal-reasons = { $problem ->
    [count] { $change ->
        [one] Give one reason for the proposal's one change, even an empty one, then reject it again.
       *[other] Give one reason for each of the proposal's { $change } changes, even an empty one, then reject it again.
      }
   *[other] The reason for change { $change } is longer than 1000 characters. Shorten it, then reject the proposal again.
  }

# The proposals could not be read or kept; the daemon's log says why.
error-proposals-unavailable = { -brand-name } could not read or keep the proposals. Its log says why; check the data folder and try again.

# Another plan dry run is running; dry runs run one at a time.
error-dry-run-busy = Another plan dry run is running, and they run one at a time. Try again when it has finished.

# A backup is running or waiting, so the plan dry run did not start.
error-dry-run-gives-way = A backup is running or waiting, and a plan dry run gives way to backups. Try again when the backups have finished.

# The proposer started as many plan dry runs in the last hour as it may.
# $seconds (count): how long until it may start another.
error-dry-run-rate = { $seconds ->
    [one] This client started as many plan dry runs in the last hour as it may. Try again in 1 second.
   *[other] This client started as many plan dry runs in the last hour as it may. Try again in { $seconds } seconds.
  }

# A path the draft plan reads is outside the dry-run allowlist, or could not be resolved, or a marker it names is not a file name alone; the message never says which.
# $setting (text): the plan's setting, as its key in the configuration file: { "sources" }, { "glob-files" }, { "iglob-files" } or { "exclude-if-present" }.
# $entry (count): the entry of that setting, from 1.
error-dry-run-not-allowed = Entry { $entry } of the plan's { $setting } would read outside the { -dry-run-allowlist }, so nothing was read. Give a path within it, and a marker by its name alone, or ask a person to add the folder on the Settings page.

# For this setting of the draft plan, rustic would read files in the folders above the plan's sources, outside the dry-run allowlist, so the dry run read nothing.
# $setting (text): the plan's setting, as its key in the configuration file: { "custom-ignorefiles" }, { "git-ignore" } or { "no-require-git" }.
error-dry-run-reads-above = For the plan's { $setting }, rustic would read files in the folders above its sources, outside the { -dry-run-allowlist }, so nothing was read. Leave { $setting } out of the draft to dry-run the rest.

# The engine cannot apply the draft plan's source filter, as a backup could not.
error-dry-run-filter = The plan's source filter can't be applied, so the plan dry run did not run. Correct its excludes, then try again.

# The walk of the plan dry run failed; the daemon's log says why.
error-dry-run-failed = The plan dry run failed. { -brand-name }'s log says why.

# A folder of the dry-run allowlist is not an absolute path, so the list was not saved.
# $entry (count): the folder's place in the list, from 1.
error-dry-run-folder-invalid = Folder { $entry } of the { -dry-run-allowlist } is not an absolute path, so the list was not saved. Give the full path, then save again.

# The dry-run allowlist holds more folders than it may, so it was not saved.
# $limit (count): how many folders it may hold.
error-dry-run-allowlist-too-long = The { -dry-run-allowlist } may hold at most { $limit } folders, so it was not saved. Remove some, then save again.

# The request names a repository the configuration does not have.
# $alias (text): the repository's alias, such as demo.
error-repository-unknown = There is no repository { $alias }. Add it to the configuration, or choose a repository that exists.

# The repository holds no snapshot by the ID the request named.
# $snapshot (text): the snapshot's ID, 64 hexadecimal digits.
error-snapshot-unknown = The { -repository } has no { -snapshot } { $snapshot }. It may have been forgotten; open a { -snapshot } the list still shows.

# The snapshot holds no folder at the path the request named.
# $path (text): the folder's path in the snapshot, its names joined by "/", such as source/docs.
error-folder-unknown = The { -snapshot } has no folder { $path }. Open a folder from the { -snapshot }'s tree.

# A request's body was not the JSON the route takes.
error-invalid-body = { -brand-name } could not read the request's body. Send the JSON the route takes.

# A request's body was larger than the most its route takes, so nothing ran.
# $limit (bytes): the most the route takes, as a size such as 128 KiB.
error-body-too-large = { -brand-name } did not read this request, because its body is larger than { $limit }. Shorten the body or leave it out, then try again.

# The request names an operation the log has none of.
# $operation (text): the operation's ID.
error-operation-unknown = There is no operation { $operation }. List the operations again to find the one you mean, or run a new one.

# The request asked to cancel an operation that has already ended.
# $operation (text): the operation's ID.
error-operation-not-running = Operation { $operation } has already ended, so there is nothing to cancel.


# A maintenance operation is running on this machine, so the request was
# refused and nothing was done. For a maintenance run it is this
# repository's; for turning maintenance off it may be another repository's.
# $operation (text): the running operation's ID.
error-maintenance-busy = Maintenance operation { $operation } is running on this { -machine }, so nothing was done. Wait for it to end, then ask again.

# The confirmation a restore asks for first was not right: the admin
# password with sign-in on, the snapshot's short ID with sign-in off.
# Nothing was restored. A wrong password counts as a failed sign-in.
error-step-up-failed = That confirmation is not right, so nothing was restored. Type the admin password, or, with sign-in off, the { -snapshot }'s short ID the dialog shows.

# A restore could not write where it was to write: the restore folder, or a
# path it restores in place, refused it. Whatever it wrote before stays.
error-restore-target-unwritable = The restore could not write where it was to write, so it stopped. Make the restore folder, or the original files' folder, writable for the user { -brand-name } runs as, then restore again.

# Another restore already runs on this machine, which runs one at a time, so
# the request was refused and nothing ran.
# $operation (text): the running restore's operation ID.
error-restore-running = Restore { $operation } is already running, and this { -machine } runs one restore at a time. Wait for it to end, then restore again.

# A restore over the original files was asked for from elsewhere than this
# machine's own web UI, the only place it is offered, so nothing ran.
error-restore-in-place-not-here = A restore over the original files can only be started in this { -machine }'s own web UI, so nothing was restored. Restore into a new folder instead, or open the { -machine }'s web UI.

# A restore was asked for through the hub, and this machine doesn't allow
# restores from the hub, so nothing ran.
error-restore-hub-not-allowed = This { -machine } doesn't take restores from the { -hub }, so nothing was restored. Set { "allow-hub-restore = true" } in the { -machine }'s configuration file to allow them.

# A restore into a new folder was asked for, and this machine has no folder
# to make it in: it has no home folder, and its configuration file doesn't
# set restore-folder, so nothing ran.
error-restore-folder-unset = This { -machine } has no home folder, so there is no folder to restore into and nothing was restored. Set { "restore-folder" } in the { -machine }'s configuration file to a folder for restores, then try again.

# A browser asked the machine to open System Settings at the Full Disk Access pane, and the browser isn't on the machine's own Mac, so nothing opened.
error-full-disk-access-not-local = { -brand-name } opens System Settings only for a browser on the Mac it runs on, so nothing opened. On that Mac, open System Settings, choose Privacy & Security, then { -full-disk-access }, and turn it on for { -brand-name }.

# The machine was asked to open System Settings at the Full Disk Access pane, and couldn't: it isn't on macOS, or macOS refused.
error-full-disk-access-settings-unopened = { -brand-name } could not open System Settings. Open it yourself, choose Privacy & Security, then { -full-disk-access }, and turn it on for { -brand-name }.

# Two downloads are already under way on this machine, confirmed or being
# handed over, and a machine hands over two at a time, so nothing ran. The
# third is refused rather than queued.
error-downloads-busy = Two downloads are already under way on this { -machine }, and it hands over two at a time, so nothing was downloaded. Wait for one to finish, then download again.

# The machine's system gave no random bytes for a download's ticket, so the
# download wasn't confirmed and nothing ran. The daemon's log says why.
error-downloads-unavailable = { -brand-name } could not get the download ready, so nothing was downloaded. Its log says why; try again, and if it happens again, check the computer it runs on.

# The snapshot holds no file or folder at the path a download named.
# $path (text): the entry's path in the snapshot, its names joined by "/", such as source/docs/notes.txt.
error-entry-unknown = The { -snapshot } has no file or folder { $path }, so nothing was downloaded. Choose a file or folder from the { -snapshot }'s tree.

# No download waits under the ticket the browser asked for: it was fetched
# already, its minute ran out, or it never existed.
error-download-unknown = This download is no longer waiting: it was already fetched, or a minute passed before it was. Download it again.

# No route of the daemon's API serves the path a program asked for: a mistyped
# path, or a program newer or older than the daemon. Nothing was done.
error-route-unknown = This version of { -brand-name } has no such route, so nothing was done. Check the path the program asked for; if the program is newer or older than this { -brand-name }, update the older one.

# A fault of Ladderback's own stopped it while it answered: a handler of the
# daemon panicked. What was asked may be done in part; the daemon goes on.
error-internal-error = { -brand-name } stopped answering through a fault of its own, so what you asked may be done only in part. Look at the page again before you try once more, and report the fault if it comes back.

# The pairing token was already spent. The hub answers the same machine with
# the same key as before, so a pairing whose answer was lost can be repeated.
# `pair` is typed as it is: keep it a string literal.
error-token-used = This token was already used. If this { -machine } paired a moment ago and the reply was lost, run { "pair" } again with the same token: the { -hub } recognizes the same { -machine } and key and answers as before. If it says this again, another { -machine } may have used the token first: look at the list of { -machine(form: "other") } on the { -hub }, reject any { -machine } you don't know, then make a new token.

# The pairing token lapsed before anyone used it.
error-token-expired = This token lapsed. Make a new one on the { -hub }.

# Someone canceled the pairing token on the hub.
error-token-revoked = This token was canceled on the { -hub }. Make a new one.

# The text isn't a whole pairing token: cut short, or changed by a wrapped
# line or a chat app.
error-token-malformed = That isn't a whole token. Copy all of it again: a wrapped line or a chat app may have cut or changed it.

# The pairing token was made by a newer hub than this machine's version of Ladderback can read.
error-token-unsupported = This token was made by a newer { -brand-name } than this { -machine } runs. Update { -brand-name } here, then pair.

# Another machine of the hub already has this machine's hostname. The token
# stays good.
# $hostname (text): the hostname the other machine registered, as it was given.
# $last_seen (text): when the hub last heard from it, such as "3 days ago".
# `--hostname` is typed as it is: keep it a string literal.
error-hostname-taken = The { -hub } already has a { -machine } named { $hostname } (last seen { $last_seen }). If this is a different { -machine }, pair under another name: add { "--hostname" }. If it is the same { -machine } and its data folder was lost, don't rename it: remove the old { -machine } on the { -hub }, then pair with this same token.

# The hub has another machine under this machine's ID.
error-agent-key-mismatch = The { -hub } has another { -machine } under this { -machine }'s ID. Don't pair again; ask whoever runs the { -hub }.

# The hub's key isn't the one this machine trusts or its token names. Nothing
# of the token was sent.
error-hub-key-mismatch = This { -hub }'s key isn't the one this { -machine } trusts. If you reset or reinstalled the { -hub } on purpose, make a new token on the { -hub } and pair again, and compare the { -hub } key's fingerprint on both screens first. If you didn't, don't pair again: someone may be posing as it.

# The hub didn't prove it holds its key on this connection. Nothing of the
# token was sent.
error-hub-proof-invalid = The { -hub } didn't prove its key. If you reset or reinstalled the { -hub } on purpose, make a new token on the { -hub } and pair again, and compare the { -hub } key's fingerprint on both screens first. If you didn't, don't pair again: someone may be posing as it.

# The hub refused this machine's proof. The hub gives one answer for every
# cause, so the message names them all.
error-proof-refused = The { -hub } refused this { -machine }. It may have been removed, or its key may have changed. Look at the list of { -machine(form: "other") } on the { -hub }; to bring it back, make a replacement token for it. If this { -machine } was never approved, its wait may have lapsed: make a new token and pair again.

# The hub already has as many machines waiting for approval as it allows. The
# token stays good.
error-pending-limit = The { -hub } already has 20 { -machine(form: "other") } waiting for approval. Approve or reject some, then pair again with this token.

# The hub closed the connection before proving itself: it can't offer the
# kind of connection this machine's token names, as when a proxy ends TLS in
# front of it. This build makes only tokens of CA-checked addresses, so the
# way on is a token for the address of a proxy that serves https with a
# certificate an authority signed.
error-binding-unavailable = Not connected: the { -hub } closed the connection because it can't offer this machine's kind of connection. Make a new token for the { -hub }'s https address, served by a proxy with a certificate that a certificate authority signed, and pair again with it. Update the { -hub } if it is old.

# The hub refused this machine because it already takes another computer
# under this machine's ID, as when a data folder was copied from one computer
# to another. Backups still run here, and their reports wait on this machine.
error-identity-conflict = The { -hub } refused this computer: another computer already connects to it as this { -machine }, as when a data folder is copied. Backups still run here, and their reports wait. If this is a new computer, take a new machine ID from the start page, restart { -brand-name } and pair it again.

# No hub answered at the token's address: the connection was refused, the
# name didn't resolve, or nothing answered in 10 seconds. Nothing was sent.
# $address (text): the hub's address, as the token or the command names it.
# $detail (text): the system's own words for the failure.
# `ladderback pair <address>` is typed as it is: keep it a string literal.
error-hub-unreachable = This machine couldn't reach the { -hub } at { $address } (connection refused, or no answer in 10 seconds). Check the address and that the { -hub } is running. If the address is wrong, give the right one: { "ladderback pair <address>" }.
    .with-detail = This machine couldn't reach the { -hub } at { $address } ({ $detail }). Check the address and that the { -hub } is running. If the address is wrong, give the right one: { "ladderback pair <address>" }.

# The hub's certificate at a pinned address isn't the one its token names.
# Nothing was sent.
# $address (text): the hub's address.
error-hub-pin-mismatch = The { -hub } at { $address } showed a certificate that isn't the one its token names. Don't pair. Check the address with whoever runs the { -hub }; if they replaced the { -hub }'s certificate key, make a new token and compare the fingerprint.

# The hub's certificate at an https address isn't one this machine's trust
# store accepts for the address's name. Nothing was sent.
# $address (text): the hub's address.
# $detail (text): the TLS library's own words for the refusal.
error-hub-untrusted = The { -hub } at { $address } showed a certificate this machine doesn't trust for that name. Don't pair. Check the address with whoever runs the { -hub }: the { -hub } or the proxy in front of it has to serve a certificate that a certificate authority this machine trusts signed for that name.
    .with-detail = The { -hub } at { $address } showed a certificate this machine doesn't trust for that name ({ $detail }). Don't pair. Check the address with whoever runs the { -hub }: the { -hub } or the proxy in front of it has to serve a certificate that a certificate authority this machine trusts signed for that name.

# The TLS handshake with the hub failed on its version or its key exchange.
# Nothing was sent.
# $address (text): the hub's address.
# $detail (text): the TLS library's own words for the failure.
error-hub-tls-refused = The { -hub } at { $address }, or a proxy in front of it, doesn't speak TLS 1.3, or, for a pinned address, the hybrid key exchange X25519MLKEM768 this machine needs. Update the proxy or the { -hub }. Don't pair over anything lower.
    .with-detail = The { -hub } at { $address }, or a proxy in front of it, doesn't speak TLS 1.3, or, for a pinned address, the hybrid key exchange X25519MLKEM768 this machine needs ({ $detail }). Update the proxy or the { -hub }. Don't pair over anything lower.

# The token's address is plain http:// and not this computer. Nothing was
# sent.
# `http://`, `https://` and `ladderback pair https://…` are typed as they
# are: keep them string literals.
error-plain-address = This address starts with { "http://" }, which shows the token to the network. This machine pairs only over { "https://" } or to localhost. Use the { -hub }'s https address ({ "ladderback pair https://…" }), served by a proxy with a certificate that a certificate authority signed.

# The connection to the hub ended before the hub answered the pairing, or
# the hub sent what isn't the next message of the handshake.
error-hub-connection-lost = The connection to the { -hub } ended before the pairing was answered. Check that the { -hub } is running, then pair again with the same token: a token is used only once the { -hub } answers.

# The connection to the hub ended after this machine paired, or the hub
# sent what isn't the next message, or one it didn't sign on this
# connection. Nothing in it was acted on.
# $detail (text): the code of the hub's signed refusal, such as
# hostname-taken, shown as it came so it can be reported.
error-hub-link-lost = The connection to the { -hub } ended, or the { -hub } sent something this machine can't trust, so nothing it sent was acted on. This machine keeps backing up. Check that the { -hub } is running, then try again.
    .with-detail = The connection to the { -hub } ended, or the { -hub } sent something this machine can't trust, so nothing it sent was acted on. This machine keeps backing up. Check that the { -hub } is running, then try again. The { -hub } said: { $detail }

# The hub offered a lower generation than this machine has already seen.
# `ladderback hub-recover` is a command: keep it a string literal.
error-generation-lower = The { -hub } offered an older state than this machine has already seen, so it ignored it. It keeps backing up. If you restored the { -hub } from a backup, or moved it, ask whoever runs it. If you didn't, the { -hub }'s key may be leaked: on the { -hub }'s machine run { "ladderback hub-recover" } with the recovery kit. A leaked key always needs a recovery or a reset of the { -hub } key.

# The hub and this machine share no protocol: one is on an older release
# than the other can use.
error-protocol-out-of-date = The { -hub } is on an older release than this machine can use, or a newer one. This machine keeps backing up. Update whichever side is older.

# A pairing token or an approval was asked for on the hub's page from
# another computer while sign-in is off, so there is no password to confirm
# it with.
error-pairing-needs-sign-in = Pairing a machine from another computer needs sign-in to be on, so the { -hub } did nothing. Set an admin password under Settings, start { -brand-name } again, then sign in and try again.

# The address for a pairing token isn't one a machine can use.
error-hub-address-invalid = That address isn't one a { -machine } can use. Type it as { "https://" }, then the host name and the port, such as { "https://hub.example:9443" }.

# A pairing token's life outside what the hub allows was asked for.
error-pairing-token-life = A pairing token lives from 5 minutes to 24 hours. Choose a life in that range.

# As many pairing tokens as the hub keeps are unused already.
error-pairing-token-limit = The { -hub } has as many unused pairing tokens as it keeps. Wait until one lapses, then make the new one.

# No unused pairing token has the ID asked for.
error-pairing-token-unknown = That pairing token isn't waiting any more: it was used, it lapsed or it was canceled already.

# No machine waits for approval under the agent ID and the key asked for.
error-agent-unknown = No { -machine } with that key is waiting for approval: it never paired, it was dropped, or it holds another key now. Read the list again and compare the key with the one on the { -machine }'s own screen.

# The machine asked to be approved is approved already.
error-agent-approved-already = That { -machine } is approved already.

# The hub couldn't read or keep its key or its tokens.
error-hub-unavailable = The { -hub } couldn't read or keep its key or its pairing tokens. Its log says why.

# This machine doesn't maintain the repository: its entry doesn't ask this
# machine to maintain it, so a forget, a prune, a check or an adoption, or the
# preview of one, was refused and nothing in the repository was changed. The
# refusal can come after the repository was read, when maintenance from this
# machine was turned off meanwhile.
# $alias (text): the repository's alias, such as demo.
error-maintenance-not-owner = This { -machine } does not maintain the repository { $alias }, so nothing in it was changed. To maintain it from this { -machine }, turn its maintenance on from the Snapshots page.

# The plan's retention policy would keep nothing, so the forget was refused
# before anything was removed.
# $plan (text): the plan's ID, such as home.
error-retention-keeps-nothing = The policy of plan { $plan } would keep none of its snapshots, so nothing was removed. Give the policy something to keep, or delete the snapshots you mean to delete.

# The group an adoption named matches no plan of this machine, so nothing
# was written.
error-adoption-no-match = No plan of this { -machine } matches that history, so nothing was written. Point the adoption at the plan whose host, label and sources it carries.

# More than one plan matches the group an adoption named, so nothing was
# written.
# $plans (text): the plans that match, such as home, mirror.
error-adoption-ambiguous = Plans { $plans } all match that history, and which one it belongs to is yours to say. Point the adoption at one plan, or change the others so only one matches.

# A forget or an adoption was started with the selection its preview named,
# and the snapshots it would act on have changed since, so nothing was done.
error-maintenance-selection-changed = The repository changed after you reviewed this, so nothing was done. Review it again to see what it would do now.

# The data folder was copied from another computer (the identifier the
# system gives this one isn't the one recorded with the machine ID), or a new
# machine ID was taken that is used from the next start, so this machine
# runs no forget, prune or adoption. Backups and checks still run.
error-maintenance-data-folder-copied = This computer's data folder came from another computer, so it doesn't forget, prune or adopt snapshots: it could remove the other computer's. Backups and checks still run. If this is a new computer, take a new machine ID from the start page and restart { -brand-name }.

# A new machine ID was asked for, and the data folder wasn't copied from
# another computer, or a new ID was taken for it already, so nothing changed.
error-machine-not-copied = No new machine ID was taken: this computer's data folder wasn't copied from another computer, or it took a new ID already. Nothing changed.

# The repository did not answer a read in time, so there was nothing to show
# or to review.
error-repository-read-timed-out = The repository did not answer in time, so what was asked of it could not be read. Check that it can be reached, then try again.

# The request asks for a maintenance kind this version does not run.
# $kind (text): the kind as it was sent.
error-maintenance-kind-unknown = { -brand-name } does not run maintenance of kind { $kind }. Ask for forget, adopt, prune or check.

# A list's cursor (the history's, or an MCP tool's) names no item of the list: it was never one, or the item it named has gone.
# $cursor (text): the cursor as it was sent.
error-invalid-cursor = { -brand-name } could not continue the list from { $cursor }: no item of it has that cursor now. Ask for the list again from its start.

# A call of an MCP tool gave an argument the tool does not take, left out one it needs, or gave one a value it does not take, so the tool did not run.
# $field (text): the argument's name, as the tool's input schema or the call names it, such as plan or timeout.
# $problem (text): what is wrong with it, one of { "missing" }, { "unknown-key" } or { "invalid" }.
error-invalid-arguments = { $problem ->
    [missing] The tool needs the argument { $field }, so it did not run. Add it, then call the tool again.
    [unknown-key] The tool takes no argument { $field }, so it did not run. Leave it out, then call the tool again.
   *[invalid] The tool does not take the value given for its argument { $field }, so it did not run. Give a value that its description and input schema allow, then call the tool again.
  }

## Warning codes

# A file or folder the backup could not read.
# $detail (text): rustic's message about why it could not be read. It is shown as it came.
warning-source-unreadable = { -brand-name } could not read this item, so it was left out of the snapshot. Check that the user { -brand-name } runs as can read it, or exclude it from the plan.
    .with-detail = { -brand-name } could not read this item, so it was left out of the snapshot ({ $detail }). Check that the user { -brand-name } runs as can read it, or exclude it from the plan.

# On macOS, a file or folder the backup could not read because Ladderback hasn't been granted Full Disk Access.
# $detail (text): rustic's message about why it could not be read. It is shown as it came.
warning-full-disk-access-missing = { -brand-name } could not read this item because macOS hasn't given it { -full-disk-access }, so it was left out of the snapshot. Turn on { -full-disk-access } for { -brand-name } in System Settings, under Privacy & Security; the start page shows how.
    .with-detail = { -brand-name } could not read this item because macOS hasn't given it { -full-disk-access } ({ $detail }), so it was left out of the snapshot. Turn on { -full-disk-access } for { -brand-name } in System Settings, under Privacy & Security; the start page shows how.

# On Linux, the Repositories page notes that the keyring service (the Secret Service) runs now, but that the session bus has no activation file for it, so nothing can start it when a program asks for it.
# `org.freedesktop.secrets.service` is the activation file's name, typed as it is: keep it a string literal.
warning-keyring-not-activatable = Your keyring service is running, but nothing can start it when a program asks for it, so a backup that starts as you log in, before the desktop starts the keyring, may wait for it and then fail. Repair the keyring's D-Bus activation file, { "org.freedesktop.secrets.service" }, or keep { -repository } passwords in a file or a systemd credential instead.

# A secret was read from a file other users of this machine can read.
# $path (text): the file's path.
# $mode (text): the file's permissions in octal, such as 0644.
# The command is typed as it is: keep it a string literal.
warning-secret-file-readable = Other users of this machine can read the secret file { $path } (permissions { $mode }). Run { "chmod 600" } on it so that only its owner can read it.

# A secret was read from a file other users of this machine can change, so they can choose the secret it holds.
# $path (text): the file's path.
# $mode (text): the file's permissions in octal, such as 0620.
# The command is typed as it is: keep it a string literal.
warning-secret-file-writable = Other users of this machine can change the secret file { $path } (permissions { $mode }), so they can choose the secret { -brand-name } reads from it. Run { "chmod 600" } on it so that only its owner can read or change it.

# A hook command around the operation failed where its `on-failure` said to go on.
# $phase (text): which hook list it ran from: before, after, failed or finally.
# $exit_code (text): the hook's exit status, or `none` when it timed out or could not be started.
warning-hook-warning = { $exit_code ->
    [none] A { $phase } hook command around the operation timed out or could not be started, so { -brand-name } went on without it. Check the hook's command, or set its `on-failure` to `error` to stop the operation when it fails.
   *[other] A { $phase } hook command around the operation failed with exit status { $exit_code }, so { -brand-name } went on without it. Check the hook's command, or set its `on-failure` to `error` to stop the operation when it fails.
  }
    .with-detail = { $exit_code ->
    [none] A { $phase } hook command around the operation timed out or could not be started, so { -brand-name } went on without it: { $detail }. Check the hook's command, or set its `on-failure` to `error` to stop the operation when it fails.
   *[other] A { $phase } hook command around the operation failed with exit status { $exit_code }, so { -brand-name } went on without it: { $detail }. Check the hook's command, or set its `on-failure` to `error` to stop the operation when it fails.
  }

# A forget or an adoption ran as several jobs and ended before the last. The jobs that finished stay done.
# $what (text): which one it was: forget or adopt.
# $done (count): how many snapshots the finished jobs handled.
# $total (count): how many snapshots the whole run was to handle.
warning-maintenance-partial = { $what ->
    [adopt] The adoption stopped after { $done } of { $total } snapshots. Those stay adopted. Adopt again to take in the rest.
   *[other] The forget stopped after { $done } of { $total } snapshots. Those stay forgotten. Run it again to forget the rest.
  }

# A check found something in the repository to tidy, such as a pack no index lists, which a backup killed before it saved its index leaves. Unless the check also found an error, no snapshot's data depends on it.
# $detail (text): rustic's own words, which name what it found. It is shown as it came, at the end of the message.
warning-check-warning = The check found something in the repository to tidy. Unless it also found an error, no snapshot's data depends on it, and the next prune tidies most of what a check finds like this.
    .with-detail = The check found something in the repository to tidy. Unless it also found an error, no snapshot's data depends on it, and the next prune tidies most of what a check finds like this. What it found: { $detail }

# A check found an error: damage in the repository, something in it that is not as its index or its snapshots say, or a part of the repository, or of this machine's copy of it, that could not be read or did not match.
# $detail (text): rustic's own words, which name what it found. It is shown as it came, at the end of the message.
warning-check-error = The check found an error in the repository or in reading it, so a snapshot that needs the part it concerns may not restore whole. Check the disk or the service the repository is on, then check the repository again.
    .with-detail = The check found an error in the repository or in reading it, so a snapshot that needs the part it concerns may not restore whole. Check the disk or the service the repository is on, then check the repository again. What it found: { $detail }

# A repository this program maintains itself (forgets and prunes from this machine) holds snapshots that other machines made. Any of them may maintain it too, which the program cannot tell.
# $count (count): how many other hosts there are.
# $hosts (text): the names of the first five of them, separated by commas. Keep them as they are.
warning-maintenance-other-hosts = { $count ->
    [one] This repository holds snapshots from another host, { $hosts }. { -brand-name } forgets and prunes it from here, and cannot tell whether that host does too. Make sure only one machine maintains it.
   *[other] This repository holds snapshots from { $count } other hosts, including { $hosts }. { -brand-name } forgets and prunes it from here, and cannot tell whether any of them does too. Make sure only one machine maintains it.
  }

# A warning this version of the program has no message for, such as one from a machine running a newer version.
# $detail (text): the sender's own message. It is shown as it came.
warning-unknown = The operation reported a problem that this version of { -brand-name } cannot describe, and went on.
    .with-detail = The operation reported a problem that this version of { -brand-name } cannot describe, and went on: { $detail }

# A repository already exists where one was to be initialised, so nothing was written.
# $location (text): where the repository is.
error-repository-exists = { -brand-name } did not initialise a repository at { $location }, because one is already there. Connect the existing repository instead.

# No repository is at the location: it is a folder on this machine holding nothing of a repository, so nothing was opened or written.
error-repository-not-found = There is no repository at this location. First check that the drive or location can be reached and is the right one; if it is, you can create the repository there.

# No repository's config file was found at the location, and nothing tells whether the place is empty or out of reach: a folder that isn't there (as on a drive that isn't plugged in) or can't be read, or a server that holds no repository there. Nothing was opened or written.
# $detail (text): the engine's own message, which names the location, with every secret in it masked. It is shown as it came.
error-repository-config-not-found = No repository was found at this location, so nothing was read or written. Check that the location is the right one and that its drive, folder or server can be reached and read; a repository that was never created there has to be created first.
    .with-detail = No repository was found at this location ({ $detail }), so nothing was read or written. Check that the location is the right one and that its drive, folder or server can be reached and read; a repository that was never created there has to be created first.

# The server the repository is on answered and refused the credentials it was sent (HTTP 401 or 403), so nothing was read or written. Not a missing repository: the server never said what it holds.
# $half (text): cold or hot, the half of a hot/cold repository whose server refused them. The sender leaves it out for a repository with no hot half, and the program then supplies the text none; any other value names neither half.
error-credentials-refused = { $half ->
    [cold] The server of this repository's cold half refused the credentials { -brand-name } sent, so nothing was read or written. Check the user name and password in the cold half's location or its options, and that the server gives that user this repository; a credential read from a file must hold nothing else, not even a trailing space.
    [hot] The server of this repository's hot half refused the credentials { -brand-name } sent, so nothing was read or written. Check the user name and password in the hot half's location or its options, and that the server gives that user this repository; a credential read from a file must hold nothing else, not even a trailing space.
    [none] The server refused the credentials { -brand-name } sent for this repository, so nothing was read or written. Check the user name and password in the repository's location or its options, and that the server gives that user this repository; a credential read from a file must hold nothing else, not even a trailing space.
   *[other] The server of one half of this repository refused the credentials { -brand-name } sent, so nothing was read or written. Check the user name and password in each half's location and options, and that each server gives that user this repository; a credential read from a file must hold nothing else, not even a trailing space.
  }

# A repository reached at several locations had none that could be used: each was silent, refused, held no repository or another one, or a secret of it was unavailable. The operation's detail lists each location and why.
error-no-location-usable = No location of this repository could be used, so nothing was read or written. The operation lists each location and why it was skipped; check that one of them can be reached, then try again.

# Every location of the repository has a rule for the network it may be tried on, and none held when the job started: the machine was on another kind of network, or couldn't tell which it was on. The operation's detail lists each location and why.
error-no-location-rule-holds = No location of this repository may be tried on the network this machine is on, so nothing was read or written. The operation lists each location's rule and why it didn't hold; connect to a network one of them allows, or run again later.

# The repository's password opened none of its keys: at its one location, or at every one of its locations that answered.
error-password-not-accepted = The repository's password was not accepted, so nothing was read or written. Check the repository's password, then try again.

# A saved repository's Create was refused because its row doesn't offer it: no backup found the location empty, or one reached the repository there once, so nothing says the location can be reached and holds no repository. Nothing was run.
error-create-not-offered = This repository wasn't created: { -brand-name } creates one only where a backup found the location empty. Check that the drive or location can be reached, then run a backup.

# A repository did not answer a test, a connection or an initialisation in time.
error-repository-timed-out = The repository did not answer in time, so { -brand-name } stopped waiting and saved nothing. Check that it can be reached, then try again.

# The configuration already holds a repository under this alias.
# $alias (text): the alias the configuration already uses.
error-repository-alias-taken = { -brand-name } already has a repository called { $alias }. Choose another name.

# The configuration already holds this repository, by its location or its ID, under another alias.
# $alias (text): the alias the repository already has.
error-repository-already-configured = { -brand-name } already has this repository, called { $alias }, and a repository has one name. Use { $alias } instead.

# The location of a configured repository now holds another repository than the one the configuration recorded.
error-repository-replaced = The repository's location now holds a different repository, so { -brand-name } read and wrote nothing there. Change the location to where the repository is now, or, if the new repository is meant to take its place, remove { "repository-id" } from its entry in the configuration file.

# A repository was to be saved under a name that is not an alias.
error-repository-alias-invalid = { -brand-name } cannot use that name for a repository. Use up to 64 lower-case letters, digits and hyphens, and start with a letter or a digit.

# A repository's secret has nothing in its source.
# $field (text): where in the settings the secret is, as the configuration names it.
# $source (text): the variable, file, keyring entry or program that should give it.
error-secret-missing = { -brand-name } found no secret in { $source }, which the repository's { $field } refers to. Check that it is there and not empty.

# A repository's secret has a source that could not be reached.
# $field (text): where in the settings the secret is.
# $source (text): the variable, file, keyring entry or program that should give it.
# $detail (text): the message of the platform, shown as it came with secrets masked.
error-secret-unreadable = { -brand-name } could not read { $source }, which the repository's { $field } refers to. Check that it is there and that the user { -brand-name } runs as may read it.
    .with-detail = { -brand-name } could not read { $source }, which the repository's { $field } refers to ({ $detail }). Check that it is there and that the user { -brand-name } runs as may read it.

# A repository's secret is in a keyring entry, and this computer has no keyring service that can be asked for it: no Secret Service runs, there is no session bus to reach one by, or the service refused otherwise than as locked, as when it has no collection. A server, or a service that starts before anyone logs in, usually has none.
# $field (text): where in the settings the secret is.
# $source (text): the keyring entry, as service/name.
# $detail (text): the message of the platform, shown as it came with secrets masked.
# `password-file`, `password-command` and `password-env` are settings, typed as they are: keep them string literals.
error-secret-keyring-unavailable = { -brand-name } could not read the keyring entry { $source }, which the repository's { $field } refers to, because this computer has no keyring service it can use. A server, or a service that starts before anyone logs in, usually has none. Start a Secret Service in a session and unlock it, or keep the secret in a file, a command or an environment variable instead ({ "password-file" }, { "password-command" } or { "password-env" }).
    .with-detail = { -brand-name } could not read the keyring entry { $source }, which the repository's { $field } refers to, because this computer has no keyring service it can use ({ $detail }). A server, or a service that starts before anyone logs in, usually has none. Start a Secret Service in a session and unlock it, or keep the secret in a file, a command or an environment variable instead ({ "password-file" }, { "password-command" } or { "password-env" }).

# A repository's secret is in a keyring entry, and the keyring service answered that its keyring is locked, or the person dismissed its unlock prompt, which leaves it locked, so the entry couldn't be read, though it may well be there. Logging in to the desktop usually unlocks it; a run that starts at login can come before that.
# $field (text): where in the settings the secret is.
# $source (text): the keyring entry, as service/name.
# $detail (text): the message of the platform, shown as it came with secrets masked.
# `password-file`, `password-command` and `password-env` are settings, typed as they are: keep them string literals.
error-secret-keyring-locked = { -brand-name } could not read the keyring entry { $source }, which the repository's { $field } refers to, because the keyring is locked. Unlock it in the system's keyring app (logging in to the desktop usually does that), then try again. To back up while the keyring is locked, keep the secret in a file, a command or an environment variable instead ({ "password-file" }, { "password-command" } or { "password-env" }).
    .with-detail = { -brand-name } could not read the keyring entry { $source }, which the repository's { $field } refers to, because the keyring is locked ({ $detail }). Unlock it in the system's keyring app (logging in to the desktop usually does that), then try again. To back up while the keyring is locked, keep the secret in a file, a command or an environment variable instead ({ "password-file" }, { "password-command" } or { "password-env" }).

# A repository's secret is not text.
# $field (text): where in the settings the secret is.
# $source (text): where it is read from.
error-secret-not-text = { $source }, which the repository's { $field } refers to, does not hold text. A secret has to be text.

# A repository's secret is longer than a secret may be.
# $field (text): where in the settings the secret is.
# $source (text): where it is read from.
# $limit (bytes): the most a secret may hold.
error-secret-too-large = { $source }, which the repository's { $field } refers to, holds more than { $limit }, which is more than a secret can be.

# The command that gives a repository's secret ran and failed.
# $field (text): where in the settings the secret is.
# $source (text): the program that was run.
# $detail (text): the first line the command wrote to stderr, with secrets masked.
error-secret-command-failed = The command { $source }, which gives the repository's { $field }, failed. Run it yourself to see why.
    .with-detail = The command { $source }, which gives the repository's { $field }, failed ({ $detail }). Run it yourself to see why.

# The command that gives a repository's secret succeeded, but something it
# started kept its output open, and the secret's line never ended, so it gave
# no whole secret.
# $field (text): where in the settings the secret is.
# $source (text): the program that was run.
error-secret-command-output-open = The command { $source }, which gives the repository's { $field }, finished, but something it started kept its output open, and the line with the { $field } never ended. Make the command print the { $field } followed by a new line, or close its output before it exits.

# The command that gives a repository's secret ran for too long.
# $field (text): where in the settings the secret is.
# $source (text): the program that was run.
# $seconds (count): the seconds it was given.
error-secret-command-timed-out = { $seconds ->
    [one] The command { $source }, which gives the repository's { $field }, did not finish in { $seconds } second, so { -brand-name } stopped it.
   *[other] The command { $source }, which gives the repository's { $field }, did not finish in { $seconds } seconds, so { -brand-name } stopped it.
  }

# The keyring did not give a repository's secret within the time limit. On
# macOS the usual cause is a keychain prompt asking to allow access that nobody
# answered.
# $field (text): where in the settings the secret is.
# $source (text): the keyring entry, as service/name.
# $seconds (count): the seconds it was given.
# "Always Allow" is the label of the button in the macOS keychain prompt: use
# the label macOS shows in your language.
error-secret-keyring-timed-out = { $seconds ->
    [one] The keyring did not answer within { $seconds } second when asked for the keyring entry { $source }, which the repository's { $field } refers to. On macOS, the keychain may be waiting for you to allow access in a prompt: answer it with Always Allow, then try again.
   *[other] The keyring did not answer within { $seconds } seconds when asked for the keyring entry { $source }, which the repository's { $field } refers to. On macOS, the keychain may be waiting for you to allow access in a prompt: answer it with Always Allow, then try again.
  }

# A repository's secret file could not be read within the time limit, as when
# it is on a network share that stopped answering.
# $field (text): where in the settings the secret is.
# $source (text): the file's path.
# $seconds (count): the seconds it was given.
error-secret-file-timed-out = { $seconds ->
    [one] { -brand-name } could not read the file { $source }, which the repository's { $field } refers to, within { $seconds } second. A file on a network share that stopped answering can hang this way: check that the share can be reached, then try again.
   *[other] { -brand-name } could not read the file { $source }, which the repository's { $field } refers to, within { $seconds } seconds. A file on a network share that stopped answering can hang this way: check that the share can be reached, then try again.
  }

# A setting that a program is started with refers to a secret.
# $field (text): where in the settings it is.
error-secret-in-process-arguments = The repository's { $field } is handed to a program as an argument, so it cannot refer to a secret: every user of the computer could read it there. Write the value out, or put the secret where the program reads it on its own.

# A setting with its references in is longer than a secret may be.
# $field (text): where in the settings it is.
# $limit (bytes): the most it may hold.
error-secret-expansion-too-long = The repository's { $field } is longer than { $limit } once the secrets it refers to are put in. Shorten it.

# The secrets of a repository, each counted once, come to more than a job can hide in everything it shows. The engine counts the credentials of the daemon's proxy settings with them.
# $limit (bytes): the most they may come to together.
# $field (text): the setting at which they passed the limit: one of the repository's, such as password or options.note, or a proxy variable of the daemon's environment, such as HTTPS_PROXY. The sender leaves it out when the repository's own secrets passed it where the engine counted them, and the program then supplies the text none.
error-secrets-too-large-to-mask = { $field ->
    [none] The repository's secrets come to more than { $limit } together, which is more than a job can hide in everything it shows, so its jobs cannot run. Refer to fewer or shorter secrets: a backend that reads a key from a file can be given the file's path instead of its contents.
   *[other] The repository's secrets, with any credentials in the daemon's proxy settings, come to more than { $limit } together, which is more than a job can hide in everything it shows, so its jobs cannot run. They passed it at the setting { $field }. Refer to fewer or shorter secrets: a backend that reads a key from a file can be given the file's path instead of its contents.
  }

# A setting is not a valid template.
# $field (text): where in the settings it is.
# $at (text): the place in it where the fault starts, counted in bytes from 0. The sender leaves it out when it has no place, and the program then supplies the text none.
# `${VAR}` and `${file:/path}` are references typed as they are: keep them string literals.
error-template-invalid = { $at ->
    [none] The repository's { $field } is too long or has too many references. Shorten it, or refer to fewer secrets. A reference is { "${VAR}" } for an environment variable or { "${file:/path}" } for a file.
   *[other] The repository's { $field } has a reference that is not valid at byte { $at }. A reference is { "${VAR}" } for an environment variable or { "${file:/path}" } for a file.
  }

# A location is an rclone connection string with more credentials written in it than a job takes: each would have to be hidden in everything the job shows.
# $field (text): which location it is.
# rclone is a program's name: keep it as it is.
error-location-too-many-credentials = The repository's { $field } is an rclone connection string with more credentials written in it than a job takes. Keep rclone's credentials in its own configuration file, and name the remote that holds them in the location instead.

# A setting names a file, a folder or a program on this computer by a relative path, such as backups/repo or ~/backups.
# $field (text): where in the settings it is.
# The slash is the character a full path starts with: keep it as it is.
error-repository-path-relative = The repository's { $field } is a path that does not start with /, so it would be read from whichever folder { -brand-name } was started in. Write it in full, starting with /.

# The person chose to keep the password they typed for a repository in the system keyring, and this computer has no keyring service to keep it in: no Secret Service runs, there is no session bus to reach one by, or it refused otherwise than as locked, as when it has no collection. Nothing was saved.
# $entry (text): the keyring entry, as service/name, such as ladderback/home-nas.
# $detail (text): the message of the platform, shown as it came with secrets masked.
# `password-file` is a setting, typed as it is: keep it a string literal.
error-keyring-unavailable = { -brand-name } could not keep the password in the keyring entry { $entry }, because this computer has no keyring service it can use, so nothing was saved. A server, or a service that starts before anyone logs in, usually has none. Keep the password in the configuration file instead, or in a file the configuration names ({ "password-file" }).
    .with-detail = { -brand-name } could not keep the password in the keyring entry { $entry }, because this computer has no keyring service it can use ({ $detail }), so nothing was saved. A server, or a service that starts before anyone logs in, usually has none. Keep the password in the configuration file instead, or in a file the configuration names ({ "password-file" }).

# The person chose to keep the password they typed for a repository in the system keyring, and the keyring service answered that its keyring is locked, or the person dismissed its unlock prompt, which leaves it locked. Nothing was saved or created, and the form keeps what was typed.
# $entry (text): the keyring entry, as service/name.
# $detail (text): the message of the platform, shown as it came with secrets masked.
# `password-file` is a setting, typed as it is: keep it a string literal.
error-keyring-locked = The password was not saved in the keyring entry { $entry }, because the keyring is locked. Nothing else changed: the repository and the rest of the form are as they were. Unlock the keyring in the system's keyring app (logging in to the desktop usually does that), then save again. If the keyring often starts locked, keep the password in a file ({ "password-file" }) or a systemd credential instead.
    .with-detail = The password was not saved in the keyring entry { $entry }, because the keyring is locked ({ $detail }). Nothing else changed: the repository and the rest of the form are as they were. Unlock the keyring in the system's keyring app (logging in to the desktop usually does that), then save again. If the keyring often starts locked, keep the password in a file ({ "password-file" }) or a systemd credential instead.

# The person chose to keep the password they typed for a repository in the system keyring, and the keyring refused it, failed, or did not answer in time. Nothing was saved.
# $entry (text): the keyring entry, as service/name.
# $detail (text): the message of the platform, shown as it came with secrets masked.
error-keyring-unwritable = { -brand-name } could not keep the password in the keyring entry { $entry }, so nothing was saved. Check that the keyring is unlocked and try again, or keep the password in the configuration file.
    .with-detail = { -brand-name } could not keep the password in the keyring entry { $entry } ({ $detail }), so nothing was saved. Check that the keyring is unlocked and try again, or keep the password in the configuration file.

# The person chose to keep the password they typed for a repository in the system keyring, and its entry already holds another password, which is never replaced: it may be the only copy of another repository's password. Nothing was saved.
# $entry (text): the keyring entry, as service/name.
error-keyring-entry-taken = The keyring entry { $entry } already holds another password, which { -brand-name } never replaces, so nothing was saved. Choose another name for the repository, or keep the password in the configuration file.

## Profile codes: importing and exporting a rustic profile (technical design §6.2)

# The profile, or a profile it uses, could not be read.
# $file (text): the path of the profile as it was given, or of the profile it uses that could not be read.
error-profile-unreadable = { -brand-name } could not read the profile { $file }, so nothing was imported. Check that the file is there and that the user { -brand-name } runs as may read it, and that the profiles it uses are there too.

# The profile is larger than any profile needs.
# $file (text): the path of the profile.
# $limit (bytes): the largest size { -brand-name } reads, as a size such as 1 MiB.
error-profile-too-large = { -brand-name } did not read the profile { $file }, because it is larger than { $limit }. A profile is far smaller, so check that the file is the right one. Nothing was imported.

# The file is not a profile that rustic accepts.
# $file (text): the path of the profile.
# $key (text): the setting at fault, as its path in the profile. The sender leaves it out when none is known, and the program then supplies the text none.
# $line (text): the line of the file where the problem was found. The sender leaves it out when none is known, and the program then supplies the text none.
# $detail (text): what is wrong, as the reader reported it. It is shown as it came, in English.
error-profile-invalid = { -brand-name } did not accept the profile { $file }{ $key ->
    [none] {""}
   *[other] , at the setting { $key }
  }{ $line ->
    [none] {""}
   *[other] , line { $line }
  }, so nothing was imported. Correct the profile, then try again.
    .with-detail = { -brand-name } did not accept the profile { $file }{ $key ->
        [none] {""}
       *[other] , at the setting { $key }
      }{ $line ->
        [none] {""}
       *[other] , line { $line }
      }, so nothing was imported ({ $detail }). Correct the profile, then try again.

# The profile says something that { -brand-name } cannot carry over.
# $file (text): the path of the profile.
# $key (text): the setting, as its path in the profile.
# $line (text): the line of the file where it is written. The sender leaves it out when none is known, and the program then supplies the text none.
error-profile-unsupported = The profile { $file } says something { -brand-name } cannot carry over, at { $key }{ $line ->
    [none] {""}
   *[other] {" "}(line { $line })
  }, so nothing was imported. Change that setting in the profile, then import it again.

# The configuration already has a plan with this ID, or the profile makes two plans with one ID.
# $plan (text): the plan's ID, such as home.
error-plan-id-taken = { -brand-name } already has a plan with the ID { $plan }, or the profile makes two of them, so nothing was imported. Give the profile's snapshot entries different names, or remove the plan from the configuration.

# The profile runs commands (hooks, a password command, an option rustic runs as a command), and nobody confirmed them, so nothing was imported.
# `--yes` is typed as it is: keep it a string literal.
error-profile-commands-unconfirmed = Nothing was imported: the profile runs commands that nobody confirmed, or that changed after they were listed. Run the import at a terminal to see them and answer, or run it again with { "--yes" } once you have read them.

# The plan can't be exported, because the repository's password is written in the configuration.
# `password` is a setting of the configuration, typed as it is: keep it a string literal.
error-inline-secret = { -brand-name } did not export the plan, because the repository's { "password" } is written in its configuration, and a profile would have to repeat it as text. Move the password to a keyring, a file, a command or an environment variable, then export again.

# The plan can't be exported, because a setting of the repository is a reference rustic can't resolve.
# $key (text): the setting, as its path in the profile, such as repository.
error-profile-export-reference = { -brand-name } did not export the plan, because the repository's { $key } refers to something at run time, which a profile cannot say for that setting. Write the value itself in the configuration, then export again.

# The plan can't be exported, because the address of the repository holds a password or another secret.
# $key (text): the setting that holds it, repository or repo-hot.
error-profile-export-url-password = { -brand-name } did not export the plan, because the { $key } of the repository holds a password or another secret in its address, and a profile would have to repeat it as text. Move the credentials out of the address into the repository's options, which an export leaves out and names the environment variable for, then export again.

# The plan can't be exported, because one of its tags holds a comma.
# $tag (text): the tag.
# The comma is typed as it is: keep it a string literal.
error-profile-export-tag-comma = { -brand-name } did not export the plan, because its tag { $tag } holds a comma ({ "," }), which rustic reads as two tags. Rename the tag without the comma, then export again.

# A location of a repository reached at several was skipped where it shouldn't have been.
# $position (text): the location's place in the list, from 1.
# $outcome (text): why, as a code: other-repository, secret-unavailable, template-invalid, repository-path-relative, secret-expansion-too-long, secret-in-process-arguments, location-too-many-credentials, silent, refused, not-a-repository or password-not-accepted.
warning-location-skipped = { $outcome ->
    [other-repository] Location { $position } holds another repository, so it was not used. Check its address.
    [secret-unavailable] Location { $position } was not tried, because a secret it needs could not be read.
    [template-invalid] Location { $position } was not tried, because one of its settings is not a valid template. The operation names the setting.
    [repository-path-relative] Location { $position } was not tried, because one of its paths is relative once resolved. Write an absolute path; the operation names the setting.
    [secret-expansion-too-long] Location { $position } was not tried, because one of its settings is too long once its references are resolved. The operation names the setting.
    [secret-in-process-arguments] Location { $position } was not tried, because a setting passed to a program holds a reference, which would show its secret. The operation names the setting.
    [location-too-many-credentials] Location { $position } was not tried, because its rclone connection string holds more credentials than a job can hide.
    [silent] Location { $position } did not answer, although its rule held.
    [refused] Location { $position } refused its credentials, although its rule held.
    [not-a-repository] Location { $position } holds no repository, although its rule held.
   *[other] Location { $position } did not accept the repository's password, although its rule held.
  }

# A location of a repository sends its credentials where whoever answers at its address can read them, so an impostor on the network could take them.
# $position (text): the location's place in the list, from 1.
# $cause (text): why: plain-http, unchecked-certificate, plain-endpoint, plain-rclone (an rclone connection string's url) or plain-ftp.
# $rule (text): network when the location has a rule about the network the machine is on, which a network made to look like the intended one passes too; none when it has no rule and is tried on every network.
# $locations (text): one for a repository of one location, which the page lists without locations, so the text speaks of the repository and not of $position; several otherwise.
# `http-cacert` is an option of the configuration, typed as it is: keep it a string literal.
# `ftps://` is the scheme an FTP endpoint is written with, typed as it is: keep it a string literal.
# `url` and `override.ca_cert` are settings of an rclone connection string, typed as they are: keep them string literals. rclone is a program's name: keep it as it is.
warning-location-plain-http-credentials = { $locations ->
    [one] { $rule ->
        [network] { $cause ->
            [unchecked-certificate] This repository accepts any certificate, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers at its address can read its credentials. Check the certificate instead, with { "http-cacert" } for a private certificate authority's certificate: HTTPS that checks the certificate stops an impostor before the credentials are sent.
            [plain-endpoint] This repository sends its credentials to an address without HTTPS, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers there can read them. Use HTTPS, which checks the server before the credentials are sent, with { "http-cacert" } for a private certificate authority's certificate.
            [plain-rclone] This repository sends its credentials through rclone to an address without HTTPS, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers there can read them. Use HTTPS in the connection string's { "url" }, which checks the server before the credentials are sent, with { "override.ca_cert" } beside it for a private certificate authority's certificate.
            [plain-ftp] This repository sends its credentials over FTP without encryption, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers at its address can read them. Write its endpoint with { "ftps://" } instead, which checks the server before the credentials are sent, or use a backend over HTTPS. { "http-cacert" } doesn't apply to FTP, so the operating system must trust the server's certificate, or the certificate authority that signed it.
           *[other] This repository sends its credentials over plain HTTP, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers at its address can read them. Use HTTPS, which checks the server before the credentials are sent, with { "http-cacert" } for a private certificate authority's certificate.
        }
       *[none] { $cause ->
            [unchecked-certificate] This repository accepts any certificate, so on any network the machine is on, whoever answers at its address can read its credentials. Check the certificate instead, with { "http-cacert" } for a private certificate authority's certificate.
            [plain-endpoint] This repository sends its credentials to an address without HTTPS, so on any network the machine is on, whoever answers there can read them. Use HTTPS, with { "http-cacert" } for a private certificate authority's certificate.
            [plain-rclone] This repository sends its credentials through rclone to an address without HTTPS, so on any network the machine is on, whoever answers there can read them. Use HTTPS in the connection string's { "url" }, with { "override.ca_cert" } beside it for a private certificate authority's certificate.
            [plain-ftp] This repository sends its credentials over FTP without encryption, so on any network the machine is on, whoever answers at its address can read them. Write its endpoint with { "ftps://" } instead. { "http-cacert" } doesn't apply to FTP, so the operating system must trust the server's certificate, or the certificate authority that signed it.
           *[other] This repository sends its credentials over plain HTTP, so on any network the machine is on, whoever answers at its address can read them. Use HTTPS, with { "http-cacert" } for a private certificate authority's certificate.
        }
    }
   *[several] { $rule ->
        [network] { $cause ->
            [unchecked-certificate] Location { $position } accepts any certificate, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers at its address can read its credentials. Check the certificate instead, with { "http-cacert" } for a private certificate authority's certificate: HTTPS that checks the certificate stops an impostor before the credentials are sent.
            [plain-endpoint] Location { $position } sends its credentials to an address without HTTPS, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers there can read them. Use HTTPS, which checks the server before the credentials are sent, with { "http-cacert" } for a private certificate authority's certificate.
            [plain-rclone] Location { $position } sends its credentials through rclone to an address without HTTPS, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers there can read them. Use HTTPS in the connection string's { "url" }, which checks the server before the credentials are sent, with { "override.ca_cert" } beside it for a private certificate authority's certificate.
            [plain-ftp] Location { $position } sends its credentials over FTP without encryption, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers at its address can read them. Write its endpoint with { "ftps://" } instead, which checks the server before the credentials are sent, or use a backend over HTTPS. { "http-cacert" } doesn't apply to FTP, so the operating system must trust the server's certificate, or the certificate authority that signed it.
           *[other] Location { $position } sends its credentials over plain HTTP, and its rule can't tell the network it is meant for from one made to look like it, so on any network that passes the rule, whoever answers at its address can read them. Use HTTPS, which checks the server before the credentials are sent, with { "http-cacert" } for a private certificate authority's certificate.
        }
       *[none] { $cause ->
            [unchecked-certificate] Location { $position } accepts any certificate, so whoever answers at its address can read its credentials. Check the certificate instead, with { "http-cacert" } for a private certificate authority's certificate.
            [plain-endpoint] Location { $position } sends its credentials to an address without HTTPS, so whoever answers there can read them. Use HTTPS, with { "http-cacert" } for a private certificate authority's certificate.
            [plain-rclone] Location { $position } sends its credentials through rclone to an address without HTTPS, so whoever answers there can read them. Use HTTPS in the connection string's { "url" }, with { "override.ca_cert" } beside it for a private certificate authority's certificate.
            [plain-ftp] Location { $position } sends its credentials over FTP without encryption, so whoever answers at its address can read them. Write its endpoint with { "ftps://" } instead. { "http-cacert" } doesn't apply to FTP, so the operating system must trust the server's certificate, or the certificate authority that signed it.
           *[other] Location { $position } sends its credentials over plain HTTP, so whoever answers at its address can read them. Use HTTPS, with { "http-cacert" } for a private certificate authority's certificate.
        }
    }
  }

# Every location of a repository has a rule about the network the machine is on, so on a network where no rule holds, nothing can be tried.
# $locations (text): one for a repository of one location, which the page lists without locations, so the text speaks of the repository; several otherwise.
warning-location-every-ruled = { $locations ->
    [one] This repository has a rule, so a scheduled run is skipped on a network where it doesn't hold, and any other run fails there. Remove the rule to have the repository tried on every network.
   *[several] Every location of this repository has a rule, so a scheduled run is skipped on a network where none holds, and any other run fails there. Leave one location without a rule to have it tried on every network.
  }

# A repository's last location has a rule about the network the machine is on, and a scheduled plan backing up to it doesn't retry a failed run.
# $plan (text): the plan's ID, as the configuration file writes it. It is shown as it came.
# $locations (text): one for a repository of one location, which the page lists without locations, so the text speaks of the repository; several otherwise.
# `retry` is a key of the configuration file, typed as it is: keep it a string literal.
warning-location-last-ruled-no-retry = { $locations ->
    [one] The plan { $plan } doesn't retry a failed run, and this repository has a rule: when the repository can't be used, as while a network comes up after the machine wakes, its scheduled run fails until the next one. Set { "retry" } on the plan to try again sooner.
   *[several] The plan { $plan } doesn't retry a failed run, and this repository's last location has a rule: when no location can be used, as while a network comes up after the machine wakes, its scheduled run fails until the next one. Set { "retry" } on the plan to try again sooner.
  }

# A location's rule lists Wi-Fi networks by name, and this machine couldn't read the name of the network it is on, so that part of the rule doesn't hold and the location is skipped there.
# $position (text): the location's place in the list, from 1.
# $locations (text): one for a repository of one location, which the page lists without locations, so the text speaks of the repository and not of $position; several otherwise.
# `wired` and `wifi` are values of the configuration file, typed as they are: keep them string literals.
warning-location-rule-name-unreadable = { $locations ->
    [one] Ladderback can't read the name of the Wi-Fi network this machine is on, so the rule of this repository, which names Wi-Fi networks, doesn't apply here and the repository isn't used. A rule of { "wired" } or { "wifi" } alone needs no name.
   *[several] Ladderback can't read the name of the Wi-Fi network this machine is on, so the rule of location { $position }, which names Wi-Fi networks, doesn't apply here and the location is skipped. A rule of { "wired" } or { "wifi" } alone needs no name.
  }

# This machine's IPv4 traffic and its IPv6 traffic leave by different network links, and a location of the repository has a rule about the network: a rule holds only if it holds for both links.
# $ipv4 (text): the kind of link IPv4 traffic leaves by: wired, wifi, or other for a tunnel, a bridge or a phone's tethering.
# $ipv6 (text): the kind of link IPv6 traffic leaves by, as $ipv4.
# $locations (text): one for a repository of one location, which the page lists without locations, so the text speaks of the repository's rule; several otherwise.
# `ipv6.route-metric` is a setting of NetworkManager, typed as it is: keep it a string literal.
warning-location-rule-mixed-link = { $locations ->
    [one] This machine's traffic leaves by two links, IPv4 by { $ipv4 ->
        [wired] a wired network
        [wifi] Wi-Fi
       *[other] another link
    } and IPv6 by { $ipv6 ->
        [wired] a wired network
        [wifi] Wi-Fi
       *[other] another link
    }, so the rule of this repository holds here only if it holds for both. To have one link carry both, give it the lower route metric for IPv6 too, as { "ipv6.route-metric" } does in NetworkManager, or put it first in the service order of the Mac's network settings.
   *[several] This machine's traffic leaves by two links, IPv4 by { $ipv4 ->
        [wired] a wired network
        [wifi] Wi-Fi
       *[other] another link
    } and IPv6 by { $ipv6 ->
        [wired] a wired network
        [wifi] Wi-Fi
       *[other] another link
    }, so a location's rule holds here only if it holds for both. To have one link carry both, give it the lower route metric for IPv6 too, as { "ipv6.route-metric" } does in NetworkManager, or put it first in the service order of the Mac's network settings.
  }

# An export wrote the repository's first location only, because a rustic profile has no fallback.
# $count (count): how many further locations it left out.
warning-profile-locations-not-exported = { $count ->
    [one] The profile points at the repository's first location only: { $count } further location was left out, because rustic has no fallback.
   *[other] The profile points at the repository's first location only: { $count } further locations were left out, because rustic has no fallback.
  }

# The import left out a setting of the profile.
# $key (text): the setting, as its path in the profile, such as copy or backup.group-by.
warning-profile-key-ignored = { -brand-name } left out { $key } of the profile, because it has no place for it. Check that nothing depends on it.

# The profile prunes after its forget, and the repository now prunes from one place.
# `maintenance` and `local` are a setting and its value in the configuration, typed as they are: keep them string literals.
warning-profile-prune = Pruning will run from one place (the { -hub } or this { -machine }), not after every forget as the profile asked. The repository's { "maintenance" } setting is now { "local" }: change it if that is not what you want.

# The profile's environment table was left out.
# `[global.env]`, `password-env` and `${VAR}` are typed as they are: keep them string literals.
warning-profile-env-ignored = { -brand-name } left out the { "[global.env]" } of the profile, because it does not set the environment of a job. Reference a variable with { "password-env" } or { "${VAR}" } instead, or set it in the environment { -brand-name } runs in.

# The hooks the profile set for all its snapshot entries now run once per plan.
warning-profile-hooks-per-plan = The hooks the profile set for all its snapshot entries now run once per plan, around each plan's backup, rather than once around everything.

# A profile named by the profile's use-profiles was not found.
# $name (text): the name as the profile wrote it.
warning-profile-used-missing = The profile uses a profile called { $name }, which was not found, so { -brand-name } went on without it. Check the name, or add the file to rustic's profile folder, then import again.

# A profile previewed from its text uses another profile, which a preview never reads.
# $name (text): the name as the profile wrote it.
warning-profile-used-not-read = The profile uses a profile called { $name }, which a preview doesn't read, so this preview leaves out what it would add. Import the profile from its file to merge it.

# A secret written in the profile was kept as text in the configuration.
# $field (text): where the secret is, as its setting.
warning-inline-secret = The profile holds a secret in { $field }, which { -brand-name } kept as text in its configuration. Move it to a keyring, a file or an environment variable, and refer to that instead.

# A repository's secret is written as text in the configuration file.
# $field (text): the setting it is in, as the configuration names it, such as password or options.secret_access_key. Keep it as it is.
# $position (text): the place, from 2, of the further location whose setting it is, in the repository's list of locations; none for the repository's own setting.
# `${VAR}` and `${file:/path}` are references typed as they are: keep them string literals.
warning-config-inline-secret = { $position ->
    [none] The { $field } setting of this repository holds a secret written as text in the configuration file. Keep the secret outside the file and refer to it: a password can come from a keyring, a file, a command or an environment variable, and an option from a { "${VAR}" } or { "${file:/path}" } reference.
   *[other] The { $field } setting of location { $position } holds a secret written as text in the configuration file. Keep the secret outside the file and refer to it with a { "${VAR}" } or { "${file:/path}" } reference.
  }

# A repository's location holds a credential written as text: a password in a URL, as in user:password@host, or a credential in an rclone connection string.
# $field (text): the setting that holds the address, as the profile or the configuration names it, such as repository.repository or repo-hot. Keep it as it is.
# $kind (text): what the address is: rclone, for an rclone connection string, which rclone is started with in its arguments, or url for any other.
# $position (text): the place, from 2, of the further location whose setting it is, in the repository's list of locations; none for the repository's own setting.
# `${VAR}`, `${file:/path}` and `rclone:remote:path` are typed as they are: keep them string literals.
warning-location-secret = { $position ->
    [none] { $kind ->
        [rclone] The address in the { $field } setting holds a credential as text, so the configuration file keeps it as text too, and rclone is started with it in its arguments, where other users of this computer can see it. Set up the remote in rclone's own configuration file, with the credential there, and name it in the address instead, as in { "rclone:remote:path" }.
       *[other] The address in the { $field } setting holds a password as text, so the configuration file keeps it as text too. Put a { "${VAR}" } or { "${file:/path}" } reference where the password is, or give the password in the backend's own option instead.
    }
   *[other] { $kind ->
        [rclone] The address in the { $field } setting of location { $position } holds a credential as text, so the configuration file keeps it as text too, and rclone is started with it in its arguments, where other users of this computer can see it. Set up the remote in rclone's own configuration file, with the credential there, and name it in the address instead, as in { "rclone:remote:path" }.
       *[other] The address in the { $field } setting of location { $position } holds a password as text, so the configuration file keeps it as text too. Put a { "${VAR}" } or { "${file:/path}" } reference where the password is, or give the password in the backend's own option instead.
    }
  }

# A repository's rclone-command setting, the command line that starts rclone, has a credential typed after one of rclone's flags, as in --pass=…: the configuration file keeps it as text, and rclone is started with it in its arguments.
# $field (text): the setting, as the configuration names it, such as options.rclone-command. Keep it as it is.
# $flag (text): the flag the credential follows, such as --pass or --s3-secret-access-key. Keep it as it is.
# $position (text): the place, from 2, of the further location whose setting it is, in the repository's list of locations; none for the repository's own setting.
warning-rclone-command-secret = { $position ->
    [none] The { $field } setting of this repository gives { $flag } a credential written as text, so the configuration file keeps it as text too, and rclone is started with it in its arguments, where other users of this computer may see it. Take it out of the command and keep it in rclone's own configuration file instead.
   *[other] The { $field } setting of location { $position } gives { $flag } a credential written as text, so the configuration file keeps it as text too, and rclone is started with it in its arguments, where other users of this computer may see it. Take it out of the command and keep it in rclone's own configuration file instead.
  }

# A glob of a plan has a slash in the middle, so rustic matches it against the whole path from the root of the file system, and written from one of the plan's sources it matches nothing. An exclude glob (it starts with !) that matches nothing leaves nothing out; an include glob (no !) that matches nothing leaves out every file that no other glob includes.
# $glob (text): the glob as written, such as !.local/share/containers. Keep it as it is.
# $plan (text): the plan's ID. Keep it as it is.
# $absolute (text): the same glob written from the root, under the source it is for as the backup walks it (its links followed). Keep it as it is. The sender leaves it out when the plan has several sources and the glob's folders are under none of them or under more than one, and the program then supplies the text none.
warning-glob-never-matches = The glob { $glob } of the plan { $plan } matches nothing: rustic matches a glob with a slash in the middle against the whole path, from the root of the file system, not from the source. Starting with !, it leaves nothing out; without one, it leaves out every file no other glob includes. { $absolute ->
    [none] Write the whole path instead, starting with the folder of the source it belongs to.
   *[other] Write the whole path instead, as in { $absolute }.
  }

# One of the program's own tags was found on a snapshot entry and dropped.
# $tag (text): the tag.
warning-profile-tag-dropped = { -brand-name } dropped the tag { $tag } from a plan, because tags that start like that are its own. It adds them itself.

# An export left an option out of the profile because it is a reference.
# $option (text): the option's name.
# $variable (text): the environment variable to set instead.
warning-profile-option-left-out = The option { $option } was left out of the profile, because it is a reference that rustic cannot resolve. Set the environment variable { $variable } instead.

# An export of a plan with no retention has no keep option, and rustic's forget refuses to run without one.
# `keep-*`, `forget` and `rustic forget` are what a person types, typed as they are: keep them string literals.
warning-profile-retention-empty = The plan keeps every snapshot, so the profile has no { "keep-*" } option, and rustic's { "forget" } refuses to run until one is set. { -brand-name } never forgets from this plan. Add a { "keep-*" } option to the profile before you run { "rustic forget" } on it.

# An import added its plans to a repository the configuration already had at the profile's location, and kept one of that repository's settings, which the profile set otherwise.
# $repository (text): the repository's name in the configuration.
# $setting (text): the setting, as the configuration names it: password, maintenance, group-by, options, options-hot, options-cold or hooks. Keep it as it is.
warning-profile-setting-kept = The repository { $repository } was already configured, so the profile's plans were added to it, and it kept its own { $setting }: the profile sets it otherwise. Change { $setting } in the configuration if the profile's is the one you want.

# An export wrote a profile that reads the repository's password from the keyring with a program that is not on this computer's PATH.
# $program (text): the program: secret-tool, or security on macOS. Keep it as it is.
# `secret-tool`, `libsecret`, `libsecret-tools` and `PATH` are what a person types, typed as they are: keep them string literals.
warning-profile-keyring-command-missing = { $program ->
    [secret-tool] The profile reads the repository's password with { "secret-tool" }, which is not on this computer's { "PATH" }, so rustic cannot read the password here. Install it where the profile will run: it comes in the { "libsecret" } package on Fedora, and in { "libsecret-tools" } on Debian and Ubuntu.
   *[other] The profile reads the repository's password with { $program }, which is not on this computer's { "PATH" }, so rustic cannot read the password here. Run the profile where { $program } is on the { "PATH" }.
  }

# An export for an AI agent or an API token left a command out of the profile: the password's command or a list of hooks, which a person wrote and which may hold a token.
# $key (text): the command's place in the profile, such as repository.password-command or global.hooks.run-before. Keep it as it is.
warning-profile-command-left-out = The command at { $key } was left out of the profile, because this export may be read by an AI agent or another program. To have it, export the plan from the web UI or the command line.

# An import made a command runnable: a hook, the password's command, or an option rustic runs as a command.
# $key (text): where the configuration keeps it, such as hooks.run-before or repositories.nas.password-command. Keep it as it is.
# $program (text): the program the command starts, such as curl.
warning-profile-command-imported = The import added a command at { $key } that runs { $program }.

# An import left the profile's global hooks out, because the configuration already had plans or repositories.
warning-profile-global-hooks-left-out = The profile's global hooks were left out, because they would also run around the plans and repositories the configuration already has. To have them, add them to this { -machine }'s hooks yourself.

# A notification of an event was not delivered to its channel, and is not tried again.
# $event (text): the event's name, such as backup.failed. Keep it as it is.
# $rule (text): the rule's name in the configuration. Keep it as it is.
# $channel (text): the channel's name in the configuration. Keep it as it is.
# $reason (text): why: secret-unavailable, invalid-address, unreachable, timed-out or rejected.
warning-notification-undelivered = { $reason ->
    [secret-unavailable] The rule { $rule } did not send { $event } to { $channel }, because a secret of the channel (its URL, token or password) could not be read. Check where the configuration says it is.
    [invalid-address] The rule { $rule } did not send { $event } to { $channel }, because the channel's URL is not an { "http" } or { "https" } address. Check the URL the channel reads.
    [unreachable] The rule { $rule } could not reach { $channel } to send { $event }. Check that the channel's server is up and that this computer can reach it, through its proxy if it has one.
    [timed-out] The rule { $rule } sent { $event } to { $channel }, but the channel did not answer in time.
   *[other] The rule { $rule } sent { $event } to { $channel }, but the channel refused it. Check the channel's URL and its token or password.
  }

# The daemon or a worker could not turn off its core dumps as it started, and went on without that protection.
# $detail (text): the operating system's own words, in English.
warning-core-dumps-on = { -brand-name } could not turn off core dumps for itself, so if it crashes, the passwords it holds may be written to disk. It went on without that protection. Check whether a security policy of this system refuses it.
    .with-detail = { -brand-name } could not turn off core dumps for itself ({ $detail }), so if it crashes, the passwords it holds may be written to disk. It went on without that protection. Check whether a security policy of this system refuses it.

# The daemon or a worker could not stop other programs of the same user from reading its memory as it started, and went on without that protection.
# $detail (text): the operating system's own words, in English.
warning-debugger-attach-allowed = { -brand-name } could not stop other programs run by the same user from attaching a debugger to it, so they could read the passwords it holds. It went on without that protection. Check whether a security policy of this system refuses it.
    .with-detail = { -brand-name } could not stop other programs run by the same user from attaching a debugger to it ({ $detail }), so they could read the passwords it holds. It went on without that protection. Check whether a security policy of this system refuses it.

## Command-line codes: what a command says when it cannot start (technical design §4.5)

# The command has no folder to keep the configuration in.
# `LADDERBACK_CONFIG_DIR` is an environment variable, typed as it is: keep it a string literal.
error-config-folder-unknown = { -brand-name } has no folder to keep its configuration in, so it did nothing. Set { "LADDERBACK_CONFIG_DIR" } to a folder, or run as a user who has a home folder.

# The command could not start the runtime it works on.
# $detail (text): the operating system's message. It is shown as it came.
error-runtime-unavailable = { -brand-name } could not start the runtime it works on, so it did nothing. Check the limits on open files and threads for the user { -brand-name } runs as, then try again.
    .with-detail = { -brand-name } could not start the runtime it works on, so it did nothing ({ $detail }). Check the limits on open files and threads for the user { -brand-name } runs as, then try again.
