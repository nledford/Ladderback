# Decision 96: Creating, changing and deleting plans in the UI

**A Plans page creates, changes and deletes plans. Each is one change of the configuration file, made under the daemon's configuration write lock by the rules the configuration keeps when it loads, and saved the way the file is written by hand (decision 69). The routes are for a session only. A plan a notification rule names isn't deleted. A deleted plan's snapshots stay in the repository, and a new plan given its ID would take them over** (recorded 2026-11-15, §6.1, §4.2, §4.4; decided by the maintainer on #616).

**Amended 2026-11-24 (#1527, proposed with the fix; the maintainer may change it):** the form still has no field for an interval schedule (`every`), so a plan on one opens with an empty cron field. That field's hint then says how often the plan runs and that an empty field keeps it, in place of the hint that empty means no schedule, and an emptied cron field removes the schedule only when it held a cron schedule. A plan on an interval saved unchanged, or with the field left blank, keeps its interval; a cron schedule typed over it replaces it. The plain schedule choice (#1396) says how an interval reads back once it has one.

## Why

- **The configuration is the aggregate.** `AgentConfig::add_plan`, `change_plan` and `remove_plan` give the configuration the change makes, checked whole as a loaded one is (`AgentConfig::new`, then the notification rules), or refuse it: a plan whose ID another has, one for a repository the configuration doesn't have, one that backs up nothing, or a change to a plan the configuration no longer has. A refusal saves nothing.
- **Under the write lock, load to save.** `EditPlans` holds `ConfigWrites` from the read to the save, so a plan change and a recorded repository ID (decision 57) can't undo each other.
- **One parser.** The form sends a plan's table in the file's keys, and the config adapter reads it with the loader's own code, so a plan the page saves is a plan the file loads, refused at the same key with the same problem.
- **The scheduler hears at once.** A save wakes the scheduler, so a new or changed plan's next run shows without waiting for its next reread (§5).
- **A plan a rule names stays.** Deleting it would leave a rule for a plan the configuration doesn't have, which the file refuses to load; the refusal names the rule.
- **Snapshots stay.** Forget removes only snapshots a plan owns (I6), so a deleted plan's snapshots become an orphaned group nothing forgets on its own. They can still be forgotten by selecting them, or adopted (decision 89).
- **A reused ID takes them over.** A plan's snapshots carry its ID, so a new plan given a deleted plan's ID owns them again and applies its own retention to them (decision 89, point 8). The delete dialog says so.
- **For a session only.** The routes are not in the API tokens' scope table (decision 93).
- **Hooks as written.** A hook command is edited as the file's text, never as a resolved secret (I4).
- **One refusal, one field.** The loader stops at the first fault, so a refusal (`plan-invalid`) names one setting and what is wrong with it, as the loader reports it; a plan with several faults is refused at the first.
- **No schedule, said.** An emptied cron field removes the whole schedule, so the plan runs only when it is started by hand. The form says so twice: the field's hint, and the save's announcement when a plan that had a schedule is saved without one, since a schedule lost by mistake would otherwise go unnoticed until a backup is missed.

Known limits: the last save wins, so a change saved over a form opened earlier, or over a hand edit made meanwhile, replaces it (the history keeps the old file); renaming a plan is #425.
