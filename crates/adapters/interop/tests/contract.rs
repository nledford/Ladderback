//! The real reader and writer against the profiles contract suite, the one
//! the fakes pass too, so the fakes can't drift from the real thing.

#[expect(dead_code, reason = "this file uses a part of the shared helpers")]
mod common;

use std::path::PathBuf;
use std::sync::Arc;

use app::contract::profiles::{self, ProfilesHarness};
use app::ports::{ProfileReader, ProfileWriter};
use common::{ENTRY, REPOSITORY, Scratch};
use domain::RepositoryAlias;
use interop::{KeyringCommand, RusticProfileWriter};

struct RealProfiles(Scratch);

impl ProfilesHarness for RealProfiles {
    fn reader(&self) -> Arc<dyn ProfileReader> {
        Arc::new(self.0.reader())
    }

    fn writer(&self) -> Arc<dyn ProfileWriter> {
        Arc::new(RusticProfileWriter::new(
            KeyringCommand::of_this_platform(),
            Some(std::ffi::OsStr::new("")),
        ))
    }

    fn good_profile(&self, _repository: &RepositoryAlias) -> PathBuf {
        self.0
            .profile("nas.toml", &format!("{REPOSITORY}\n{ENTRY}"))
    }

    fn reference_profile(&self, _repository: &RepositoryAlias) -> PathBuf {
        let text = format!(
            "[repository]\nrepository = \"s3:${{file:/srv/demo-endpoint}}\"\n\
             password-file = \"/srv/demo-password\"\n\n{ENTRY}"
        );
        self.0.profile("nas.toml", &text)
    }

    fn broken_profile(&self) -> PathBuf {
        self.0.profile("broken.toml", "this is [not toml")
    }

    fn missing_profile(&self) -> PathBuf {
        self.0.work.join("missing.toml")
    }

    fn text_using(&self, _repository: &RepositoryAlias, used: &str) -> String {
        // Where a read from a file would find it: in the working folder.
        self.0.profile(
            &format!("{used}.toml"),
            "[[backup.snapshots]]\nname = \"work\"\nsources = [\"/home/demo/work\"]\n",
        );
        format!("[global]\nuse-profiles = [\"{used}\"]\n\n{REPOSITORY}\n{ENTRY}")
    }
}

#[tokio::test]
async fn the_rustic_reader_and_writer_meet_the_profiles_contract() {
    profiles::run(&RealProfiles(Scratch::new())).await;
}
