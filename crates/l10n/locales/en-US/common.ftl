### Words and helpers every surface shares, and the messages of the API's error
### and warning codes (technical design §8.4). A code's message is named after
### its name on the wire: `error-<code>` or `warning-<code>`.
###
### What a person types or looks up elsewhere, such as an address, an option or
### a header, is a string literal: `{ "--allowed-host" }`. It is no text of a
### language, so no translation changes it, and a pseudolocale leaves it as it is.

# The product's name. A message that says it uses this term.
-brand-name = Ladderback

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

# The daemon stopped while the operation was queued or running: a crash, a kill or a power cut.
error-daemon-stopped = { -brand-name } stopped before this operation ended, so the outcome of the operation is unknown. Check the repository, then run the operation again.

# The request named a host the daemon does not answer for.
# `localhost`, `127.0.0.1` and `--allowed-host` are typed as they are: keep them string literals.
error-host-not-allowed = { -brand-name } refused this request because it names a host that { -brand-name } does not answer for. Open { -brand-name } at { "localhost" } or { "127.0.0.1" }, or allow the host with { "--allowed-host" }.

error-origin-not-allowed = { -brand-name } refused this request because it came from a different site. Open { -brand-name } at its own address and try again.

# A request that changes something was not sent as JSON.
# `Content-Type: application/json` is a header, typed as it is: keep it a string literal.
error-json-required = { -brand-name } refused this request because it was not sent as JSON. Send a request that changes anything with the header { "Content-Type: application/json" }.

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

# An error this version of the program has no message for, such as one from a newer agent.
# $detail (text): the sender's own message. It is shown as it came.
error-unknown = An unexpected error occurred.
    .with-detail = An unexpected error occurred: { $detail }

# The request names a plan the configuration does not have.
# $plan (text): the plan's ID, such as home.
error-plan-unknown = There is no plan { $plan }. Add it to the configuration, or run a plan that exists.

# The request names a repository the configuration does not have.
# $alias (text): the repository's alias, such as demo.
error-repository-unknown = There is no repository { $alias }. Add it to the configuration, or choose a repository that exists.

# A request's body was not the JSON the route takes.
error-invalid-body = { -brand-name } could not read the request's body. Send the JSON the route takes.

# A request's body was larger than the most its route takes, so nothing ran.
# $limit (bytes): the most the route takes, as a size such as 128 KiB.
error-body-too-large = { -brand-name } did not read this request, because its body is larger than { $limit }. Shorten the body or leave it out, then try again.

# The request names an operation the log has none of.
# $operation (text): the operation's ID.
error-operation-unknown = There is no operation { $operation }. Refresh the history page, or run a new operation.

# The request asked to cancel an operation that has already ended.
# $operation (text): the operation's ID.
error-operation-not-running = Operation { $operation } has already ended, so there is nothing to cancel.


# Another maintenance operation already runs on the repository, so the
# request was refused and nothing ran.
# $operation (text): the running operation's ID.
error-maintenance-busy = Operation { $operation } is already maintaining this repository, and one repository runs one maintenance operation at a time. Wait for it to end, then ask again.

# This agent doesn't maintain the repository: its entry doesn't set
# maintenance to local, so a forget, a prune, a check or an adoption, or the
# preview of one, was refused and nothing was asked of the repository.
# $alias (text): the repository's alias, such as demo.
error-maintenance-not-owner = This agent does not maintain the repository { $alias }, so nothing was asked of it. To maintain it from this agent, set { "maintenance = \"local\"" } in its entry in the configuration file.

# The plan's retention policy would keep nothing, so the forget was refused
# before anything was removed.
# $plan (text): the plan's ID, such as home.
error-retention-keeps-nothing = The policy of plan { $plan } would keep none of its snapshots, so nothing was removed. Give the policy something to keep, or delete the snapshots you mean to delete.

# The group an adoption named matches no plan of this agent, so nothing
# was written.
error-adoption-no-match = No plan of this agent matches that history, so nothing was written. Point the adoption at the plan whose host, label and sources it carries.

# More than one plan matches the group an adoption named, so nothing was
# written.
# $plans (text): the plans that match, such as home, mirror.
error-adoption-ambiguous = Plans { $plans } all match that history, and which one it belongs to is yours to say. Point the adoption at one plan, or change the others so only one matches.

# A forget or an adoption was started with the selection its preview named,
# and the snapshots it would act on have changed since, so nothing was done.
error-maintenance-selection-changed = The repository changed after you reviewed this, so nothing was done. Review it again to see what it would do now.

# The repository did not answer a read in time, so there was nothing to show
# or to review.
error-repository-read-timed-out = The repository did not answer in time, so what was asked of it could not be read. Check that it can be reached, then try again.

# The request asks for a maintenance kind this version does not run.
# $kind (text): the kind as it was sent.
error-maintenance-kind-unknown = { -brand-name } does not run maintenance of kind { $kind }. Ask for forget, adopt, prune or check.

# A history request's cursor is not an operation's ID.
# $cursor (text): the cursor as it was sent.
error-invalid-cursor = { -brand-name } could not continue the history from { $cursor }. Use the cursor the previous page gave.

## Warning codes

# A file or folder the backup could not read.
# $detail (text): rustic's message about why it could not be read. It is shown as it came.
warning-source-unreadable = { -brand-name } could not read this item, so it was left out of the snapshot. Check that the user { -brand-name } runs as can read it, or exclude it from the plan.
    .with-detail = { -brand-name } could not read this item, so it was left out of the snapshot ({ $detail }). Check that the user { -brand-name } runs as can read it, or exclude it from the plan.

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

# A check found an error: damage in the repository, something in it that is not as its index or its snapshots say, or a part of the repository, or of this agent's copy of it, that could not be read or did not match.
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

# A warning this version of the program has no message for, such as one from a newer agent.
# $detail (text): the sender's own message. It is shown as it came.
warning-unknown = The operation reported a problem that this version of { -brand-name } cannot describe, and went on.
    .with-detail = The operation reported a problem that this version of { -brand-name } cannot describe, and went on: { $detail }

# A repository already exists where one was to be initialised, so nothing was written.
# $location (text): where the repository is.
error-repository-exists = { -brand-name } did not initialise a repository at { $location }, because one is already there. Connect the existing repository instead.

# No repository is at the location: it is a folder on this machine holding nothing of a repository, so nothing was opened or written.
error-repository-not-found = There is no repository at this location. First check that the drive or location can be reached and is the right one; if it is, you can create the repository there.

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

# The import left out a setting of the profile.
# $key (text): the setting, as its path in the profile, such as copy or backup.group-by.
warning-profile-key-ignored = { -brand-name } left out { $key } of the profile, because it has no place for it. Check that nothing depends on it.

# The profile prunes after its forget, and the repository now prunes from one place.
# `maintenance` and `local` are a setting and its value in the configuration, typed as they are: keep them string literals.
warning-profile-prune = Pruning will run from one place (the hub or this agent), not after every forget as the profile asked. The repository's { "maintenance" } setting is now { "local" }: change it if that is not what you want.

# The profile's environment table was left out.
# `[global.env]`, `password-env` and `${VAR}` are typed as they are: keep them string literals.
warning-profile-env-ignored = { -brand-name } left out the { "[global.env]" } of the profile, because it does not set the environment of a job. Reference a variable with { "password-env" } or { "${VAR}" } instead, or set it in the environment { -brand-name } runs in.

# The hooks the profile set for all its snapshot entries now run once per plan.
warning-profile-hooks-per-plan = The hooks the profile set for all its snapshot entries now run once per plan, around each plan's backup, rather than once around everything.

# A profile named by the profile's use-profiles was not found.
# $name (text): the name as the profile wrote it.
warning-profile-used-missing = The profile uses a profile called { $name }, which was not found, so { -brand-name } went on without it. Check the name, or add the file to rustic's profile folder, then import again.

# A secret written in the profile was kept as text in the configuration.
# $field (text): where the secret is, as its setting.
warning-inline-secret = The profile holds a secret in { $field }, which { -brand-name } kept as text in its configuration. Move it to a keyring, a file or an environment variable, and refer to that instead.

# A repository's secret is written as text in the configuration file.
# $field (text): where it is, as the configuration names it, such as password or options.secret_access_key. Keep it as it is.
# `${VAR}` and `${file:/path}` are references typed as they are: keep them string literals.
warning-config-inline-secret = The { $field } of this repository is written as text in the configuration file. Keep the secret outside the file and refer to it: a password can come from a keyring, a file, a command or an environment variable, and an option from a { "${VAR}" } or { "${file:/path}" } reference.

# A repository's location holds a credential written as text: a password in a URL, as in user:password@host, or a credential in an rclone connection string.
# $field (text): the setting that holds the location, as the profile or the configuration names it, such as repository.repository or repo-hot. Keep it as it is.
# $kind (text): what the location is: rclone, for an rclone connection string, which rclone is started with in its arguments, or url for any other.
# `${VAR}`, `${file:/path}` and `rclone:remote:path` are typed as they are: keep them string literals.
warning-location-secret = { $kind ->
    [rclone] The location in { $field } holds a credential as text, so the configuration file keeps it as text too, and rclone is started with it in its arguments, where other users of this computer can see it. Set up the remote in rclone's own configuration file, with the credential there, and name it in the location instead, as in { "rclone:remote:path" }.
   *[other] The location in { $field } holds a password as text, so the configuration file keeps it as text too. Put a { "${VAR}" } or { "${file:/path}" } reference where the password is, or give the password in the backend's own option instead.
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

## Command-line codes: what a command says when it cannot start (technical design §4.5)

# The command has no folder to keep the configuration in.
# `LADDERBACK_CONFIG_DIR` is an environment variable, typed as it is: keep it a string literal.
error-config-folder-unknown = { -brand-name } has no folder to keep its configuration in, so it did nothing. Set { "LADDERBACK_CONFIG_DIR" } to a folder, or run as a user who has a home folder.

# The command could not start the runtime it works on.
# $detail (text): the operating system's message. It is shown as it came.
error-runtime-unavailable = { -brand-name } could not start the runtime it works on, so it did nothing. Check the limits on open files and threads for the user { -brand-name } runs as, then try again.
    .with-detail = { -brand-name } could not start the runtime it works on, so it did nothing ({ $detail }). Check the limits on open files and threads for the user { -brand-name } runs as, then try again.
