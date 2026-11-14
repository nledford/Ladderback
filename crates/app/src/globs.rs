//! A plan's globs that can never match, and what to write instead (#510):
//! the domain finds them against the sources as the backup walks them, and
//! for a plan of several sources the disk tells which one a glob was
//! written from. The import, the daemon's start and the plans list all say
//! them so.

use domain::planning::{Placement, Plan};

use crate::ports::NormalizeSources;

/// A glob of a plan that can never match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeverMatching {
    /// The glob as written, its `!` included.
    pub glob: String,
    /// The whole path to write instead, under the source the glob is for;
    /// none when no one source is (of several, none or more than one holds
    /// its folders).
    pub absolute: Option<String>,
}

/// The globs of `plan` that can never match, `globs` then `iglobs`, each in
/// the order written, read against its sources as `sources` walks them.
pub async fn never_matching_globs(
    plan: &Plan,
    sources: &dyn NormalizeSources,
) -> Vec<NeverMatching> {
    let _ = (plan, sources);
    todo!()
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "test code")]
mod tests {
    use async_trait::async_trait;
    use domain::planning::Source;
    use domain::{PlanId, RepositoryAlias};

    use super::*;
    use crate::ports::SourcesError;

    /// A disk that holds these paths and every folder above them, each as
    /// written.
    struct Disk(&'static [&'static str]);

    #[async_trait]
    impl NormalizeSources for Disk {
        async fn normalized(&self, sources: &[Source]) -> Result<Vec<Source>, SourcesError> {
            sources
                .iter()
                .map(|source| {
                    let path = source.as_str().trim_end_matches('/');
                    self.0
                        .iter()
                        .any(|held| {
                            held.strip_prefix(path)
                                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
                        })
                        .then(|| source.clone())
                        .ok_or(SourcesError::Unreadable {
                            detail: "not there".to_owned(),
                        })
                })
                .collect()
        }
    }

    fn plan(sources: &[&str], globs: &[&str]) -> Plan {
        let mut plan = Plan::new(
            PlanId::parse("home").expect("an ID"),
            RepositoryAlias::parse("nas").expect("an alias"),
            sources
                .iter()
                .map(|source| Source::parse(source).expect("a source"))
                .collect(),
        );
        plan.filter.globs = globs.iter().map(|glob| (*glob).to_owned()).collect();
        plan
    }

    fn said(glob: &str, absolute: Option<&str>) -> NeverMatching {
        NeverMatching {
            glob: glob.to_owned(),
            absolute: absolute.map(str::to_owned),
        }
    }

    const SOURCES: &[&str] = &["/home/user", "/srv/disk"];

    #[tokio::test]
    async fn of_several_sources_the_one_that_holds_the_folders_is_the_glob_s() {
        let disk = Disk(&["/home/user/docs", "/srv/disk/docker/app/cache/x"]);
        let plan = plan(
            SOURCES,
            &["!docker/app/cache", "!docker/*/cache", "!./docs/a/"],
        );
        assert_eq!(
            never_matching_globs(&plan, &disk).await,
            [
                said("!docker/app/cache", Some("!/srv/disk/docker/app/cache")),
                said("!docker/*/cache", Some("!/srv/disk/docker/*/cache")),
                said("!./docs/a/", None),
            ]
        );
    }

    #[tokio::test]
    async fn of_several_sources_none_or_two_that_hold_the_folders_say_no_path() {
        let both = Disk(&["/home/user/docker/app/cache", "/srv/disk/docker/app/cache"]);
        let neither = Disk(&["/home/user", "/srv/disk"]);
        let plan = plan(SOURCES, &["!docker/app/cache"]);
        for disk in [both, neither] {
            assert_eq!(
                never_matching_globs(&plan, &disk).await,
                [said("!docker/app/cache", None)],
                "{:?}",
                disk.0
            );
        }
    }

    #[tokio::test]
    async fn the_only_source_needs_no_look_at_the_disk() {
        let empty = Disk(&[]);
        let one = plan(&["/home/user"], &["!docker/app/cache"]);
        assert_eq!(
            never_matching_globs(&one, &empty).await,
            [said(
                "!docker/app/cache",
                Some("!/home/user/docker/app/cache")
            )]
        );
    }
}
