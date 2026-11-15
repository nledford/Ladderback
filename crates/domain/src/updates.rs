//! Releases of Ladderback and the update notice (technical design §3.1, §5;
//! decision NN). The agent asks for the newest release at most once a day,
//! at a time spread by jitter, and a full release of a higher version than
//! the one it runs is the **available update**, which the header shows.
//!
//! Nothing here reads a clock, draws a random number or asks the network:
//! `now`, the draw and the release are passed in, from the `Clock`, `Random`
//! and `ReleaseFeed` ports.

use std::cmp::Ordering;
use std::fmt;

use crate::execution::next_run::{RandomDraw, share};
use crate::{SignedDuration, Timestamp};

/// The longest version text read, in bytes: no real version comes near it,
/// and a longer one is outside text that isn't a version.
pub const MAX_VERSION_BYTES: usize = 64;

/// How long after one release check the next comes, before its jitter.
pub const CHECK_INTERVAL: SignedDuration = SignedDuration::from_hours(24);

/// How far a release check may be put after its moment, so that agents
/// started together don't all ask at once.
pub const CHECK_JITTER: SignedDuration = SignedDuration::from_hours(1);

/// How long after it starts an agent that has never checked first asks,
/// before the jitter: long enough that a daemon started for a moment, as a
/// test or a quick try does, never asks.
pub const FIRST_CHECK_DELAY: SignedDuration = SignedDuration::from_mins(5);

/// Where a release's notes are read: its page on GitHub, under its tag.
pub const RELEASES_PAGE: &str = "https://github.com/nledford/Ladderback/releases/tag/";

/// A version of Ladderback as SemVer 2.0.0 writes it: `0.3.0`, or
/// `0.4.0-rc.1` for a prerelease. Build metadata (`+3f937cf`) is read and
/// dropped: it plays no part in which version is newer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<Identifier>,
}

/// One dot-separated part of a prerelease: a number, which orders as one,
/// or text, which orders after every number.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Identifier {
    Numeric(u64),
    Text(String),
}

/// The text isn't a SemVer version (or, for a tag, `v` and one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidVersion;

impl Version {
    /// The version `text` is: `MAJOR.MINOR.PATCH`, each a number without a
    /// leading zero, then an optional `-` prerelease and `+` build metadata.
    pub fn parse(text: &str) -> Result<Self, InvalidVersion> {
        if text.len() > MAX_VERSION_BYTES {
            return Err(InvalidVersion);
        }
        let (rest, build) = match text.split_once('+') {
            Some((rest, build)) => (rest, Some(build)),
            None => (text, None),
        };
        if let Some(build) = build
            && !build.split('.').all(is_identifier)
        {
            return Err(InvalidVersion);
        }
        let (core, pre) = match rest.split_once('-') {
            Some((core, pre)) => (
                core,
                pre.split('.')
                    .map(Identifier::parse)
                    .collect::<Result<_, _>>()?,
            ),
            None => (rest, Vec::new()),
        };
        let mut numbers = core.split('.').map(number);
        let (Some(major), Some(minor), Some(patch), None) = (
            numbers.next(),
            numbers.next(),
            numbers.next(),
            numbers.next(),
        ) else {
            return Err(InvalidVersion);
        };
        Ok(Self {
            major: major?,
            minor: minor?,
            patch: patch?,
            pre,
        })
    }

    /// The version a release's tag names: `v` and the version (`v0.3.0`).
    pub fn of_tag(tag: &str) -> Result<Self, InvalidVersion> {
        tag.strip_prefix('v')
            .map_or(Err(InvalidVersion), Self::parse)
    }

    /// Whether it is a prerelease (`0.4.0-rc.1`).
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    /// The tag its release has: `v0.3.0`.
    pub fn tag(&self) -> String {
        format!("v{self}")
    }

    /// The address of its release's notes.
    pub fn release_notes(&self) -> String {
        format!("{RELEASES_PAGE}{}", self.tag())
    }
}

impl Identifier {
    /// A prerelease part: digits without a leading zero are a number; other
    /// letters, digits and hyphens are text.
    fn parse(text: &str) -> Result<Self, InvalidVersion> {
        if text.bytes().all(|byte| byte.is_ascii_digit()) {
            number(text).map(Self::Numeric)
        } else if is_identifier(text) {
            Ok(Self::Text(text.to_owned()))
        } else {
            Err(InvalidVersion)
        }
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Numeric(number) => write!(f, "{number}"),
            Self::Text(text) => f.write_str(text),
        }
    }
}

/// Whether `text` is a SemVer identifier: one or more ASCII letters, digits
/// and hyphens.
fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// A SemVer number: digits that fit a `u64`, without a leading zero.
fn number(text: &str) -> Result<u64, InvalidVersion> {
    let digits = text.bytes().all(|byte| byte.is_ascii_digit());
    if !digits || (text.len() > 1 && text.starts_with('0')) {
        return Err(InvalidVersion);
    }
    text.parse().map_err(|_| InvalidVersion)
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        let mut separator = '-';
        for identifier in &self.pre {
            write!(f, "{separator}{identifier}")?;
            separator = '.';
        }
        Ok(())
    }
}

impl Ord for Version {
    /// SemVer's precedence: the three numbers in turn; then a prerelease
    /// before the release of its numbers, two prereleases by their parts in
    /// turn, a number before text, and fewer parts before more.
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The newest release a release feed told of: its version, and whether it
/// is marked a prerelease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub prerelease: bool,
}

/// The update `release` is for an agent running `running`: its version,
/// when it is a full release (neither marked a prerelease nor with a
/// prerelease part) newer than `running`; `None` otherwise.
pub fn available_update(running: &Version, release: &Release) -> Option<Version> {
    let full = !release.prerelease && !release.version.is_prerelease();
    (full && release.version > *running).then(|| release.version.clone())
}

/// When the agent asks for the newest release next, at `now`, having last
/// asked at `last`: a day after `last`, but never sooner than
/// [`FIRST_CHECK_DELAY`] from now, so an agent that has just started waits
/// that long whenever it last asked; never more than a day from now, should
/// the clock have been set back. Then spread by up to [`CHECK_JITTER`], as
/// the draw falls.
pub fn next_check(last: Option<Timestamp>, now: Timestamp, draw: RandomDraw) -> Timestamp {
    let soon = later(now, FIRST_CHECK_DELAY);
    let moment = last.map_or(soon, |last| {
        later(last, CHECK_INTERVAL)
            .max(soon)
            .min(later(now, CHECK_INTERVAL))
    });
    later(moment, share(CHECK_JITTER, draw))
}

/// `duration` after `moment`, or the last moment there is.
fn later(moment: Timestamp, duration: SignedDuration) -> Timestamp {
    moment.checked_add(duration).unwrap_or(Timestamp::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn time(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn a_version_reads_its_three_numbers() {
        let read = version("12.0.345");
        assert_eq!((read.major, read.minor, read.patch), (12, 0, 345));
        assert!(!read.is_prerelease());
        assert_eq!(read.to_string(), "12.0.345");
    }

    #[test]
    fn a_prerelease_keeps_its_parts_and_drops_build_metadata() {
        let read = version("0.4.0-rc.1+3f937cf");
        assert!(read.is_prerelease());
        assert_eq!(read.to_string(), "0.4.0-rc.1");
        assert_eq!(version("1.0.0+build.7").to_string(), "1.0.0");
        assert_eq!(version("1.0.0-x-y.0a").to_string(), "1.0.0-x-y.0a");
    }

    #[test]
    fn text_that_is_no_version_is_refused() {
        for text in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.2.3-",
            "1.2.3-rc..1",
            "1.2.3-01",
            "1.2.3-rc_1",
            "1.2.3+",
            "1.2.3+a..b",
            "-1.2.3",
            "1.2.x",
            " 1.2.3",
            "v1.2.3",
            "1.2.18446744073709551616",
            "1.2.3-18446744073709551616",
        ] {
            assert_eq!(Version::parse(text), Err(InvalidVersion), "{text:?}");
        }
        assert_eq!(version("0.0.0").to_string(), "0.0.0");
        assert_eq!(version("1.2.3-0").to_string(), "1.2.3-0");
    }

    #[test]
    fn version_text_longer_than_the_cap_is_refused() {
        let at_cap = format!("1.2.3-{}", "a".repeat(MAX_VERSION_BYTES - 6));
        assert_eq!(at_cap.len(), MAX_VERSION_BYTES);
        assert!(Version::parse(&at_cap).is_ok());
        let over = format!("{at_cap}a");
        assert_eq!(Version::parse(&over), Err(InvalidVersion));
    }

    #[test]
    fn a_tag_is_v_and_a_version() {
        assert_eq!(Version::of_tag("v0.3.0"), Ok(version("0.3.0")));
        assert_eq!(Version::of_tag("v0.4.0-rc.1"), Ok(version("0.4.0-rc.1")));
        for tag in [
            "0.3.0",
            "V0.3.0",
            "vv0.3.0",
            "v",
            "nightly",
            "release-0.3.0",
        ] {
            assert_eq!(Version::of_tag(tag), Err(InvalidVersion), "{tag:?}");
        }
        assert_eq!(version("0.3.0").tag(), "v0.3.0");
        assert_eq!(version("0.4.0-rc.1").tag(), "v0.4.0-rc.1");
    }

    #[test]
    fn release_notes_are_the_release_page_of_its_tag() {
        assert_eq!(
            version("0.3.0").release_notes(),
            "https://github.com/nledford/Ladderback/releases/tag/v0.3.0"
        );
    }

    #[test]
    fn versions_order_as_semver_says() {
        // SemVer 2.0.0's own example, in order.
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "1.10.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            let (lower, higher) = (version(pair[0]), version(pair[1]));
            assert_eq!(lower.cmp(&higher), Ordering::Less, "{pair:?}");
            assert_eq!(higher.cmp(&lower), Ordering::Greater, "{pair:?}");
            assert_eq!(lower.partial_cmp(&higher), Some(Ordering::Less), "{pair:?}");
        }
        assert_eq!(version("1.0.0+a").cmp(&version("1.0.0+b")), Ordering::Equal);
        assert_eq!(version("2.0.0").cmp(&version("10.0.0")), Ordering::Less);
        assert_eq!(version("1.2.0").cmp(&version("1.10.0")), Ordering::Less);
        assert_eq!(version("1.0.2").cmp(&version("1.0.10")), Ordering::Less);
    }

    fn release(text: &str, prerelease: bool) -> Release {
        Release {
            version: version(text),
            prerelease,
        }
    }

    #[test]
    fn a_newer_full_release_is_the_available_update() {
        let running = version("0.2.0");
        for newer in ["0.2.1", "0.3.0", "1.0.0"] {
            assert_eq!(
                available_update(&running, &release(newer, false)),
                Some(version(newer)),
                "{newer}"
            );
        }
    }

    #[test]
    fn an_equal_older_or_prerelease_one_is_no_update() {
        let running = version("0.2.0");
        for (text, prerelease) in [
            ("0.2.0", false),
            ("0.1.9", false),
            ("0.3.0-rc.1", false),
            ("0.3.0", true),
        ] {
            assert_eq!(
                available_update(&running, &release(text, prerelease)),
                None,
                "{text} {prerelease}"
            );
        }
    }

    #[test]
    fn a_prerelease_that_runs_is_updated_by_its_release() {
        let running = version("0.3.0-rc.2");
        assert_eq!(
            available_update(&running, &release("0.3.0", false)),
            Some(version("0.3.0"))
        );
        assert_eq!(available_update(&running, &release("0.2.9", false)), None);
    }

    #[test]
    fn an_agent_that_never_asked_asks_soon_after_it_starts() {
        let now = time("2026-06-10T00:00:00Z");
        assert_eq!(
            next_check(None, now, RandomDraw::LOWEST),
            now + FIRST_CHECK_DELAY
        );
        let latest = next_check(None, now, RandomDraw::HIGHEST);
        assert!(latest > now + FIRST_CHECK_DELAY + CHECK_JITTER - SignedDuration::from_secs(1));
        assert!(latest <= now + FIRST_CHECK_DELAY + CHECK_JITTER);
    }

    #[test]
    fn the_next_check_is_a_day_after_the_last() {
        let last = time("2026-06-10T00:00:00Z");
        let now = time("2026-06-10T06:00:00Z");
        assert_eq!(
            next_check(Some(last), now, RandomDraw::LOWEST),
            last + CHECK_INTERVAL
        );
        let latest = next_check(Some(last), now, RandomDraw::HIGHEST);
        assert!(latest > last + CHECK_INTERVAL + CHECK_JITTER - SignedDuration::from_secs(1));
        assert!(latest <= last + CHECK_INTERVAL + CHECK_JITTER);
    }

    #[test]
    fn a_day_that_passed_while_the_agent_was_off_asks_soon() {
        let last = time("2026-06-10T00:00:00Z");
        let now = time("2026-06-12T00:00:00Z");
        assert_eq!(
            next_check(Some(last), now, RandomDraw::LOWEST),
            now + FIRST_CHECK_DELAY
        );
        // An agent started as the day after the last check ends still waits
        // its first delay.
        let end = last + CHECK_INTERVAL;
        assert_eq!(
            next_check(Some(last), end, RandomDraw::LOWEST),
            end + FIRST_CHECK_DELAY
        );
    }

    #[test]
    fn an_agent_started_just_before_its_check_is_due_still_waits_its_first_delay() {
        let last = time("2026-06-09T12:00:00Z");
        let now = time("2026-06-10T11:58:00Z");
        assert_eq!(
            next_check(Some(last), now, RandomDraw::LOWEST),
            now + FIRST_CHECK_DELAY
        );
        // Started earlier, with the first delay ending first, the check comes
        // a day after the last.
        let earlier = time("2026-06-10T11:50:00Z");
        assert_eq!(
            next_check(Some(last), earlier, RandomDraw::LOWEST),
            last + CHECK_INTERVAL
        );
    }

    #[test]
    fn a_clock_set_back_puts_the_next_check_no_more_than_a_day_away() {
        let last = time("2026-06-12T00:00:00Z");
        let now = time("2026-06-10T00:00:00Z");
        assert_eq!(
            next_check(Some(last), now, RandomDraw::LOWEST),
            now + CHECK_INTERVAL
        );
    }
}
