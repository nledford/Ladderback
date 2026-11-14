//! What the build script makes of what it found (decision NN). The build
//! script compiles this file too (`build.rs`), so it needs nothing but `std`;
//! the program's tests test it.

use std::path::Path;

/// What `git` answered about the work tree the build runs in.
pub struct Checkout<'a> {
    /// `git rev-parse --show-toplevel`: the work tree's top folder.
    pub top: &'a str,
    /// `git rev-parse HEAD`.
    pub head: &'a str,
    /// `git tag --points-at HEAD`: one tag a line.
    pub tags: &'a str,
    /// `git status --porcelain --untracked-files=no`: one changed file a
    /// line, none when the tracked files are as the commit has them.
    pub changes: &'a str,
}

/// The build, as the program is told it.
#[derive(Debug, PartialEq, Eq)]
pub struct Found {
    /// The full commit hash, lowercase.
    pub commit: Option<String>,
    /// Whether tracked files differed from the commit.
    pub dirty: bool,
    /// Whether the commit is the one the tag `v<version>` names.
    pub release: bool,
}

/// Whether `text` is a commit hash, whole or short: 7 to 64 hex digits.
pub fn is_commit(text: &str) -> bool {
    todo!()
}

/// The build of `version` from the package folder `package`: the commit a
/// packager `named`, when it is one, built as between releases and without
/// changes; otherwise what `git` said of the `checkout`, when that is the
/// package's own work tree and not one around it (a source archive unpacked
/// inside another repository); otherwise nothing known.
pub fn found(
    version: &str,
    package: &Path,
    named: Option<&str>,
    checkout: Option<&Checkout<'_>>,
) -> Found {
    todo!()
}

/// The build script's instructions that hand `found` to the program, as
/// `LADDERBACK_BUILT_COMMIT` (empty when unknown), `LADDERBACK_BUILT_DIRTY`
/// and `LADDERBACK_BUILT_RELEASE` (`true` or `false`).
pub fn instructions(found: &Found) -> [String; 3] {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90\n";
    const PACKAGE: &str = "/home/demo/ladderback";

    fn checkout<'a>(tags: &'a str, changes: &'a str) -> Checkout<'a> {
        Checkout {
            top: "/home/demo/ladderback\n",
            head: HEAD,
            tags,
            changes,
        }
    }

    fn at(named: Option<&str>, checkout: Option<&Checkout<'_>>) -> Found {
        found("0.3.0", Path::new(PACKAGE), named, checkout)
    }

    fn between_releases(commit: &str) -> Found {
        Found {
            commit: Some(commit.to_owned()),
            dirty: false,
            release: false,
        }
    }

    #[test]
    fn a_commit_is_seven_to_sixty_four_hex_digits() {
        assert!(is_commit("3f937cf"));
        assert!(is_commit(&"a".repeat(64)));
        assert!(is_commit("3F937CF"));
        assert!(!is_commit("3f937c"));
        assert!(!is_commit(&"a".repeat(65)));
        assert!(!is_commit("3f937cg"));
        assert!(!is_commit("3f937cf.dirty"));
        assert!(!is_commit(""));
    }

    #[test]
    fn a_clean_checkout_of_its_release_tag_is_the_release() {
        let tagged = checkout("v0.2.0\nv0.3.0\n", "");
        assert_eq!(
            at(None, Some(&tagged)),
            Found {
                commit: Some("3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90".to_owned()),
                dirty: false,
                release: true,
            }
        );
    }

    #[test]
    fn a_tag_of_another_version_is_not_this_release() {
        for tags in ["v0.2.0\n", "0.3.0\n", "v0.3.0-rc.1\n", "v0.3.00\n", ""] {
            assert_eq!(
                at(None, Some(&checkout(tags, ""))),
                between_releases("3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90"),
                "{tags:?}"
            );
        }
    }

    #[test]
    fn a_changed_tracked_file_marks_the_build_dirty() {
        let changed = checkout("v0.3.0\n", " M src/main.rs\n");
        assert_eq!(
            at(None, Some(&changed)),
            Found {
                commit: Some("3f937cf0a1b2c3d4e5f60718293a4b5c6d7e8f90".to_owned()),
                dirty: true,
                release: true,
            }
        );
    }

    #[test]
    fn a_work_tree_around_the_package_is_not_its_checkout() {
        let around = Checkout {
            top: "/home/demo\n",
            ..checkout("v0.3.0\n", "")
        };
        assert_eq!(
            at(None, Some(&around)),
            Found {
                commit: None,
                dirty: false,
                release: false,
            }
        );
    }

    #[test]
    fn a_head_that_is_not_a_commit_is_unknown() {
        let odd = Checkout {
            head: "HEAD\n",
            ..checkout("", " M src/main.rs\n")
        };
        assert_eq!(
            at(None, Some(&odd)),
            Found {
                commit: None,
                dirty: false,
                release: false,
            }
        );
    }

    #[test]
    fn without_git_nothing_is_known() {
        assert_eq!(
            at(None, None),
            Found {
                commit: None,
                dirty: false,
                release: false,
            }
        );
    }

    #[test]
    fn a_packagers_commit_wins_over_git_and_is_between_releases() {
        let tagged = checkout("v0.3.0\n", " M src/main.rs\n");
        assert_eq!(
            at(Some("ABCDEF1234"), Some(&tagged)),
            between_releases("abcdef1234")
        );
        assert_eq!(at(Some("abcdef1"), None), between_releases("abcdef1"));
    }

    #[test]
    fn a_packagers_value_that_is_not_a_commit_is_set_aside() {
        let tagged = checkout("v0.3.0\n", "");
        for named in ["", "main", "v0.3.0", "abc"] {
            assert_eq!(
                at(Some(named), Some(&tagged)),
                at(None, Some(&tagged)),
                "{named:?}"
            );
        }
    }

    #[test]
    fn the_instructions_hand_over_each_field() {
        let found = Found {
            commit: Some("abcdef1".to_owned()),
            dirty: true,
            release: false,
        };
        assert_eq!(
            instructions(&found),
            [
                "cargo:rustc-env=LADDERBACK_BUILT_COMMIT=abcdef1".to_owned(),
                "cargo:rustc-env=LADDERBACK_BUILT_DIRTY=true".to_owned(),
                "cargo:rustc-env=LADDERBACK_BUILT_RELEASE=false".to_owned(),
            ]
        );
        let unknown = Found {
            commit: None,
            dirty: false,
            release: true,
        };
        assert_eq!(
            instructions(&unknown),
            [
                "cargo:rustc-env=LADDERBACK_BUILT_COMMIT=".to_owned(),
                "cargo:rustc-env=LADDERBACK_BUILT_DIRTY=false".to_owned(),
                "cargo:rustc-env=LADDERBACK_BUILT_RELEASE=true".to_owned(),
            ]
        );
    }
}
