### The default text of a notification (technical design §11): one message per
### event, sent to a webhook as its `message`, and to an ntfy topic as the
### notification's body. A rule's own template replaces it. No message holds a
### secret, a location or the engine's own words: only the facts of the event.
### A plan's ID, a repository's alias and a code are typed by the user or are
### stable names: they go in as they are.

# A backup started.
# $plan (text): the plan's ID, such as home, or the operation's ID when the backup has no plan.
notify-backup-started = The backup of { $plan } started.

# A backup ended well, with no warning.
# $plan (text): the plan's ID, such as home, or the operation's ID when the backup has no plan.
notify-backup-succeeded = The backup of { $plan } succeeded.

# A backup ended well, but with warnings: files it couldn't read, a hook that warned.
# $plan (text): the plan's ID, such as home, or the operation's ID when the backup has no plan.
notify-backup-warning = The backup of { $plan } succeeded with warnings. Its history says what they were.

# A backup failed.
# $plan (text): the plan's ID, such as home, or the operation's ID when the backup has no plan.
# $code (text): the failure's code, such as operation-failed, or none when it is unknown.
notify-backup-failed = The backup of { $plan } failed ({ $code }).

# A backup failed, and the agent planned one of its retries.
# $plan (text): the plan's ID, such as home, or the operation's ID when the backup has no plan.
# $code (text): the failure's code, such as operation-failed, or none when it is unknown.
# $attempt (number): which retry this is, such as 1.
# $attempts (number): how many retries the plan makes at most, such as 3.
# $at (instant): when the retry runs, read in UTC; the zone's name follows it.
notify-backup-retrying = The backup of { $plan } failed ({ $code }). Retry { $attempt } of { $attempts } runs at { DATETIME($at) } { DATETIME($at, timeZoneName: "short") }.

# A forget of old snapshots failed.
# $repository (text): the repository's alias, such as nas, or the operation's ID when the agent has none for it.
# $code (text): the failure's code, such as operation-failed, or none when it is unknown.
notify-forget-failed = Forgetting old snapshots in { $repository } failed ({ $code }).

# A prune of a repository failed.
# $repository (text): the repository's alias, such as nas, or the operation's ID when the agent has none for it.
# $code (text): the failure's code, such as operation-failed, or none when it is unknown.
notify-prune-failed = The prune of { $repository } failed ({ $code }).

# A check of a repository failed.
# $repository (text): the repository's alias, such as nas, or the operation's ID when the agent has none for it.
# $code (text): the failure's code, such as operation-failed, or none when it is unknown.
notify-check-failed = The check of { $repository } failed ({ $code }).
