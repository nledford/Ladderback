//! Editing the file a person wrote (§6.1): a change is made in the file as
//! it is, and the rest of it, comments, blank lines and layout, stays as
//! written. Recording a repository's ID adds one key this way (decision 57),
//! and a save sets, adds and removes the settings it changes (decision 69).

use std::collections::HashMap;
use std::path::Path;

use domain::planning::AgentConfig;
use toml_edit::{ArrayOfTables, Decor, DocumentMut, Item, RawString, Table, TableLike};

use crate::schema::{Schemas, over_cap, parse, render_as};

/// The byte order mark a file may start with.
const BOM: &str = "\u{feff}";

/// `text` changed by `edit`, which is given it as the editor keeps it:
/// without a byte order mark and with `\n` line endings. The file's own
/// mark and line ending are put back; a file whose lines end both ways has
/// no own ending, and is `mixed`, unedited.
pub(crate) fn as_written<E>(
    text: &str,
    mixed: E,
    edit: impl FnOnce(&str) -> Result<String, E>,
) -> Result<String, E> {
    let (bom, body) = text
        .strip_prefix(BOM)
        .map_or(("", text), |body| (BOM, body));
    let lines = body.matches('\n').count();
    let crlf = body.matches("\r\n").count();
    let edited = if crlf == 0 {
        edit(body)?
    } else if crlf == lines {
        edit(&body.replace("\r\n", "\n"))?.replace('\n', "\r\n")
    } else {
        return Err(mixed);
    };
    Ok(format!("{bom}{edited}"))
}

/// `text`, the file as it is, changed to hold `config`, whose text in the
/// daemon's layout is `rendered`; `None` when it can't be.
///
/// The change is what differs between the file's configuration and
/// `config`, each written in the daemon's layout: a setting whose text
/// differs there is set where the file writes it, added after the last
/// setting of its table, or removed with its line; a plan is found by its
/// ID. Everything else is left as written, so a setting the file writes
/// its own way (`'…'`, `"5 minutes"`, a default said out loud) keeps that
/// way until a save changes it. The result must read back as `config`, as
/// a load reads it, and fit in a file.
pub(crate) fn saved(
    file: &Path,
    text: &str,
    config: &AgentConfig,
    rendered: &str,
    schemas: &Schemas,
) -> Option<String> {
    // A file in an older version is the load's to migrate, as a whole.
    let before = parse(file, text, schemas)
        .ok()
        .filter(|parsed| !parsed.migrated)?;
    let base = render_as(file, &before.config, schemas.current).ok()?;
    let edited = as_written(text, (), |body| merged(body, &base, rendered).ok_or(())).ok()?;
    let read = parse(file, &edited, schemas).ok()?;
    (read.config == *config && !over_cap(edited.len())).then_some(edited)
}

/// `text` with the change from `base` to `new` made in it.
fn merged(text: &str, base: &str, new: &str) -> Option<String> {
    let mut doc: DocumentMut = text.parse().ok()?;
    let base: DocumentMut = base.parse().ok()?;
    let new: DocumentMut = new.parse().ok()?;
    let mut left = Left::default();
    merge(
        doc.as_table_mut(),
        Some(base.as_table()),
        new.as_table(),
        0,
        &mut left,
    );
    left.put_back(&mut doc);
    Some(doc.to_string())
}

/// Makes the change from `base` to `new`, a table in the daemon's layout
/// before and after the save, in `mine`, that table as the file writes it,
/// whose body is written under the header at position `at` or its own.
fn merge(
    mine: &mut Table,
    base: Option<&dyn TableLike>,
    new: &dyn TableLike,
    at: isize,
    left: &mut Left,
) {
    let at = header_at(mine).unwrap_or(at);
    for (key, item) in new.iter() {
        set(
            mine,
            key,
            base.and_then(|base| base.get(key)),
            item,
            at,
            left,
        );
    }
    for (key, _) in base.into_iter().flat_map(TableLike::iter) {
        if !new.contains_key(key) {
            remove(mine, key, at, left);
        }
    }
}

/// Makes `key` of `mine` say what `new` says, where `base` is what it said
/// before the save, both in the daemon's layout.
fn set(mine: &mut Table, key: &str, base: Option<&Item>, new: &Item, at: isize, left: &mut Left) {
    let Some(item) = mine.get_mut(key) else {
        if !same(base, new) {
            mine.insert(key, placed(new));
        }
        return;
    };
    match (item, new.as_table_like(), new) {
        (Item::Table(table), Some(new), _) => {
            merge(table, base.and_then(Item::as_table_like), new, at, left);
        }
        (Item::ArrayOfTables(tables), _, Item::ArrayOfTables(new)) => {
            plans(tables, base.and_then(Item::as_array_of_tables), new, left);
        }
        (item, _, _) => {
            if same(base, new) {
                return;
            }
            // A value written as tables of its own, a hook list, is written
            // again whole as the daemon writes it, spaced as a setting is.
            let tables = !item.is_value();
            if tables {
                left.headers(item);
            }
            replace(item, new);
            if let Some(mut key) = mine.key_mut(key).filter(|_| tables) {
                key.leaf_decor_mut().clear();
            }
        }
    }
}

/// Removes `key` from `mine`, whose body is written under the header at
/// position `at`, with the comment directly above it: what is written
/// above that comment's blank line stays where it is.
fn remove(mine: &mut Table, key: &str, at: isize, left: &mut Left) {
    let Some(index) = mine.iter().position(|(name, _)| name == key) else {
        return;
    };
    let Some((key, item)) = mine.remove_entry(key) else {
        return;
    };
    if !in_body(&item) {
        left.headers(&item);
        return;
    }
    let kept = above(prefix(key.leaf_decor()));
    if kept.is_empty() {
        return;
    }
    match mine.iter_mut().skip(index).find(|(_, item)| in_body(item)) {
        Some((mut next, _)) => {
            let decor = next.leaf_decor_mut();
            let mut text = kept.to_owned();
            join(&mut text, prefix(decor));
            decor.set_prefix(text);
        }
        None => left.0.push((at, Gap::Setting(kept.to_owned()))),
    }
}

/// Whether `item` is written in the body of its table, not under a header
/// of its own.
fn in_body(item: &Item) -> bool {
    item.is_value() || item.as_table().is_some_and(Table::is_dotted)
}

/// The position of `table`'s header in the file, if it is written under
/// one the file has: a table only implied, by a header under it or by
/// dotted keys, has none.
fn header_at(table: &Table) -> Option<isize> {
    table.position()
}

/// The text written before an entry, from the end of the line before.
fn prefix(decor: &Decor) -> &str {
    decor
        .prefix()
        .and_then(RawString::as_str)
        .unwrap_or_default()
}

/// What the entries a save removed leave of the text before them, by the
/// position of the header they were written under or had, until every
/// change is made.
#[derive(Default)]
struct Left(Vec<(isize, Gap)>);

/// The text before a removed entry.
enum Gap {
    /// What is written above the comment of a setting that was the last
    /// one of its table.
    Setting(String),
    /// All that was written before a table's header.
    Header(String),
}

impl Left {
    /// Leaves the text before each header of `item`, removed.
    fn headers(&mut self, item: &Item) {
        match item {
            Item::Table(table) => self.header(table),
            Item::ArrayOfTables(tables) => tables.iter().for_each(|table| self.header(table)),
            Item::None | Item::Value(_) => {}
        }
    }

    /// Leaves the text before the header of `table`, removed, or before
    /// each header in it when it has none.
    fn header(&mut self, table: &Table) {
        match header_at(table) {
            Some(at) => self
                .0
                .push((at, Gap::Header(prefix(table.decor()).to_owned()))),
            None => table.iter().for_each(|(_, item)| self.headers(item)),
        }
    }

    /// Puts what the removed entries leave before the next header the file
    /// still has, or at its end.
    fn put_back(mut self, doc: &mut DocumentMut) {
        self.0.sort_by_key(|(at, _)| *at);
        let mut kept = Vec::new();
        each_header(doc.as_table_mut(), &mut |at, _| kept.push(at));
        kept.sort_unstable();
        let mut before: HashMap<isize, Vec<Gap>> = HashMap::new();
        let mut end = Vec::new();
        for (was, gap) in self.0 {
            match kept.get(kept.partition_point(|at| *at <= was)) {
                Some(at) => before.entry(*at).or_default().push(gap),
                None => end.push(gap),
            }
        }
        each_header(doc.as_table_mut(), &mut |at, table| {
            if let Some(gap) = before.remove(&at) {
                let text = filled(gap, prefix(table.decor()), false);
                table.decor_mut().set_prefix(text);
            }
        });
        if !end.is_empty() {
            let text = filled(end, doc.trailing().as_str().unwrap_or_default(), true);
            doc.set_trailing(text);
        }
    }
}

/// Calls `f` with each table under `table` written under a header of its
/// own, and that header's position.
fn each_header(table: &mut Table, f: &mut impl FnMut(isize, &mut Table)) {
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(table) => each_header_from(table, f),
            Item::ArrayOfTables(tables) => {
                for table in tables.iter_mut() {
                    each_header_from(table, f);
                }
            }
            Item::None | Item::Value(_) => {}
        }
    }
}

/// Calls `f` with `table`, if it is written under a header of its own, and
/// with each table under it that is.
fn each_header_from(table: &mut Table, f: &mut impl FnMut(isize, &mut Table)) {
    if let Some(at) = header_at(table) {
        f(at, table);
    }
    each_header(table, f);
}

/// The text before a header, or at the `end` of the file: `own`, what is
/// written there, after what the removed entries of `gap` leave. A
/// removed table goes with the comment directly above its header and the
/// lines written directly below its last setting; a removed setting goes
/// with the comment directly above it.
fn filled(gap: Vec<Gap>, own: &str, end: bool) -> String {
    let mut text = String::new();
    let mut after_header = false;
    for gap in gap {
        match gap {
            Gap::Setting(kept) => {
                join(&mut text, &kept);
                after_header = false;
            }
            Gap::Header(before) => {
                let before = if after_header {
                    below(&before, false)
                } else {
                    &before
                };
                join(&mut text, above(before));
                after_header = true;
            }
        }
    }
    let own = if after_header { below(own, end) } else { own };
    if end && own.trim().is_empty() {
        // Blank lines that went before a removed table end no file.
        let blank: usize = text
            .split_inclusive('\n')
            .rev()
            .take_while(|line| is_blank(line))
            .map(str::len)
            .sum();
        text.truncate(text.len() - blank);
    }
    join(&mut text, own);
    text
}

/// `before`, the text before an entry, up to and with its last blank
/// line: what is above the comment directly over the entry.
fn above(before: &str) -> &str {
    let mut end = 0;
    let mut read = 0;
    for line in before.split_inclusive('\n') {
        read += line.len();
        if is_blank(line) {
            end = read;
        }
    }
    &before[..end]
}

/// `after`, the text after an entry removed, from its first blank line:
/// the lines directly below the entry's last are its own. With no blank
/// line, they are those of the entry that follows, or the file's `end`.
fn below(after: &str, end: bool) -> &str {
    let mut read = 0;
    for line in after.split_inclusive('\n') {
        if is_blank(line) {
            return &after[read..];
        }
        read += line.len();
    }
    if end { "" } else { after }
}

/// Adds `more` to `text`, one blank line where both have one at the join.
fn join(text: &mut String, more: &str) {
    let mut more = more;
    if ends_blank(text) {
        while let Some((line, rest)) = more.split_once('\n') {
            if !line.trim().is_empty() {
                break;
            }
            more = rest;
        }
    }
    text.push_str(more);
}

/// Whether the last line of `text` is a whole blank one.
fn ends_blank(text: &str) -> bool {
    text.split_inclusive('\n').next_back().is_some_and(is_blank)
}

/// Whether `line` is a whole blank line.
fn is_blank(line: &str) -> bool {
    line.ends_with('\n') && line.trim().is_empty()
}

/// Makes the change from `base` to `new` in `mine`, the plans as the file
/// writes them, each found by its ID, in the order of `new`: a plan kept is
/// changed where it is written, one added follows the last, and one
/// removed goes with what is written in it.
fn plans(
    mine: &mut ArrayOfTables,
    base: Option<&ArrayOfTables>,
    new: &ArrayOfTables,
    left: &mut Left,
) {
    // By ID, not by a search per plan: a file may hold thousands of them.
    let mut old: HashMap<String, Table> = std::mem::take(mine)
        .into_iter()
        .filter_map(|table| Some((id_of(&table)?.to_owned(), table)))
        .collect();
    let was: HashMap<&str, &Table> = base
        .into_iter()
        .flat_map(ArrayOfTables::iter)
        .filter_map(|table| Some((id_of(table)?, table)))
        .collect();
    for plan in new.iter() {
        let id = id_of(plan).unwrap_or_default();
        let table = match old.remove(id) {
            Some(mut table) => {
                let was = was.get(id).map(|was| *was as &dyn TableLike);
                merge(&mut table, was, plan, 0, left);
                table
            }
            None => {
                let mut table = plan.clone();
                unplace(&mut table);
                table
            }
        };
        mine.push(table);
    }
    old.values().for_each(|table| left.header(table));
}

/// A plan's ID, which every plan has: the configuration was read.
fn id_of(plan: &Table) -> Option<&str> {
    plan.get("id").and_then(Item::as_str)
}

/// Whether `base` and `new`, both in the daemon's layout, say the same.
fn same(base: Option<&Item>, new: &Item) -> bool {
    base.is_some_and(|base| text(base) == text(new))
}

/// `item` written as one value, without the space around it.
fn text(item: &Item) -> String {
    let mut item = item.clone();
    item.make_value();
    if let Some(value) = item.as_value_mut() {
        value.decor_mut().clear();
    }
    item.to_string()
}

/// `new` as it goes into the file: a value spaced as the daemon writes
/// one, a table after a blank line and after the table before it.
fn placed(new: &Item) -> Item {
    let mut item = new.clone();
    match &mut item {
        Item::Value(value) => value.decor_mut().clear(),
        Item::Table(table) => unplace(table),
        Item::ArrayOfTables(tables) => tables.iter_mut().for_each(unplace),
        Item::None => {}
    }
    item
}

/// Drops the place `table` and the tables in it had in the daemon's text,
/// so each is written after the table before it in the file.
fn unplace(table: &mut Table) {
    table.set_position(None);
    table.decor_mut().set_prefix("\n");
    for (_, item) in table.iter_mut() {
        if let Some(table) = item.as_table_mut() {
            unplace(table);
        }
    }
}

/// Puts `new` in the place of `item`, as a value with the space and
/// comment `item` has around it.
fn replace(item: &mut Item, new: &Item) {
    let mut value = new.clone();
    value.make_value();
    if let (Some(value), Some(old)) = (value.as_value_mut(), item.as_value()) {
        *value.decor_mut() = old.decor().clone();
    }
    *item = value;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use domain::planning::{
        AgentConfig, AgentSettings, Concurrency, CronExpression, GroupBy, HookEntry, Hooks,
        Hostname, KeepCount, Maintenance, Plan, RepositoryConfig, RepositorySettings,
        RetentionPolicy, Schedule, Source,
    };
    use domain::secrets::SecretRef;
    use domain::{PlanId, RepositoryAlias};

    use super::saved;
    use crate::schema::{CURRENT_SCHEMA, MAX_CONFIG_BYTES, Schemas, parse, render_as};

    const FILE: &str = "config.toml";

    /// A file as a person writes one: comments, blank lines, their own
    /// order and spacing, defaults said out loud, and values written in
    /// another form than the daemon's (`'…'`, `"5 minutes"`, a list over
    /// several lines).
    const WRITTEN: &str = r#"# The demo laptop's agent, written by hand.
schema = 1

[agent]
concurrency = 1        # how many operations run at once
nice    =   10

# The NAS in the hall cupboard.
[repositories.nas]
  repository = '/srv/demo-repo'
  password-env = "DEMO_PASSWORD"
  maintenance = "none"   # the hub may own it later
  options = { timeout = "60s" }

[[plans]]
id = "home"
repository = "nas"
sources = [
  "/home/demo",        # everything
  "/srv/demo-shared",  # and the shared folder
]
schedule = { cron = "0 * * * *", jitter = "5 minutes" }

# How long the history goes back.
[plans.retention]
keep-daily = 7
"#;

    /// The parts of a configuration, to change one and build it again.
    struct Parts {
        agent: AgentSettings,
        hooks: Hooks,
        repositories: BTreeMap<RepositoryAlias, RepositoryConfig>,
        plans: Vec<Plan>,
    }

    fn read(text: &str) -> AgentConfig {
        parse(Path::new(FILE), text, &Schemas::production())
            .expect("the file parses")
            .config
    }

    fn rebuilt(config: &AgentConfig, change: impl FnOnce(&mut Parts)) -> AgentConfig {
        let mut parts = Parts {
            agent: config.agent().clone(),
            hooks: config.hooks().clone(),
            repositories: config.repositories().clone(),
            plans: config.plans().to_vec(),
        };
        change(&mut parts);
        AgentConfig::new(parts.agent, parts.hooks, parts.repositories, parts.plans)
            .expect("a valid configuration")
    }

    /// What a save of `config` writes over `text`, when it edits it.
    fn saved_as(text: &str, config: &AgentConfig) -> Option<String> {
        let rendered = render_as(Path::new(FILE), config, CURRENT_SCHEMA).expect("renders");
        saved(
            Path::new(FILE),
            text,
            config,
            &rendered,
            &Schemas::production(),
        )
    }

    /// What a save of the configuration in `text`, changed by `change`,
    /// writes over `text`.
    fn changed(text: &str, change: impl FnOnce(&mut Parts)) -> Option<String> {
        saved_as(text, &rebuilt(&read(text), change))
    }

    fn alias(name: &str) -> RepositoryAlias {
        RepositoryAlias::parse(name).expect("an alias")
    }

    fn plan_id(name: &str) -> PlanId {
        PlanId::parse(name).expect("a plan ID")
    }

    fn entry_at(location: &str) -> RepositoryConfig {
        RepositoryConfig {
            settings: RepositorySettings {
                repository: location.to_owned(),
                repo_hot: None,
                password: SecretRef::Env("DEMO_PASSWORD".to_owned()),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
            },
            id: None,
            maintenance: Maintenance::default(),
            group_by: GroupBy::default(),
            hooks: Hooks::default(),
        }
    }

    fn plan_mut<'p>(parts: &'p mut Parts, id: &str) -> &'p mut Plan {
        let id = plan_id(id);
        parts
            .plans
            .iter_mut()
            .find(|plan| plan.id == id)
            .expect("the plan")
    }

    #[test]
    fn a_save_of_the_configuration_as_it_was_changes_no_byte() {
        assert_eq!(changed(WRITTEN, |_| {}).as_deref(), Some(WRITTEN));
    }

    #[test]
    fn a_changed_setting_is_changed_where_it_is_written_beside_its_comment() {
        let saved = changed(WRITTEN, |parts| {
            parts.agent.concurrency = Concurrency::new(2).expect("a concurrency");
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace("concurrency = 1        #", "concurrency = 2        #"))
        );
    }

    #[test]
    fn a_setting_a_save_adds_follows_the_last_one_of_its_table() {
        let saved = changed(WRITTEN, |parts| {
            parts.agent.hostname = Some(Hostname::parse("demo-host").expect("a hostname"));
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "nice    =   10\n",
                "nice    =   10\nhostname = \"demo-host\"\n"
            ))
        );
    }

    #[test]
    fn a_setting_a_save_removes_goes_with_its_line() {
        let saved = changed(WRITTEN, |parts| {
            let nas = parts.repositories.get_mut(&alias("nas")).expect("nas");
            nas.settings.options.clear();
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace("  options = { timeout = \"60s\" }\n", ""))
        );
    }

    /// Only the comment directly above a removed setting goes with it.
    #[test]
    fn a_setting_a_save_removes_leaves_what_is_written_above_its_comment() {
        let written = WRITTEN.replace(
            "  maintenance = \"none\"   # the hub may own it later\n  options = { timeout = \"60s\" }\n",
            "  # password-env = \"OLD_PASSWORD\"\n\n  # Slow, so wait.\n  options = { timeout = \"60s\" }\n  maintenance = \"none\"   # the hub may own it later\n",
        );
        let saved = changed(&written, |parts| {
            let nas = parts.repositories.get_mut(&alias("nas")).expect("nas");
            nas.settings.options.clear();
        });
        assert_eq!(
            saved,
            Some(written.replace(
                "  # Slow, so wait.\n  options = { timeout = \"60s\" }\n",
                ""
            ))
        );
    }

    /// What is written above the comment of the last setting of a table
    /// stays at the end of the table when the setting goes.
    #[test]
    fn the_last_setting_a_save_removes_leaves_what_is_written_above_its_comment() {
        let written = WRITTEN.replace(
            "  options = { timeout = \"60s\" }\n",
            "  # timeout = \"30s\"\n\n  # Slow, so wait.\n  options = { timeout = \"60s\" }\n",
        );
        let saved = changed(&written, |parts| {
            let nas = parts.repositories.get_mut(&alias("nas")).expect("nas");
            nas.settings.options.clear();
        });
        assert_eq!(
            saved,
            Some(written.replace(
                "  # Slow, so wait.\n  options = { timeout = \"60s\" }\n\n",
                ""
            ))
        );
    }

    #[test]
    fn a_value_written_another_way_is_kept_until_it_changes_then_written_the_daemons_way() {
        let saved = changed(WRITTEN, |parts| {
            let home = plan_mut(parts, "home");
            let jitter = home.schedule.as_ref().and_then(Schedule::jitter);
            let schedule = Schedule::new(CronExpression::parse("0 2 * * *").expect("a cron"));
            home.schedule = Some(
                schedule
                    .with_jitter(jitter.expect("a jitter"))
                    .expect("a schedule"),
            );
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "schedule = { cron = \"0 * * * *\", jitter = \"5 minutes\" }",
                "schedule = { cron = \"0 2 * * *\", jitter = \"5m\" }"
            ))
        );
    }

    #[test]
    fn a_repository_a_save_adds_follows_the_last_one() {
        let saved = changed(WRITTEN, |parts| {
            parts
                .repositories
                .insert(alias("cloud"), entry_at("/srv/demo-cloud"));
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "  options = { timeout = \"60s\" }\n",
                "  options = { timeout = \"60s\" }\n\n[repositories.cloud]\nrepository = \"/srv/demo-cloud\"\npassword-env = \"DEMO_PASSWORD\"\n"
            ))
        );
    }

    #[test]
    fn a_repository_a_save_removes_goes_with_the_comment_above_it() {
        let two = format!(
            "{WRITTEN}\n# The old USB drive, retired.\n[repositories.usb]\nrepository = \"/Volumes/demo-usb/repo\"\npassword-env = \"DEMO_PASSWORD\"\n"
        );
        let saved = changed(&two, |parts| {
            parts.repositories.remove(&alias("usb"));
        });
        assert_eq!(saved.as_deref(), Some(WRITTEN));
    }

    /// `WRITTEN` with a second repository, `usb`, after `nas`, whose last
    /// line is one put out of use.
    fn two_repositories() -> String {
        WRITTEN.replace(
            "  options = { timeout = \"60s\" }\n",
            "  options = { timeout = \"60s\" }\n  # password-env = \"OLD_PASSWORD\"\n\n# The old USB drive, retired.\n[repositories.usb]\nrepository = \"/Volumes/demo-usb/repo\"\npassword-env = \"DEMO_PASSWORD\"\n",
        )
    }

    /// Only the comment directly above a removed repository goes with it:
    /// a line above the blank line before that comment is the one before's.
    #[test]
    fn a_repository_a_save_removes_leaves_what_is_written_above_its_comment() {
        let saved = changed(&two_repositories(), |parts| {
            parts.repositories.remove(&alias("usb"));
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "  options = { timeout = \"60s\" }\n",
                "  options = { timeout = \"60s\" }\n  # password-env = \"OLD_PASSWORD\"\n"
            ))
        );
    }

    /// A line written at the end of a removed repository goes with it, and
    /// the comment above the next one stays.
    #[test]
    fn a_repository_a_save_removes_goes_with_the_lines_written_at_its_end() {
        let two = two_repositories();
        let saved = changed(&two, |parts| {
            parts.repositories.remove(&alias("nas"));
            plan_mut(parts, "home").repository = alias("usb");
        });
        assert_eq!(
            saved,
            Some(
                two.replace(
                    "\n# The NAS in the hall cupboard.\n[repositories.nas]\n  repository = '/srv/demo-repo'\n  password-env = \"DEMO_PASSWORD\"\n  maintenance = \"none\"   # the hub may own it later\n  options = { timeout = \"60s\" }\n  # password-env = \"OLD_PASSWORD\"\n",
                    ""
                )
                .replace("repository = \"nas\"", "repository = \"usb\"")
            )
        );
    }

    /// The blank line above a removed repository's comment stays when the
    /// next table's header is written directly below its last line.
    #[test]
    fn a_repository_a_save_removes_leaves_the_blank_line_above_it_for_the_next_table() {
        let written = WRITTEN.replace(
            "  options = { timeout = \"60s\" }\n\n[[plans]]",
            "  options = { timeout = \"60s\" }\n\n# The old USB drive, retired.\n[repositories.usb]\nrepository = \"/Volumes/demo-usb/repo\"\npassword-env = \"DEMO_PASSWORD\"\n[[plans]]",
        );
        let saved = changed(&written, |parts| {
            parts.repositories.remove(&alias("usb"));
        });
        assert_eq!(saved.as_deref(), Some(WRITTEN));
    }

    /// What is written above the comment of a removed repository stays
    /// when nothing follows it in the file.
    #[test]
    fn a_repository_a_save_removes_at_the_end_leaves_what_is_written_above_its_comment() {
        let written = format!(
            "{WRITTEN}# keep-weekly = 4\n\n# The old USB drive, retired.\n[repositories.usb]\nrepository = \"/Volumes/demo-usb/repo\"\npassword-env = \"DEMO_PASSWORD\"\n"
        );
        let saved = changed(&written, |parts| {
            parts.repositories.remove(&alias("usb"));
        });
        assert_eq!(saved, Some(format!("{WRITTEN}# keep-weekly = 4\n")));
    }

    /// A table back at its defaults is not in the daemon's layout: a save
    /// removes it from the file with its comment.
    #[test]
    fn agent_and_hooks_back_at_their_defaults_go_from_the_file() {
        let written = WRITTEN.replace(
            "nice    =   10\n",
            "nice    =   10\n\n# Say so first.\n[hooks]\nrun-before = [\"echo one\"]\n",
        );
        let saved = changed(&written, |parts| {
            parts.agent = AgentSettings::default();
            parts.hooks = Hooks::default();
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "[agent]\nconcurrency = 1        # how many operations run at once\nnice    =   10\n\n",
                ""
            ))
        );
    }

    /// A hook list written as tables of its own is written again as the
    /// daemon writes it, under `[hooks]`, when it changes.
    #[test]
    fn a_hook_list_written_as_tables_is_written_whole_when_it_changes() {
        let written = "schema = 1\n\n# Say so first.\n[[hooks.run-before]]\ncommand = \"echo\"   # a greeting\nargs = [\"one\"]\n";
        let saved = changed(written, |parts| {
            parts
                .hooks
                .run_before
                .push(HookEntry::parse("echo two").expect("a hook"));
        });
        assert_eq!(
            saved.as_deref(),
            Some("schema = 1\n\n[hooks]\nrun-before = [\"echo one\", \"echo two\"]\n")
        );
    }

    #[test]
    fn the_last_repository_and_plan_a_save_removes_leave_the_rest() {
        let saved = changed(WRITTEN, |parts| {
            parts.repositories.clear();
            parts.plans.clear();
        });
        assert_eq!(
            saved.as_deref(),
            Some(
                "# The demo laptop's agent, written by hand.\nschema = 1\n\n[agent]\nconcurrency = 1        # how many operations run at once\nnice    =   10\n"
            )
        );
    }

    #[test]
    fn a_changed_plan_keeps_what_is_written_around_the_change() {
        let saved = changed(WRITTEN, |parts| {
            plan_mut(parts, "home").label = Some("demo-laptop home".to_owned());
        });
        assert_eq!(
            saved,
            Some(WRITTEN.replace(
                "jitter = \"5 minutes\" }\n",
                "jitter = \"5 minutes\" }\nlabel = \"demo-laptop home\"\n"
            ))
        );
    }

    /// The plan `docs`, keeping its last 3 snapshots, and its text in the
    /// daemon's layout.
    fn docs() -> Plan {
        let mut docs = Plan::new(
            plan_id("docs"),
            alias("nas"),
            vec![Source::parse("/home/demo/docs").expect("a source")],
        );
        docs.retention = RetentionPolicy {
            keep_last: Some(KeepCount::new(3).expect("a count")),
            ..RetentionPolicy::default()
        };
        docs
    }

    const DOCS: &str = "\n[[plans]]\nid = \"docs\"\nrepository = \"nas\"\nsources = [\"/home/demo/docs\"]\n\n[plans.retention]\nkeep-last = 3\n";

    #[test]
    fn a_plan_a_save_adds_follows_the_last_plan_and_its_retention() {
        let saved = changed(WRITTEN, |parts| parts.plans.push(docs()));
        assert_eq!(saved, Some(format!("{WRITTEN}{DOCS}")));
    }

    #[test]
    fn the_first_plan_a_save_adds_follows_the_last_table() {
        let written = "# By hand.\nschema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"   # the NAS\npassword-env = \"DEMO_PASSWORD\"\n";
        let saved = changed(written, |parts| parts.plans.push(docs()));
        assert_eq!(saved, Some(format!("{written}{DOCS}")));
    }

    /// A file that writes options as tables of their own has more tables
    /// before its plans than the daemon's layout: a plan added keeps its
    /// place after them, and its retention stays with it.
    #[test]
    fn a_plan_added_after_options_written_as_tables_keeps_its_retention() {
        let written = WRITTEN.replace(
            "  options = { timeout = \"60s\" }\n",
            "\n[repositories.nas.options]\ntimeout = \"60s\"\n\n[repositories.nas.options-hot]\ntimeout = \"30s\"\n\n[repositories.nas.options-cold]\ntimeout = \"90s\"\n",
        );
        let saved = changed(&written, |parts| parts.plans.push(docs()));
        assert_eq!(saved, Some(format!("{written}{DOCS}")));
    }

    #[test]
    fn an_option_written_in_a_table_of_its_own_is_changed_there() {
        let written = WRITTEN.replace(
            "  options = { timeout = \"60s\" }\n",
            "\n# Options for the NAS.\n[repositories.nas.options]\ntimeout = \"60s\"   # it is slow\nretries = \"3\"\n",
        );
        let saved = changed(&written, |parts| {
            let nas = parts.repositories.get_mut(&alias("nas")).expect("nas");
            nas.settings
                .options
                .insert("timeout".to_owned(), "90s".to_owned());
        });
        assert_eq!(
            saved,
            Some(written.replace("timeout = \"60s\"   #", "timeout = \"90s\"   #"))
        );
    }

    #[test]
    fn an_entry_written_inline_is_written_inline_again_when_it_changes() {
        let written = "schema = 1\n\n[repositories]\n# The NAS.\nnas = { repository = \"/srv/demo-repo\", password-env = \"DEMO_PASSWORD\" }  # by hand\n";
        let saved = changed(written, |parts| {
            let nas = parts.repositories.get_mut(&alias("nas")).expect("nas");
            nas.maintenance = Maintenance::Local;
        });
        assert_eq!(
            saved.as_deref(),
            Some(
                "schema = 1\n\n[repositories]\n# The NAS.\nnas = { repository = \"/srv/demo-repo\", password-env = \"DEMO_PASSWORD\", maintenance = \"local\" }  # by hand\n"
            )
        );
    }

    #[test]
    fn a_table_a_save_adds_goes_at_the_end_of_the_file() {
        let written = "# By hand.\nschema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"\npassword-env = \"DEMO_PASSWORD\"\n";
        let saved = changed(written, |parts| {
            parts.agent.hostname = Some(Hostname::parse("demo-host").expect("a hostname"));
        });
        assert_eq!(
            saved,
            Some(format!("{written}\n[agent]\nhostname = \"demo-host\"\n"))
        );
    }

    #[test]
    fn a_file_with_crlf_lines_and_a_byte_order_mark_keeps_both() {
        let written = format!("\u{feff}{}", WRITTEN.replace('\n', "\r\n"));
        let saved = changed(&written, |parts| {
            parts.agent.hostname = Some(Hostname::parse("demo-host").expect("a hostname"));
        });
        assert_eq!(
            saved,
            Some(written.replace(
                "nice    =   10\r\n",
                "nice    =   10\r\nhostname = \"demo-host\"\r\n"
            ))
        );
    }

    #[test]
    fn a_file_whose_lines_end_both_ways_is_not_edited() {
        let written = WRITTEN.replacen('\n', "\r\n", 1);
        assert_eq!(changed(&written, |_| {}), None);
    }

    #[test]
    fn a_file_that_is_not_a_configuration_is_not_edited() {
        let config = read(WRITTEN);
        assert_eq!(saved_as("schema = [", &config), None);
        assert_eq!(saved_as("schema = 1\ncolour = \"red\"\n", &config), None);
    }

    /// A file in an older version is the load's to migrate, as a whole.
    #[test]
    fn a_file_in_an_older_version_is_not_edited() {
        let newer = Schemas {
            current: 2,
            steps: vec![|_| Ok(())],
        };
        let config = read(WRITTEN);
        let rendered = render_as(Path::new(FILE), &config, 2).expect("renders");
        assert_eq!(
            saved(Path::new(FILE), WRITTEN, &config, &rendered, &newer),
            None
        );
    }

    /// Plans are in the order of the file, and an edit keeps each where it
    /// is written: plans put in another order can't be edited in.
    #[test]
    fn plans_put_in_another_order_are_not_edited_in() {
        let two = format!(
            "{WRITTEN}\n[[plans]]\nid = \"docs\"\nrepository = \"nas\"\nsources = [\"/home/demo/docs\"]\n"
        );
        assert_eq!(changed(&two, |parts| parts.plans.reverse()), None);
    }

    #[test]
    fn an_edit_larger_than_a_file_may_be_is_not_made() {
        let comment = "#".repeat(usize::try_from(MAX_CONFIG_BYTES).expect("fits") - WRITTEN.len());
        let written = format!("{WRITTEN}{comment}");
        assert!(changed(&written, |_| {}).is_some(), "at the cap");
        let saved = changed(&written, |parts| {
            parts.agent.hostname = Some(Hostname::parse("demo-host").expect("a hostname"));
        });
        assert_eq!(saved, None);
    }
}

#[cfg(test)]
mod plan_tests {
    use std::path::Path;

    use domain::PlanId;

    use super::saved;
    use crate::schema::{CURRENT_SCHEMA, Schemas, parse, render_as};

    /// The plan removed is the one with that ID, with what is written in it,
    /// not the first one written.
    #[test]
    fn a_plan_a_save_removes_goes_with_what_is_written_in_it() {
        let head = "schema = 1\n\n[repositories.nas]\nrepository = \"/srv/demo-repo\"\npassword-env = \"DEMO_PASSWORD\"\n";
        let home = "\n# Home.\n[[plans]]\nid = \"home\"\nrepository = \"nas\"\nsources = [\"/home/demo\"]   # all of it\n";
        let docs = "\n# Docs.\n[[plans]]\nid = \"docs\"\nrepository = \"nas\"\nsources = [\"/home/demo/docs\"]  # docs only\n";
        let written = format!("{head}{home}{docs}");
        let file = Path::new("config.toml");
        let schemas = Schemas::production();
        let read = parse(file, &written, &schemas).expect("parses").config;
        let home_id = PlanId::parse("home").expect("an ID");
        let config = domain::planning::AgentConfig::new(
            read.agent().clone(),
            read.hooks().clone(),
            read.repositories().clone(),
            read.plans()
                .iter()
                .filter(|plan| plan.id != home_id)
                .cloned()
                .collect(),
        )
        .expect("valid");
        let rendered = render_as(file, &config, CURRENT_SCHEMA).expect("renders");
        assert_eq!(
            saved(file, &written, &config, &rendered, &schemas),
            Some(format!("{head}{docs}"))
        );
    }
}
