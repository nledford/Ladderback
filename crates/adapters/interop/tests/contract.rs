//! The real reader and writer against the profiles contract suite, the one
//! the fakes pass too, so the fakes can't drift from the real thing.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use app::contract::profiles::{self, ProfilesHarness};
use app::ports::{ProfileReader, ProfileWriter};
use common::{ENTRY, REPOSITORY, Scratch};
use domain::RepositoryAlias;
use interop::RusticProfileWriter;

struct RealProfiles(Scratch);

impl ProfilesHarness for RealProfiles {
    fn reader(&self) -> Arc<dyn ProfileReader> {
        Arc::new(self.0.reader())
    }

    fn writer(&self) -> Arc<dyn ProfileWriter> {
        Arc::new(RusticProfileWriter::new())
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
}

#[tokio::test]
async fn the_rustic_reader_and_writer_meet_the_profiles_contract() {
    profiles::run(&RealProfiles(Scratch::new())).await;
}
