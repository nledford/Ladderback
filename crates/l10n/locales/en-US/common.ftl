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

# A warning this version of the program has no message for, such as one from a newer agent.
# $detail (text): the sender's own message. It is shown as it came.
warning-unknown = The operation reported a problem that this version of { -brand-name } cannot describe, and went on.
    .with-detail = The operation reported a problem that this version of { -brand-name } cannot describe, and went on: { $detail }

# A repository already exists where one was to be initialised, so nothing was written.
# $location (text): where the repository is.
error-repository-exists = { -brand-name } did not initialise a repository at { $location }, because one is already there. Connect the existing repository instead.

# A repository did not answer a test, a connection or an initialisation in time.
error-repository-timed-out = The repository did not answer in time, so { -brand-name } stopped waiting and saved nothing. Check that it can be reached, then try again.

# The configuration already holds a repository under this alias.
# $alias (text): the alias the configuration already uses.
error-repository-alias-taken = { -brand-name } already has a repository called { $alias }. Choose another name.

# A repository was to be saved under a name that is not an alias.
error-repository-alias-invalid = { -brand-name } cannot use that name for a repository. Use up to 64 lower-case letters, digits and hyphens, and start with a letter or a digit.

# A repository's settings hold a secret reference that could not be resolved.
# $field (text): where in the settings the reference is.
error-secret-unresolved = { -brand-name } could not read the secret in the repository's { $field }. Check where the secret is kept.

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
warning-config-inline-secret = The { $field } of this repository is written as text in the configuration file. Keep the secret in a keyring, a file, a command or an environment variable instead, and refer to it from the configuration.

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
