//! What a plan leaves out of its backup (technical design §6.1): rustic's
//! `Excludes` and `LocalSourceFilterOptions`, under the keys rustic's
//! profiles use.
//!
//! Left out on purpose: `exclude-larger-than`, which would drop a file from a
//! snapshot with no warning, for a size the user set once and forgot.

use super::{GlobFile, Source};

/// The patterns, marker files and switches that decide what a plan's
/// sources contribute. Every list is in the order it was written, and
/// nothing here is read from disk: a glob file that doesn't exist is a
/// failure when the plan runs, not a refusal here.
///
/// The whole of it is optional: the default leaves nothing out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceFilter {
    /// Glob patterns; a leading `!` excludes (rustic's `globs`).
    pub globs: Vec<String>,
    /// The same, case-insensitively (`iglobs`).
    pub iglobs: Vec<String>,
    /// Files that hold glob patterns, one per line (`glob-files`). Each is an
    /// absolute path, so it names the same file however the daemon started.
    pub glob_files: Vec<GlobFile>,
    /// The same, case-insensitively (`iglob-files`).
    pub iglob_files: Vec<GlobFile>,
    /// Leave out what `.gitignore` files leave out (`git-ignore`).
    pub git_ignore: bool,
    /// Apply `.gitignore` files outside a git repository too
    /// (`no-require-git`).
    pub no_require_git: bool,
    /// File names to treat like `.gitignore` (`custom-ignorefiles`).
    pub custom_ignorefiles: Vec<String>,
    /// Leave out a folder that holds a file of this name (`exclude-if-present`).
    pub exclude_if_present: Vec<String>,
    /// Leave out what has this extended attribute (`exclude-if-xattr`).
    pub exclude_if_xattr: Vec<String>,
    /// Don't cross into other file systems (`one-file-system`).
    pub one_file_system: bool,
}

/// A glob of a plan that can never match (#510). rustic matches its globs
/// with gitignore's rules against the whole path a backup walks, from the
/// root of the file system, so a glob with a slash in the middle is read
/// from there: written from a source (`!.local/share/containers`), as a
/// profile run from inside its source folder may have it, it matches
/// nothing. Without such a slash, a glob matches at any depth (`!.cache`);
/// one that starts with `/` or `**/` is read as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeverMatchingGlob<'a> {
    /// The glob as written, its `!` included.
    pub glob: &'a str,
    /// Whether it is an iglob, which names folders in any case.
    case_insensitive: bool,
    /// The plan's sources as written, and as the backup walks them, in the
    /// same order.
    written: &'a [Source],
    walked: &'a [Source],
}

/// Where a glob that can never match is to be written instead (#510), as
/// [`NeverMatchingGlob::placement`] says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// As this whole path, from the root.
    Whole(String),
    /// Under the one source, as the backup walks it, that holds these
    /// folders on disk: the glob's own, written from that source, up to its
    /// first wildcard. [`NeverMatchingGlob::under`] writes it there.
    Holder(String),
}

impl<'a> NeverMatchingGlob<'a> {
    /// Where the glob is to be written instead. One that names a source
    /// from the root as written, through a link the backup follows, goes on
    /// under that source as walked (`!home/user/.cache` is
    /// `!/var/home/user/.cache` where `/home` is a link to `/var/home`). Any
    /// other is read as written from a source: the plan's only one, or,
    /// with several, the one that holds its folders, which only the disk
    /// can tell ([`Placement::Holder`]).
    pub fn placement(&self) -> Placement {
        todo!()
    }

    /// The whole path under the one source of `holding`, the plan's
    /// sources as walked that hold the folders of [`Placement::Holder`];
    /// none when none or several do, as no one source is then the glob's.
    pub fn under(&self, holding: &[&Source]) -> Option<String> {
        let _ = holding;
        todo!()
    }

    /// The glob less its `!`.
    fn rest(&self) -> &'a str {
        self.glob.strip_prefix('!').unwrap_or(self.glob)
    }

    /// The glob's folders, less any empty or `.` one.
    fn parts(&self) -> Vec<&'a str> {
        self.rest()
            .split('/')
            .filter(|part| !matches!(*part, "" | "."))
            .collect()
    }

    /// The glob's folders up to its first wildcard, without the trailing
    /// whitespace rustic doesn't read: what a source must hold for the
    /// glob to be written from it.
    fn folders(&self) -> String {
        let folders: Vec<&str> = self
            .rest()
            .trim_end()
            .split('/')
            .filter(|part| !matches!(*part, "" | "."))
            .take_while(|part| !part.contains(WILDCARDS))
            .collect();
        folders.join("/")
    }

    /// The glob written from the root under `source` as rustic reads it:
    /// its folders, then `tail`, its `!` and a slash at the end, which
    /// marks a folder, kept.
    fn whole(&self, source: &Source, tail: &[&str]) -> String {
        let mut whole = if self.glob.starts_with('!') {
            String::from("!")
        } else {
            String::new()
        };
        for folder in walked_names(source).into_iter().chain(tail.iter().copied()) {
            whole.push('/');
            whole.push_str(folder);
        }
        if self.rest().ends_with('/') {
            whole.push('/');
        }
        whole
    }
}

/// What a glob's part may hold that stands for other names, or escapes
/// one: past it, the part names no folder as written.
const WILDCARDS: [char; 5] = ['*', '?', '[', '{', '\\'];

impl SourceFilter {
    /// The globs that can never match under `walked`, the plan's sources
    /// as the backup walks them, in the order of `written`, the same
    /// sources as written (see [`Plan::never_matching_globs`]): `globs`,
    /// then `iglobs`, each in the order written.
    ///
    /// [`Plan::never_matching_globs`]: super::Plan::never_matching_globs
    pub(super) fn never_matching<'a>(
        &'a self,
        written: &'a [Source],
        walked: &'a [Source],
    ) -> impl Iterator<Item = NeverMatchingGlob<'a>> {
        let _ = (written, walked, &self.globs);
        std::iter::from_fn(|| todo!())
    }
}

/// The names of the folders from the root down to `source`, read as
/// rustic reads a source before it resolves its links: an empty part (a
/// repeated `/`) and `.` name none, and `..` the folder above, the root's
/// being the root. A source that can't be resolved (it isn't there yet)
/// reaches the rule read this way only.
fn walked_names(source: &Source) -> Vec<&str> {
    let mut names = Vec::new();
    for name in source.as_str().split('/') {
        match name {
            "" | "." => {}
            ".." => {
                names.pop();
            }
            name => names.push(name),
        }
    }
    names
}

/// The path rustic matches `glob` against from the root of the file system,
/// when it has a slash in the middle, as gitignore's rules read a line: a
/// comment (`#`) is none; trailing whitespace, a leading `!` and a slash at
/// the end, which marks a folder, are not part of it; one that starts with
/// `/` is written from the root already, and one that starts with `**/`
/// matches at any depth.
fn rooted_path(glob: &str) -> Option<&str> {
    if glob.starts_with('#') {
        return None;
    }
    let line = glob.trim_end();
    let line = line.strip_prefix('!').unwrap_or(line);
    if line.starts_with('/') {
        return None;
    }
    let line = line.strip_suffix('/').unwrap_or(line);
    (line.contains('/') && !line.starts_with("**/")).then_some(line)
}

/// Whether `path`, read from the root, may name the source whose folders
/// are `names`, or a path in it: each of its parts up to the first wildcard
/// or escape names the source's folder at its place, or the source has no
/// more. Only a literal part that differs says it can't, so no glob that
/// may match is said not to.
fn may_reach(path: &str, names: &[&str], case_insensitive: bool) -> bool {
    let mut names = names.iter();
    for part in path.split('/') {
        if part.contains(WILDCARDS) {
            return true;
        }
        match names.next() {
            Some(name) if same(part, name, case_insensitive) => {}
            Some(_) => return false,
            None => return true,
        }
    }
    true
}

/// Whether a glob's literal `part` names `name`, in any case for an iglob.
fn same(part: &str, name: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        part.chars()
            .flat_map(char::to_lowercase)
            .eq(name.chars().flat_map(char::to_lowercase))
    } else {
        part == name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::Plan;
    use crate::{PlanId, RepositoryAlias};

    fn plan(sources: &[&str], globs: &[&str], iglobs: &[&str]) -> Plan {
        let mut plan = Plan::new(
            PlanId::parse("home").unwrap(),
            RepositoryAlias::parse("nas").unwrap(),
            sources
                .iter()
                .map(|source| Source::parse(source).unwrap())
                .collect(),
        );
        plan.filter.globs = globs.iter().map(|glob| (*glob).to_owned()).collect();
        plan.filter.iglobs = iglobs.iter().map(|glob| (*glob).to_owned()).collect();
        plan
    }

    /// The globs of `plan` said to match nothing, as written, with its
    /// sources walked as written.
    fn never_matching(plan: &Plan) -> Vec<&str> {
        plan.never_matching_globs(&plan.sources)
            .map(|glob| glob.glob)
            .collect()
    }

    /// Where each glob of `plan` that matches nothing is to be written
    /// instead, with its sources walked as written.
    fn placements(plan: &Plan) -> Vec<Placement> {
        plan.never_matching_globs(&plan.sources)
            .map(|glob| glob.placement())
            .collect()
    }

    fn whole(path: &str) -> Placement {
        Placement::Whole(path.to_owned())
    }

    fn holder(folders: &str) -> Placement {
        Placement::Holder(folders.to_owned())
    }

    #[test]
    fn a_glob_with_a_slash_in_the_middle_written_from_the_source_never_matches() {
        for glob in [
            "a/b",
            "!a/b",
            "./a/b",
            "!.local/share/containers",
            "a/b/",
            "a/**",
            "a//b",
            "!a/b ",
        ] {
            for (globs, iglobs) in [(&[glob][..], &[][..]), (&[], &[glob])] {
                let plan = plan(&["/home/user"], globs, iglobs);
                assert_eq!(never_matching(&plan), [glob], "{glob:?}");
            }
        }
    }

    #[test]
    fn a_glob_rustic_reads_where_it_is_written_is_not_said_to_match_nothing() {
        for glob in [
            // From the root.
            "/a/b",
            "!/a/b",
            // At any depth.
            "**/a/b",
            "!**/a/b",
            "a",
            "!.cache",
            // A slash at the end only marks a folder.
            "a/",
            "!.cache/",
            "!.cache/ ",
            // A backslash escapes; it doesn't part folders.
            "a\\b",
            // Nothing, and a comment, which rustic skips.
            "",
            "#a/b",
            // The whole path, less its first slash, which rustic reads from
            // the root too, and parts that a wildcard may match.
            "home/user/a",
            "!home/user",
            "*/user/a",
            "home/*/a",
            "home/[u]ser/a",
            "home/us?r/a",
            "{home,srv}/user/a",
            "home/\\user/a",
        ] {
            let plan = plan(&["/home/user"], &[glob], &[glob]);
            assert_eq!(never_matching(&plan), [] as [&str; 0], "{glob:?}");
        }
    }

    #[test]
    fn only_an_iglob_reads_the_source_in_any_case() {
        let plan = plan(&["/home/user"], &["HOME/User/a"], &["HOME/User/b"]);
        assert_eq!(never_matching(&plan), ["HOME/User/a"]);
    }

    #[test]
    fn a_glob_matches_nothing_only_when_it_reaches_none_of_the_sources() {
        let plan = plan(
            &["/home/user", "/srv/data"],
            &["srv/data/a", "srv/other/a", "home/user/a", "home/other/a"],
            &[],
        );
        assert_eq!(never_matching(&plan), ["srv/other/a", "home/other/a"]);
    }

    #[test]
    fn under_the_root_every_glob_reaches_the_source() {
        let plan = plan(&["/"], &["a/b", "!./a/b"], &["a/b"]);
        assert_eq!(never_matching(&plan), [] as [&str; 0]);
    }

    #[test]
    fn the_globs_come_before_the_iglobs_each_in_the_order_written() {
        let plan = plan(&["/home/user"], &["!b/c", "!a/c"], &["!d/c", "!c/c"]);
        assert_eq!(never_matching(&plan), ["!b/c", "!a/c", "!d/c", "!c/c"]);
    }

    #[test]
    fn a_plan_without_sources_has_no_glob_said_to_match_nothing() {
        let plan = plan(&[], &["a/b"], &["a/b"]);
        assert_eq!(never_matching(&plan), [] as [&str; 0]);
    }

    #[test]
    fn the_whole_path_is_the_glob_under_the_only_source_its_exclusion_kept() {
        for (source, glob, path) in [
            (
                "/home/user",
                "!.local/share/containers",
                "!/home/user/.local/share/containers",
            ),
            ("/home/user", "a/b/", "/home/user/a/b/"),
            ("/home/user", "./a/b", "/home/user/a/b"),
            ("/home/user", "!././a/b", "!/home/user/a/b"),
            ("/srv/data/", "!a/b", "!/srv/data/a/b"),
            ("/home/user", "!a/*/b", "!/home/user/a/*/b"),
        ] {
            let plan = plan(&[source], &[glob], &[]);
            assert_eq!(
                placements(&plan),
                [whole(path)],
                "{glob:?} under {source:?}"
            );
        }
    }

    #[test]
    fn a_source_is_read_with_its_dots_and_repeated_slashes_as_rustic_reads_it() {
        for source in [
            "/home/./user",
            "/home/other/../user",
            "/home//user",
            "/home/user/",
            "/../home/user",
        ] {
            let plan = plan(&[source], &["!home/user/.cache/x", "!srv/a"], &[]);
            assert_eq!(never_matching(&plan), ["!srv/a"], "{source:?}");
            assert_eq!(
                placements(&plan),
                [whole("!/home/user/srv/a")],
                "{source:?}"
            );
        }
    }

    #[test]
    fn the_whole_path_is_built_from_the_glob_s_folders_with_no_empty_or_dot_one() {
        for (glob, path) in [
            ("!.//a/b", "!/home/user/a/b"),
            ("a//b", "/home/user/a/b"),
            ("./a/./b", "/home/user/a/b"),
            ("!a/b//", "!/home/user/a/b/"),
            ("./a//b/", "/home/user/a/b/"),
        ] {
            let plan = plan(&["/home/user"], &[glob], &[]);
            assert_eq!(placements(&plan), [whole(path)], "{glob:?}");
        }
    }

    #[test]
    fn with_several_sources_a_glob_naming_none_is_for_the_one_that_holds_its_folders() {
        // Only the disk can tell which source `!docker/app/cache` is
        // written from. The folders it asks for are the glob's own up to its
        // first wildcard, with no empty or `.` one and no trailing
        // whitespace, which rustic doesn't read.
        let plan = plan(
            &["/home/user", "/srv/disk"],
            &["!docker/app/cache", "!./a//b/*/c/", "!x/y ", "!./*/x"],
            &["a/[bc]/d", "a/b?/c"],
        );
        assert_eq!(
            placements(&plan),
            [
                holder("docker/app/cache"),
                holder("a/b"),
                holder("x/y"),
                holder(""),
                holder("a"),
                holder("a"),
            ]
        );
    }

    #[test]
    fn the_whole_path_goes_under_the_one_source_that_holds_the_folders_or_none() {
        let plan = plan(
            &["/home/user", "/srv/disk"],
            &["!docker/app/cache/", "x/y"],
            &[],
        );
        let [home, disk] = [&plan.sources[0], &plan.sources[1]];
        let said = |holding: &[&Source]| -> Vec<Option<String>> {
            plan.never_matching_globs(&plan.sources)
                .map(|glob| glob.under(holding))
                .collect()
        };
        let some = |path: &str| Some(path.to_owned());
        assert_eq!(
            said(&[disk]),
            [some("!/srv/disk/docker/app/cache/"), some("/srv/disk/x/y")]
        );
        assert_eq!(
            said(&[home]),
            [
                some("!/home/user/docker/app/cache/"),
                some("/home/user/x/y")
            ]
        );
        assert_eq!(said(&[home, disk]), [None, None]);
        assert_eq!(said(&[]), [None, None]);
    }

    #[test]
    fn the_globs_are_read_against_the_sources_as_the_backup_walks_them() {
        // `/home` is a link to `/var/home` and `/srv` one to `/mnt`, so the
        // backup walks `/var/home/user` and `/mnt/data/set`: a glob from
        // `home` or `srv` reaches nothing, one from `var/home` does, and
        // the whole path follows the link.
        let plan = plan(
            &["/home/user", "/srv/./data/set"],
            &[
                "!home/user/.cache",
                "!var/home/user/.cache",
                "!.local/x/y",
                "!srv/data/set/tmp/",
                "!srv/data",
                "!HOME/user/.thumbnails",
            ],
            &["!HOME/User/.thumbnails"],
        );
        let walked = [
            Source::parse("/var/home/user").unwrap(),
            Source::parse("/mnt/data/set").unwrap(),
        ];
        let said: Vec<(&str, Placement)> = plan
            .never_matching_globs(&walked)
            .map(|glob| (glob.glob, glob.placement()))
            .collect();
        assert_eq!(
            said,
            [
                // It names the first source as written: under it as walked.
                ("!home/user/.cache", whole("!/var/home/user/.cache")),
                // It names no source: under the one that holds it.
                ("!.local/x/y", holder(".local/x/y")),
                // It names the second source, its dot read.
                ("!srv/data/set/tmp/", whole("!/mnt/data/set/tmp/")),
                // Short of every source it starts like.
                ("!srv/data", holder("srv/data")),
                // A glob names folders in their case, an iglob in any.
                ("!HOME/user/.thumbnails", holder("HOME/user/.thumbnails")),
                (
                    "!HOME/User/.thumbnails",
                    whole("!/var/home/user/.thumbnails")
                ),
            ]
        );
        // The whole path goes under the holder as the backup walks it.
        let local = plan.never_matching_globs(&walked).nth(1).unwrap();
        assert_eq!(
            local.under(&[&walked[1]]),
            Some("!/mnt/data/set/.local/x/y".to_owned())
        );
    }
}
