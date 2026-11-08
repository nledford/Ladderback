### Words and helpers every surface shares, and the messages of the API's error
### and warning codes (technical design §8.4). A code's message is named after
### its name on the wire: `error-<code>` or `warning-<code>`.

# The product's name. A message that says it uses this term.
-brand-name = Steading

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

# The engine's process never took its job.
error-engine-start-timed-out = The engine did not take its job in time, so the operation never ran. Run it again.

# The engine stopped reporting, but its process stayed alive.
error-engine-exit-timed-out = The engine stopped reporting, so { -brand-name } ended its process. Check the repository, then run the operation again.

error-host-not-allowed = { -brand-name } refused this request because it names a host that { -brand-name } does not answer for. Open { -brand-name } at localhost or 127.0.0.1, or allow the host with --allowed-host.

error-origin-not-allowed = { -brand-name } refused this request because it came from a different site. Open { -brand-name } at its own address and try again.

error-json-required = { -brand-name } refused this request because it was not sent as JSON. Send a request that changes anything with the header Content-Type: application/json.

# An error this version of the program has no message for, such as one from a newer agent.
# $detail (text): the sender's own message. It is shown as it came.
error-unknown = An unexpected error occurred.
    .with-detail = An unexpected error occurred: { $detail }

## Warning codes

# A file or folder the backup could not read.
# $detail (text): rustic's message about why it could not be read. It is shown as it came.
warning-source-unreadable = { -brand-name } could not read this item, so it was left out of the snapshot. Check that the user { -brand-name } runs as can read it, or exclude it from the plan.
    .with-detail = { -brand-name } could not read this item, so it was left out of the snapshot ({ $detail }). Check that the user { -brand-name } runs as can read it, or exclude it from the plan.

# A warning this version of the program has no message for, such as one from a newer agent.
# $detail (text): the sender's own message. It is shown as it came.
warning-unknown = The operation reported a problem that this version of { -brand-name } cannot describe, and went on.
    .with-detail = The operation reported a problem that this version of { -brand-name } cannot describe, and went on: { $detail }
