//! What the reader's tests share: a scratch folder with the three places a
//! used profile is looked for, and profiles written into them.

#![allow(dead_code, reason = "each test file uses a part of this")]

use std::fs;
use std::path::{Path, PathBuf};

use app::ports::{ImportedProfile, ProfileError, ProfileNotice, ProfileReader};
use domain::RepositoryAlias;
use domain::planning::{HookEntry, Plan};
use interop::FileProfileReader;
use tempfile::TempDir;

/// A repository with a password in a file: what every profile needs.
pub const REPOSITORY: &str = "\
[repository]
repository = \"/srv/demo-repo\"
password-file = \"/srv/demo-password\"
";

/// One snapshot entry named `home`.
pub const ENTRY: &str = "\
[[backup.snapshots]]
name = \"home\"
sources = [\"/home/demo\"]
";

pub struct Scratch {
    _dir: TempDir,
    /// The folder a profile is written in, and the last place a used profile
    /// is looked for.
    pub work: PathBuf,
    /// rustic's config folder: the first place.
    pub rustic: PathBuf,
    /// Stands for `/etc/rustic`: the second place.
    pub etc: PathBuf,
}

impl Scratch {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("a scratch folder");
        let work = dir.path().join("work");
        let rustic = dir.path().join("rustic");
        let etc = dir.path().join("etc");
        for folder in [&work, &rustic, &etc] {
            fs::create_dir_all(folder).expect("a folder");
        }
        Self {
            _dir: dir,
            work,
            rustic,
            etc,
        }
    }

    pub fn reader(&self) -> FileProfileReader {
        self.reader_with(&[])
    }

    /// A reader whose environment has the variables `variables`.
    pub fn reader_with(&self, variables: &[(&str, &str)]) -> FileProfileReader {
        FileProfileReader::new(
            vec![self.rustic.clone(), self.etc.clone(), self.work.clone()],
            variables
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
        )
    }

    /// Writes a profile in the working folder; returns where.
    pub fn profile(&self, name: &str, text: &str) -> PathBuf {
        write_in(&self.work, name, text)
    }

    /// Reads `nas.toml` with `text` as the repository `nas`.
    pub async fn read(&self, text: &str) -> Result<ImportedProfile, ProfileError> {
        self.read_with(&[], text).await
    }

    pub async fn read_with(
        &self,
        variables: &[(&str, &str)],
        text: &str,
    ) -> Result<ImportedProfile, ProfileError> {
        let file = self.profile("nas.toml", text);
        self.reader_with(variables).read(&file, &nas()).await
    }
}

/// A named pipe at `path`, or `None` where `mkfifo` is not to be had. Opening
/// one for reading blocks until somebody opens it for writing, so a reader
/// that opens it anyway would hang; a thread that opens it for writing
/// unblocks that reader, which reads nothing, so the test fails instead of
/// hanging. Where the reader leaves the pipe alone, the thread waits for the
/// rest of the process, which ends with the test run.
#[cfg(unix)]
pub fn fifo(path: &Path) -> Option<()> {
    let mut mkfifo = std::process::Command::new("mkfifo");
    mkfifo.arg(path);
    let made = spawn_gate::hold(|| mkfifo.spawn())
        .and_then(|mut child| child.wait())
        .is_ok_and(|status| status.success());
    if !made {
        return None;
    }
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = fs::OpenOptions::new().write(true).open(path);
    });
    Some(())
}

pub fn write_in(folder: &Path, name: &str, text: &str) -> PathBuf {
    let path = folder.join(name);
    fs::write(&path, text).expect("a profile");
    path
}

pub fn nas() -> RepositoryAlias {
    RepositoryAlias::parse("nas").expect("an alias")
}

/// The profile `text` gave, which has to be one.
pub async fn read_ok(text: &str) -> ImportedProfile {
    Scratch::new()
        .read(text)
        .await
        .unwrap_or_else(|error| panic!("the profile should be read: {error:?}\n{text}"))
}

/// The refusal `text` gives, which has to be one.
pub async fn read_err(text: &str) -> ProfileError {
    match Scratch::new().read(text).await {
        Ok(profile) => panic!("the profile should be refused, got {profile:?}\n{text}"),
        Err(error) => error,
    }
}

pub fn plan<'a>(profile: &'a ImportedProfile, id: &str) -> &'a Plan {
    profile
        .plans
        .iter()
        .find(|plan| plan.id.as_str() == id)
        .unwrap_or_else(|| panic!("no plan {id} in {:?}", ids(profile)))
}

pub fn ids(profile: &ImportedProfile) -> Vec<&str> {
    profile.plans.iter().map(|plan| plan.id.as_str()).collect()
}

/// Hooks as the commands they run, each as its words joined by spaces.
pub fn commands(hooks: &[HookEntry]) -> Vec<String> {
    hooks
        .iter()
        .map(|hook| {
            let mut words = vec![hook.command.program().to_owned()];
            words.extend(hook.command.args().iter().cloned());
            words.join(" ")
        })
        .collect()
}

pub fn ignored(key: &str) -> ProfileNotice {
    ProfileNotice::KeyIgnored {
        key: key.to_owned(),
    }
}

/// The line, from 1, of the first line that has `needle` in `text`.
pub fn line_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|line| line.contains(needle))
        .map(|index| index + 1)
        .unwrap_or_else(|| panic!("no line with {needle:?}"))
}
