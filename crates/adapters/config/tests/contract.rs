//! The file store against the config store's contract suite, the one the
//! in-memory fake passes too, so the fake can't drift from the real thing.

mod support;

use std::time::Duration;

use app::contract;
use support::Scratch;

#[tokio::test]
async fn the_file_store_meets_the_config_store_contract() {
    let scratch = Scratch::new();
    let store = scratch.store();
    tokio::time::timeout(Duration::from_secs(60), contract::config_store::run(&store))
        .await
        .expect("the contract suite finishes");
}

#[test]
fn the_json_codec_meets_the_change_codec_contract() {
    contract::change_codec::run(&config_adapter::JsonChangeCodec);
}

/// A revision is the hash of the file's bytes: the same text is the same
/// revision, a changed byte another, and no file the revision of no text.
#[tokio::test]
async fn a_revision_names_the_files_text() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let none = store.revision().await.expect("a revision of no file");
    assert_eq!(
        none.as_str(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    scratch.write("schema = 1\n");
    let first = store.revision().await.expect("a revision");
    assert_ne!(first, none);
    assert_eq!(store.revision().await.expect("a revision"), first);
    scratch.write("schema = 1\n\n");
    assert_ne!(store.revision().await.expect("a revision"), first);
}
