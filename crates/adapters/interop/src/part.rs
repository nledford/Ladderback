//! What one profile gives, typed, and the way rustic merges what a profile
//! uses into it (`conflate` in `rustic-rs` 0.11.4): the profile's own value
//! wins; a list the profile leaves empty is the used one's, and a list it
//! sets replaces the used one; a switch either turns on is on; hooks and
//! `use-profiles` are appended, the profile's own first; `options` are
//! merged key by key, the profile's own key winning; and the snapshot entries
//! are put together, sorted by name and then sources, and a repeat of one is
//! dropped, the first kept.

use std::collections::BTreeMap;

use app::ports::Position;
use domain::planning::{GroupBy, HookEntry, Hooks, Hostname, RetentionPolicy, Source};
use domain::secrets::CommandLine;

/// What one file gave: every setting of it, and what it left out.
#[derive(Default)]
pub(crate) struct Part {
    pub global: Global,
    pub repository: Repository,
    pub backup: Settings,
    pub entries: Vec<Entry>,
    pub forget: Forget,
    /// The paths of the keys that were left out, in the order they were met.
    pub left_out: Vec<String>,
    /// The names in `use-profiles`, each with where it is written. Not merged:
    /// each is resolved where it is.
    pub uses: Vec<Use>,
}

/// A name in `use-profiles`.
pub(crate) struct Use {
    pub name: String,
    /// `global.use-profiles[0]`.
    pub key: String,
    pub at: Option<Position>,
}

#[derive(Default)]
pub(crate) struct Global {
    pub group_by: Option<GroupBy>,
    pub hooks: Hooks,
    /// Whether `[global.env]` holds anything.
    pub env: bool,
}

#[derive(Default)]
pub(crate) struct Repository {
    pub repository: Option<String>,
    pub repo_hot: Option<String>,
    pub options: BTreeMap<String, String>,
    pub options_hot: BTreeMap<String, String>,
    pub options_cold: BTreeMap<String, String>,
    pub password: Option<String>,
    pub password_file: Option<String>,
    pub password_command: Option<CommandLine>,
    pub hooks: Hooks,
}

/// The settings `[backup]` and a snapshot entry both have.
#[derive(Clone, Default)]
pub(crate) struct Settings {
    pub label: Option<String>,
    pub host: Option<Hostname>,
    pub tags: Vec<String>,
    pub globs: Vec<String>,
    pub iglobs: Vec<String>,
    pub glob_files: Vec<String>,
    pub iglob_files: Vec<String>,
    pub git_ignore: bool,
    pub no_require_git: bool,
    pub custom_ignorefiles: Vec<String>,
    pub exclude_if_present: Vec<String>,
    pub exclude_if_xattr: Vec<String>,
    pub one_file_system: bool,
    pub hooks: Hooks,
}

#[derive(Clone)]
pub(crate) struct Entry {
    pub name: Option<String>,
    pub sources: Vec<Source>,
    pub settings: Settings,
}

#[derive(Default)]
pub(crate) struct Forget {
    pub group_by: Option<GroupBy>,
    pub prune: bool,
    pub retention: RetentionPolicy,
}

impl Part {
    /// Merges `used`, a profile this one uses, into it.
    pub(crate) fn merge(&mut self, used: Self) {
        self.global.merge(used.global);
        self.repository.merge(used.repository);
        self.backup.merge(used.backup);
        self.forget.merge(used.forget);
        self.entries.extend(used.entries);
        sort_and_dedup(&mut self.entries);
        for key in used.left_out {
            if !self.left_out.contains(&key) {
                self.left_out.push(key);
            }
        }
    }
}

/// Entries as rustic's `merge_snapshots` leaves them: sorted by name and then
/// sources, the sort stable, and of two with the same name and sources the
/// first one kept.
pub(crate) fn sort_and_dedup(entries: &mut Vec<Entry>) {
    entries.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.sources.cmp(&right.sources))
    });
    entries
        .dedup_by(|later, earlier| later.name == earlier.name && later.sources == earlier.sources);
}

impl Global {
    fn merge(&mut self, used: Self) {
        self.group_by = self.group_by.or(used.group_by);
        append_hooks(&mut self.hooks, used.hooks);
        self.env |= used.env;
    }
}

impl Repository {
    fn merge(&mut self, used: Self) {
        take_if_none(&mut self.repository, used.repository);
        take_if_none(&mut self.repo_hot, used.repo_hot);
        take_if_none(&mut self.password, used.password);
        take_if_none(&mut self.password_file, used.password_file);
        take_if_none(&mut self.password_command, used.password_command);
        merge_options(&mut self.options, used.options);
        merge_options(&mut self.options_hot, used.options_hot);
        merge_options(&mut self.options_cold, used.options_cold);
        append_hooks(&mut self.hooks, used.hooks);
    }
}

impl Settings {
    /// Merges `used` into these settings. Hooks are appended, which is right
    /// for the profiles one profile uses; the hooks of an entry and of the
    /// backup section are put together another way.
    pub(crate) fn merge(&mut self, used: Self) {
        take_if_none(&mut self.label, used.label);
        take_if_none(&mut self.host, used.host);
        replace_if_empty(&mut self.tags, used.tags);
        replace_if_empty(&mut self.globs, used.globs);
        replace_if_empty(&mut self.iglobs, used.iglobs);
        replace_if_empty(&mut self.glob_files, used.glob_files);
        replace_if_empty(&mut self.iglob_files, used.iglob_files);
        self.git_ignore |= used.git_ignore;
        self.no_require_git |= used.no_require_git;
        replace_if_empty(&mut self.custom_ignorefiles, used.custom_ignorefiles);
        replace_if_empty(&mut self.exclude_if_present, used.exclude_if_present);
        replace_if_empty(&mut self.exclude_if_xattr, used.exclude_if_xattr);
        self.one_file_system |= used.one_file_system;
        append_hooks(&mut self.hooks, used.hooks);
    }
}

impl Forget {
    fn merge(&mut self, used: Self) {
        self.group_by = self.group_by.or(used.group_by);
        self.prune |= used.prune;
        let own = &mut self.retention;
        let used = used.retention;
        own.keep_last = own.keep_last.or(used.keep_last);
        own.keep_minutely = own.keep_minutely.or(used.keep_minutely);
        own.keep_hourly = own.keep_hourly.or(used.keep_hourly);
        own.keep_daily = own.keep_daily.or(used.keep_daily);
        own.keep_weekly = own.keep_weekly.or(used.keep_weekly);
        own.keep_monthly = own.keep_monthly.or(used.keep_monthly);
        own.keep_quarter_yearly = own.keep_quarter_yearly.or(used.keep_quarter_yearly);
        own.keep_half_yearly = own.keep_half_yearly.or(used.keep_half_yearly);
        own.keep_yearly = own.keep_yearly.or(used.keep_yearly);
        own.keep_within = own.keep_within.or(used.keep_within);
        own.keep_within_minutely = own.keep_within_minutely.or(used.keep_within_minutely);
        own.keep_within_hourly = own.keep_within_hourly.or(used.keep_within_hourly);
        own.keep_within_daily = own.keep_within_daily.or(used.keep_within_daily);
        own.keep_within_weekly = own.keep_within_weekly.or(used.keep_within_weekly);
        own.keep_within_monthly = own.keep_within_monthly.or(used.keep_within_monthly);
        own.keep_within_quarter_yearly = own
            .keep_within_quarter_yearly
            .or(used.keep_within_quarter_yearly);
        own.keep_within_half_yearly = own.keep_within_half_yearly.or(used.keep_within_half_yearly);
        own.keep_within_yearly = own.keep_within_yearly.or(used.keep_within_yearly);
    }
}

fn take_if_none<T>(own: &mut Option<T>, used: Option<T>) {
    if own.is_none() {
        *own = used;
    }
}

fn replace_if_empty<T>(own: &mut Vec<T>, used: Vec<T>) {
    if own.is_empty() {
        *own = used;
    }
}

fn merge_options(own: &mut BTreeMap<String, String>, used: BTreeMap<String, String>) {
    for (key, value) in used {
        own.entry(key).or_insert(value);
    }
}

fn append_hooks(own: &mut Hooks, used: Hooks) {
    append(&mut own.run_before, used.run_before);
    append(&mut own.run_after, used.run_after);
    append(&mut own.run_failed, used.run_failed);
    append(&mut own.run_finally, used.run_finally);
}

fn append(own: &mut Vec<HookEntry>, used: Vec<HookEntry>) {
    own.extend(used);
}
