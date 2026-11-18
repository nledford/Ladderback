//! The plan dry run's rules (§16, decision 89, point 13): the folders a dry
//! run may read (the dry-run allowlist), what of a plan's source filter
//! counts as one exclude, and how often one proposer may dry-run.

use std::collections::VecDeque;

use jiff::{SignedDuration, Timestamp};

use super::names::is_absolute_path;
use super::{InvalidValue, SourceFilter, ValueKind};

/// The most folders the dry-run allowlist holds.
pub const MAX_DRY_RUN_FOLDERS: usize = 64;

/// The most dry runs one proposer starts in [`DRY_RUN_WINDOW`].
pub const MAX_DRY_RUNS_PER_HOUR: usize = 20;

/// The window [`MAX_DRY_RUNS_PER_HOUR`] counts over.
pub const DRY_RUN_WINDOW: SignedDuration = SignedDuration::from_hours(1);

/// One folder of the dry-run allowlist, as written: an absolute path. It is
/// resolved against the disk (its links followed, its `..` taken away) only
/// when a dry run checks a source against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DryRunFolder(String);

impl DryRunFolder {
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        if is_absolute_path(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidValue::new(ValueKind::DryRunFolder, text))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The folders a plan dry run may read, which a person sets on the Settings
/// page; none by default, so no dry run reads anything until a person
/// lists a folder. A proposal never changes it (decision 89, point 1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DryRunAllowlist(Vec<DryRunFolder>);

impl DryRunAllowlist {
    /// `folders`, in the order given, unless there are more than
    /// [`MAX_DRY_RUN_FOLDERS`]: the first one beyond is refused.
    pub fn new(folders: Vec<DryRunFolder>) -> Result<Self, InvalidValue> {
        match folders.get(MAX_DRY_RUN_FOLDERS) {
            Some(beyond) => Err(InvalidValue::new(ValueKind::DryRunFolder, beyond.as_str())),
            None => Ok(Self(folders)),
        }
    }

    pub fn folders(&self) -> &[DryRunFolder] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Whether `path` is `folder` or lies under it, both resolved, compared
/// component by component: `/srv/demo` holds `/srv/demo/a`, never
/// `/srv/demo-2`. The root folder holds every path.
pub fn is_within(path: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches('/');
    match path.strip_prefix(folder) {
        Some(rest) => rest.is_empty() || rest.starts_with('/'),
        None => false,
    }
}

/// Whether `name` is one file's name alone, as `exclude-if-present` means
/// its markers: the walk looks for each in every folder it reads, so a
/// name with a `/` (an absolute path above all) would read a file
/// anywhere, outside the allowlist too.
pub fn is_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/')
}

/// One exclude of a plan's source filter: the setting it is written under,
/// as the configuration file names it, and its place in that setting's
/// list from 0, none for a switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExcludeSetting {
    pub setting: &'static str,
    pub index: Option<usize>,
}

impl SourceFilter {
    /// Each exclude of this filter, in the order of its settings and of
    /// each setting's list, with a filter that holds that exclude alone: a
    /// dry run counts what each removes on its own. `no-require-git` goes
    /// with `git-ignore`, which it changes, and alone is no exclude.
    pub fn excludes(&self) -> Vec<(ExcludeSetting, SourceFilter)> {
        let mut each = Vec::new();
        let mut list = |setting: &'static str, count: usize, alone: &dyn Fn(usize) -> Self| {
            for index in 0..count {
                each.push((
                    ExcludeSetting {
                        setting,
                        index: Some(index),
                    },
                    alone(index),
                ));
            }
        };
        let one = |items: &[String], index: usize| items.get(index).cloned().into_iter().collect();
        list("globs", self.globs.len(), &|index| Self {
            globs: one(&self.globs, index),
            ..Self::default()
        });
        list("iglobs", self.iglobs.len(), &|index| Self {
            iglobs: one(&self.iglobs, index),
            ..Self::default()
        });
        list("glob-files", self.glob_files.len(), &|index| Self {
            glob_files: self.glob_files.get(index).cloned().into_iter().collect(),
            ..Self::default()
        });
        list("iglob-files", self.iglob_files.len(), &|index| Self {
            iglob_files: self.iglob_files.get(index).cloned().into_iter().collect(),
            ..Self::default()
        });
        list(
            "custom-ignorefiles",
            self.custom_ignorefiles.len(),
            &|index| Self {
                custom_ignorefiles: one(&self.custom_ignorefiles, index),
                ..Self::default()
            },
        );
        list(
            "exclude-if-present",
            self.exclude_if_present.len(),
            &|index| Self {
                exclude_if_present: one(&self.exclude_if_present, index),
                ..Self::default()
            },
        );
        list("exclude-if-xattr", self.exclude_if_xattr.len(), &|index| {
            Self {
                exclude_if_xattr: one(&self.exclude_if_xattr, index),
                ..Self::default()
            }
        });
        let switch = |setting: &'static str, alone: Self| {
            (
                ExcludeSetting {
                    setting,
                    index: None,
                },
                alone,
            )
        };
        if self.git_ignore {
            each.push(switch(
                "git-ignore",
                Self {
                    git_ignore: true,
                    no_require_git: self.no_require_git,
                    ..Self::default()
                },
            ));
        }
        if self.one_file_system {
            each.push(switch(
                "one-file-system",
                Self {
                    one_file_system: true,
                    ..Self::default()
                },
            ));
        }
        each
    }

    /// The first setting of this filter for which rustic's walker reads
    /// files in the folders above the sources, outside every folder a dry
    /// run may read, or none: it looks for each `custom-ignorefiles` name
    /// and, with `git-ignore`, each `.gitignore` in every folder up to the
    /// root, and with `no-require-git` applies git's global excludes file.
    /// A dry run refuses a filter that has one.
    pub fn reads_above(&self) -> Option<&'static str> {
        [
            ("custom-ignorefiles", !self.custom_ignorefiles.is_empty()),
            ("git-ignore", self.git_ignore),
            ("no-require-git", self.no_require_git),
        ]
        .into_iter()
        .find_map(|(setting, set)| set.then_some(setting))
    }
}

/// When one proposer started its dry runs in the last [`DRY_RUN_WINDOW`],
/// the oldest first: at most [`MAX_DRY_RUNS_PER_HOUR`] of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DryRunRecord(VecDeque<Timestamp>);

impl DryRunRecord {
    /// Records a dry run started at `now`, unless the proposer started
    /// [`MAX_DRY_RUNS_PER_HOUR`] in the [`DRY_RUN_WINDOW`] before it: then
    /// nothing is recorded, and the answer is when it may start one again.
    pub fn start(&mut self, now: Timestamp) -> Result<(), Timestamp> {
        while self
            .0
            .front()
            .is_some_and(|started| now.duration_since(*started) >= DRY_RUN_WINDOW)
        {
            self.0.pop_front();
        }
        if self.0.len() >= MAX_DRY_RUNS_PER_HOUR {
            let oldest = self.0.front().copied().unwrap_or(now);
            return Err(oldest
                .saturating_add(DRY_RUN_WINDOW)
                .unwrap_or(Timestamp::MAX));
        }
        self.0.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::GlobFile;

    #[test]
    fn a_folder_holds_itself_and_what_lies_under_it_by_whole_components() {
        assert!(is_within("/srv/demo", "/srv/demo"));
        assert!(is_within("/srv/demo/a/b", "/srv/demo"));
        assert!(is_within("/srv/demo/a", "/srv/demo/"));
        assert!(is_within("/srv/demo", "/"));
        assert!(!is_within("/srv/demo-2", "/srv/demo"));
        assert!(!is_within("/srv", "/srv/demo"));
        assert!(!is_within("/other/demo", "/srv/demo"));
    }

    #[test]
    fn a_file_name_is_one_name_alone() {
        assert!(is_file_name(".gitignore"));
        assert!(is_file_name("CACHEDIR.TAG"));
        for name in ["", ".", "..", "/etc/passwd", "../.ignore", "a/b", "a/"] {
            assert!(!is_file_name(name), "{name:?}");
        }
    }

    #[test]
    fn an_allowlist_folder_is_an_absolute_path() {
        assert_eq!(
            DryRunFolder::parse("/srv/demo").map(|folder| folder.as_str().to_owned()),
            Ok("/srv/demo".to_owned())
        );
        for text in ["srv/demo", "~/demo", "", "/with\0nul"] {
            assert_eq!(
                DryRunFolder::parse(text),
                Err(InvalidValue::new(ValueKind::DryRunFolder, text)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_allowlist_holds_at_most_its_limit_of_folders() {
        let folder = |n: usize| DryRunFolder::parse(&format!("/srv/{n}")).expect("a folder");
        let full: Vec<_> = (0..MAX_DRY_RUN_FOLDERS).map(folder).collect();
        let allowlist = DryRunAllowlist::new(full.clone()).expect("at the limit");
        assert_eq!(allowlist.folders(), full.as_slice());
        assert!(!allowlist.is_empty(), "it lists folders");
        let mut over = full;
        over.push(folder(MAX_DRY_RUN_FOLDERS));
        assert_eq!(
            DryRunAllowlist::new(over),
            Err(InvalidValue::new(
                ValueKind::DryRunFolder,
                format!("/srv/{MAX_DRY_RUN_FOLDERS}")
            ))
        );
        assert!(DryRunAllowlist::default().is_empty());
    }

    #[test]
    fn each_exclude_comes_alone_in_the_order_of_its_settings() {
        let glob_file = |text: &str| GlobFile::parse(text).expect("a glob file");
        let filter = SourceFilter {
            globs: vec!["!a".to_owned(), "!b".to_owned()],
            iglobs: vec!["!c".to_owned()],
            glob_files: vec![glob_file("/srv/g")],
            iglob_files: vec![glob_file("/srv/i")],
            git_ignore: true,
            no_require_git: true,
            custom_ignorefiles: vec![".ignore".to_owned()],
            exclude_if_present: vec![".nobackup".to_owned()],
            exclude_if_xattr: vec!["user.skip".to_owned()],
            one_file_system: true,
        };
        let setting = |setting, index| ExcludeSetting { setting, index };
        let alone = |change: &dyn Fn(&mut SourceFilter)| {
            let mut filter = SourceFilter::default();
            change(&mut filter);
            filter
        };
        assert_eq!(
            filter.excludes(),
            [
                (
                    setting("globs", Some(0)),
                    alone(&|f| f.globs = vec!["!a".to_owned()])
                ),
                (
                    setting("globs", Some(1)),
                    alone(&|f| f.globs = vec!["!b".to_owned()])
                ),
                (
                    setting("iglobs", Some(0)),
                    alone(&|f| f.iglobs = vec!["!c".to_owned()])
                ),
                (
                    setting("glob-files", Some(0)),
                    alone(&|f| f.glob_files = vec![glob_file("/srv/g")])
                ),
                (
                    setting("iglob-files", Some(0)),
                    alone(&|f| f.iglob_files = vec![glob_file("/srv/i")])
                ),
                (
                    setting("custom-ignorefiles", Some(0)),
                    alone(&|f| f.custom_ignorefiles = vec![".ignore".to_owned()])
                ),
                (
                    setting("exclude-if-present", Some(0)),
                    alone(&|f| f.exclude_if_present = vec![".nobackup".to_owned()])
                ),
                (
                    setting("exclude-if-xattr", Some(0)),
                    alone(&|f| f.exclude_if_xattr = vec!["user.skip".to_owned()])
                ),
                (
                    setting("git-ignore", None),
                    alone(&|f| {
                        f.git_ignore = true;
                        f.no_require_git = true;
                    })
                ),
                (
                    setting("one-file-system", None),
                    alone(&|f| f.one_file_system = true)
                ),
            ]
        );
    }

    #[test]
    fn no_require_git_alone_and_an_empty_filter_have_no_exclude() {
        assert_eq!(SourceFilter::default().excludes(), []);
        let filter = SourceFilter {
            no_require_git: true,
            ..SourceFilter::default()
        };
        assert_eq!(filter.excludes(), []);
    }

    #[test]
    fn a_filter_that_reads_above_its_sources_names_its_first_such_setting() {
        let mut filter = SourceFilter {
            globs: vec!["!a".to_owned()],
            exclude_if_present: vec![".nobackup".to_owned()],
            exclude_if_xattr: vec!["user.skip".to_owned()],
            one_file_system: true,
            ..SourceFilter::default()
        };
        assert_eq!(filter.reads_above(), None);
        filter.no_require_git = true;
        assert_eq!(filter.reads_above(), Some("no-require-git"));
        filter.git_ignore = true;
        assert_eq!(filter.reads_above(), Some("git-ignore"));
        filter.custom_ignorefiles = vec![".ignore".to_owned()];
        assert_eq!(filter.reads_above(), Some("custom-ignorefiles"));
    }

    #[test]
    fn a_proposer_starts_its_limit_of_dry_runs_an_hour_then_waits_for_the_oldest() {
        let start = Timestamp::from_second(1_800_000_000).expect("an instant");
        let at = |seconds: i64| start + SignedDuration::from_secs(seconds);
        let mut record = DryRunRecord::default();
        for n in 0..MAX_DRY_RUNS_PER_HOUR {
            assert_eq!(record.start(at(i64::try_from(n).expect("small"))), Ok(()));
        }
        let full = at(100);
        assert_eq!(record.start(full), Err(start + DRY_RUN_WINDOW));
        // A refusal records nothing: the oldest still decides.
        assert_eq!(record.start(full), Err(start + DRY_RUN_WINDOW));
        // An instant before the hour is up, still full; at it, one more.
        assert_eq!(
            record.start(start + DRY_RUN_WINDOW - SignedDuration::from_secs(1)),
            Err(start + DRY_RUN_WINDOW)
        );
        assert_eq!(record.start(start + DRY_RUN_WINDOW), Ok(()));
        assert_eq!(
            record.start(start + DRY_RUN_WINDOW),
            Err(at(1) + DRY_RUN_WINDOW)
        );
    }
}
