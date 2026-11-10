//! Reading a profile and the profiles it uses (`use-profiles`), each from a
//! file, within bounds: a file is read to a size cap, the profiles are
//! followed to a depth, no more than a number of them are read, and a profile
//! that uses itself, however far round, is refused.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use app::ports::{ProfileError, ProfileProblem};

use crate::parse::parse;
use crate::part::Part;
use crate::{MAX_ENTRIES, MAX_PROFILE_BYTES, MAX_PROFILES, MAX_USE_DEPTH};

type Loaded<T> = Result<T, ProfileError>;

/// The longest name a used profile may have: a file name's limit on the
/// platforms Ladderback runs on, less the `.toml` it may be given.
const MAX_NAME_BYTES: usize = 250;

/// A profile with everything it uses merged into it.
pub(crate) struct Resolved {
    pub part: Part,
    /// The names in `use-profiles` that no folder had a profile for.
    pub missing: Vec<String>,
}

/// The profile in `file`, and the profiles it uses, found in `search`, first
/// folder first.
pub(crate) fn resolve(file: &Path, search: &[PathBuf]) -> Loaded<Resolved> {
    let mut resolver = Resolver {
        search,
        reads: 0,
        missing: Vec::new(),
    };
    let part = resolver.load(file, 0, &mut Vec::new())?;
    Ok(Resolved {
        part,
        missing: resolver.missing,
    })
}

struct Resolver<'a> {
    search: &'a [PathBuf],
    /// How many profiles were read, a profile used twice twice.
    reads: usize,
    missing: Vec<String>,
}

impl Resolver<'_> {
    /// The profile in `file` with its used profiles merged into it. `depth`
    /// is how far down `use-profiles` this is, and `ancestors` the profiles
    /// that use it, by their canonical paths.
    fn load(&mut self, file: &Path, depth: usize, ancestors: &mut Vec<PathBuf>) -> Loaded<Part> {
        let shown = file.display().to_string();
        let text = read_bounded(file, &shown)?;
        self.reads += 1;
        let mut part = parse(&shown, &text)?;
        let canonical = fs::canonicalize(file).map_err(|_| unreadable(&shown))?;
        ancestors.push(canonical);
        for used in std::mem::take(&mut part.uses) {
            let invalid = |problem| ProfileError::Invalid {
                file: shown.clone(),
                key: Some(used.key.clone()),
                at: used.at,
                problem,
                detail: String::new(),
            };
            if !is_file_name(&used.name) {
                return Err(invalid(ProfileProblem::UsedName));
            }
            let Some(found) = self.find(&used.name)? else {
                if !self.missing.contains(&used.name) {
                    self.missing.push(used.name.clone());
                }
                continue;
            };
            let canonical =
                fs::canonicalize(&found).map_err(|_| unreadable(&found.display().to_string()))?;
            if ancestors.contains(&canonical) {
                return Err(invalid(ProfileProblem::UsedCycle));
            }
            if depth + 1 > MAX_USE_DEPTH {
                return Err(invalid(ProfileProblem::UsedTooDeep));
            }
            if self.reads >= MAX_PROFILES {
                return Err(invalid(ProfileProblem::UsedTooMany));
            }
            let used_part = self.load(&found, depth + 1, ancestors)?;
            part.merge(used_part);
            if part.entries.len() > MAX_ENTRIES {
                return Err(ProfileError::Invalid {
                    file: shown,
                    key: Some("backup.snapshots".to_owned()),
                    at: None,
                    problem: ProfileProblem::TooManyEntries,
                    detail: String::new(),
                });
            }
        }
        ancestors.pop();
        Ok(part)
    }

    /// The file a name in `use-profiles` stands for: `<name>.toml` in the
    /// first folder that has one, as rustic looks. `None` when no folder does.
    fn find(&self, name: &str) -> Loaded<Option<PathBuf>> {
        let file_name = if name.ends_with(".toml") {
            name.to_owned()
        } else {
            format!("{name}.toml")
        };
        for folder in self.search {
            let candidate = folder.join(&file_name);
            match fs::metadata(&candidate) {
                Ok(_) => return Ok(Some(candidate)),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                    ) => {}
                Err(_) => return Err(unreadable(&candidate.display().to_string())),
            }
        }
        Ok(None)
    }
}

/// Whether `name` is the name of a file and not a path: nothing that steps
/// into another folder or out of this one, no control character, and no more
/// than a file name can hold.
fn is_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

fn unreadable(file: &str) -> ProfileError {
    ProfileError::Unreadable {
        file: file.to_owned(),
    }
}

/// The text of the file, and no more than one byte over the cap of it: a file
/// that never ends (a symlink to `/dev/zero`) can't fill the memory, and what
/// is not a regular file (a pipe would block) is not opened.
fn read_bounded(file: &Path, shown: &str) -> Loaded<String> {
    let metadata = fs::metadata(file).map_err(|_| unreadable(shown))?;
    if !metadata.is_file() {
        return Err(unreadable(shown));
    }
    let mut bytes = Vec::new();
    File::open(file)
        .and_then(|file| file.take(MAX_PROFILE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| unreadable(shown))?;
    if u64::try_from(bytes.len()).is_ok_and(|len| len > MAX_PROFILE_BYTES) {
        return Err(ProfileError::TooLarge {
            file: shown.to_owned(),
            limit: MAX_PROFILE_BYTES,
        });
    }
    String::from_utf8(bytes).map_err(|_| ProfileError::Invalid {
        file: shown.to_owned(),
        key: None,
        at: None,
        problem: ProfileProblem::Syntax,
        detail: "not UTF-8 text".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_a_file_name_when_it_goes_nowhere() {
        for name in ["base", "base.toml", "my profile", "…", ".hidden", "a..b"] {
            assert!(is_file_name(name), "{name:?}");
        }
        for name in [
            "",
            ".",
            "..",
            "../base",
            "dir/base",
            "dir\\base",
            "/etc/rustic/base",
            "a\0b",
            "a\nb",
        ] {
            assert!(!is_file_name(name), "{name:?}");
        }
        assert!(is_file_name(&"a".repeat(MAX_NAME_BYTES)));
        assert!(!is_file_name(&"a".repeat(MAX_NAME_BYTES + 1)));
    }
}
