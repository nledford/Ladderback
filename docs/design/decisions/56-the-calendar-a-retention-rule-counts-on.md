# Decision 56: The calendar a retention rule counts on

**Each snapshot's own: the offset its clock recorded when it was taken** (recorded 2026-11-11, §3.1; #116). A calendar rule (`keep-hourly` through `keep-yearly`, and the `keep-within-*` form of each) puts a snapshot in the hour, day, week (the ISO week number within the calendar year, as rustic counts it), month, quarter, half-year and year that the clock which took it read. A snapshot time is stored as an instant with that clock's offset (`2026-11-10T23:30:00-06:00`); rustic reads it back in that fixed offset, and its `ForgetGroups` compares each snapshot's calendar fields in the snapshot's own offset. The zone of the machine that runs the forget, a plan's schedule zone and UTC play no part.

Why:

- **It is what rustic does**, so an exported profile (§6.2) and a `rustic forget` run anywhere remove what Ladderback removes. In `rustic_core` at the pinned revision, `RusticTime` deserializes a snapshot time without a zone name into `TimeZone::fixed` of its offset (`repofile.rs`), and `equal_day`, `equal_week` and the other checks compare `sn.time.year()`, `day_of_year()` and the rest of those values (`commands/forget.rs`). rustic 0.11.4 does the same: with snapshots taken at `2026-11-01 12:00 +00:00`, `2026-11-10 00:30 +13:00` and `2026-11-10 23:30 -10:00`, `forget --dry-run --keep-daily 2 --group-by ''` removed the `+13:00` one under `TZ=UTC`, `Pacific/Auckland`, `America/Chicago` and `Pacific/Honolulu` alike.
- **The answer doesn't depend on where the forget runs.** From M4b a hub in another zone forgets a plan's snapshots, and it must remove what the agent would.
- **It needs no change.** The real `Retention` adapter runs `ForgetGroups` over the repository's own snapshot files (decision 25), which carry the offsets. The listing's `Snapshot` has only a `Timestamp` and no offset, so whatever evaluates retention reads the snapshot files, never the listing.

What it means, by example (the `@e2e` outline in `features/maintenance/forget.feature` checks the first three):

- A backup at 23:30 in Chicago (`-06:00`) counts on 10 November, though it is 00:30 on 11 November in New York and 05:30 in UTC.
- Daylight saving time: the offset is the one in force when the snapshot was taken. In Chicago on 1 November 2026, `00:30 -05:00` and `23:30 -06:00` are one day, 24 hours apart.
- A laptop flown from Auckland to Hawaii takes snapshots at `2026-11-10 00:30 +13:00` and `2026-11-10 23:30 -10:00`, 46 hours apart and both on 10 November: `keep-daily` keeps one of them. No single zone puts them on one day. For a machine that travels, a day can be longer or shorter than 24 hours. rustic compares each snapshot only with the next older one, so a date that comes back after an eastward flight over the date line, as from Auckland to Hawaii (10 November, then 9 November, then 10 November again) counts as a day of its own each time.
- A machine whose clock runs in UTC, such as a container with no zone set, records `+00:00`, and its days are UTC days.
- A snapshot time with no offset, which neither rustic nor Ladderback writes, is read as UTC, as rustic reads it.
- The snapshot list shows a snapshot time in the viewer's zone (decision 35), so a viewer in another zone can see a date other than the one retention counts the snapshot on (#243).

**Across an upgrade nothing changes:** this records what forget already did, so a repository forgets exactly what it did before (I6).

Rejected: the forgetting machine's zone (the result would depend on where the forget runs and differ from rustic's); a zone set per plan (a new setting that differs from rustic and from every exported profile); UTC (differs from rustic, and a backup at 23:30 in Chicago would count on the next day).
