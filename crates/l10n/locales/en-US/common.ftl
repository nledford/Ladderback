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

# The request names an operation the log has none of.
# $operation (text): the operation's ID.
error-operation-unknown = There is no operation { $operation }. Refresh the history page, or run a new operation.

# The request asked to cancel an operation that has already ended.
# $operation (text): the operation's ID.
error-operation-not-running = Operation { $operation } has already ended, so there is nothing to cancel.

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

# A warning this version of the program has no message for, such as one from a newer agent.
# $detail (text): the sender's own message. It is shown as it came.
warning-unknown = The operation reported a problem that this version of { -brand-name } cannot describe, and went on.
    .with-detail = The operation reported a problem that this version of { -brand-name } cannot describe, and went on: { $detail }

# A repository already exists where one was to be initialised, so nothing was written.
# $location (text): where the repository is.
error-repository-exists = { -brand-name } did not initialise a repository at { $location }, because one is already there. Connect the existing repository instead.

# The configuration already holds a repository under this alias.
# $alias (text): the alias the configuration already uses.
error-repository-alias-taken = { -brand-name } already has a repository called { $alias }. Choose another name.

# A repository's settings hold a secret reference that could not be resolved.
# $field (text): where in the settings the reference is.
error-secret-unresolved = { -brand-name } could not read the secret in the repository's { $field }. Check where the secret is kept.
