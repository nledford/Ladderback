//! `FileConfigStore::record_id` on a real file: the daemon adds one line to
//! a file a person may have written by hand, and leaves every other byte
//! alone, or writes nothing (technical design §6.1, decision NN).

mod support;

use app::ports::{ConfigStore, NotInPlace, RecordError};
use domain::planning::{AgentConfig, NotRecorded, RepositoryConfig};
use domain::{RepositoryAlias, RepositoryId};
use support::*;

fn alias(name: &str) -> RepositoryAlias {
    RepositoryAlias::parse(name).expect("an alias")
}

fn opened() -> RepositoryId {
    RepositoryId::parse(&"07".repeat(32)).expect("a repository ID")
}

fn id_line() -> String {
    format!("repository-id = \"{}\"\n", "07".repeat(32))
}

async fn loaded(scratch: &Scratch) -> AgentConfig {
    scratch.store().load().await.expect("the file loads")
}

async fn entry(scratch: &Scratch, name: &str) -> RepositoryConfig {
    loaded(scratch)
        .await
        .repository(&alias(name))
        .cloned()
        .expect("the entry")
}

/// Comments, blank lines, keys in an order the daemon never writes, a quoted
/// header, and an entry that isn't the last table.
const HAND_WRITTEN: &str = "\
# My backups, by hand.
schema = 1

# The NAS in the hall.
[repositories.\"nas\"]
maintenance = \"local\"   # the laptop prunes it
password-env = \"DEMO_NAS_PASSWORD\"

repository = '/srv/demo nas'
# end of the NAS

[repositories.usb]
password-env = \"DEMO_USB_PASSWORD\"
repository = \"/media/demo/usb\"

[[plans]]
id = \"home\"   # nightly
repository = \"nas\"
sources = [ \"/home/demo\" ]
";

#[tokio::test]
async fn a_hand_written_file_gains_one_line_and_is_otherwise_byte_for_byte_the_same() {
    let scratch = Scratch::new();
    scratch.write(HAND_WRITTEN);
    let read = entry(&scratch, "nas").await;
    let before = loaded(&scratch).await;
    assert_eq!(
        scratch
            .store()
            .record_id(&alias("nas"), &read, &opened())
            .await,
        Ok(())
    );
    let expected = HAND_WRITTEN.replacen(
        "repository = '/srv/demo nas'\n",
        &format!("repository = '/srv/demo nas'\n{}", id_line()),
        1,
    );
    assert_eq!(scratch.read(), expected);
    assert_eq!(
        loaded(&scratch).await,
        before
            .record_id(&alias("nas"), &read, &opened())
            .expect("the rule records it")
    );
}

#[tokio::test]
async fn the_file_it_replaced_is_kept_in_the_history() {
    let scratch = Scratch::new();
    scratch.write(HAND_WRITTEN);
    let read = entry(&scratch, "usb").await;
    scratch
        .store()
        .record_id(&alias("usb"), &read, &opened())
        .await
        .expect("the ID is recorded");
    let history = scratch.history();
    let [(_, kept)] = history.as_slice() else {
        panic!("one version kept: {history:?}");
    };
    assert_eq!(kept.as_slice(), HAND_WRITTEN.as_bytes());
    assert_eq!(mode_of(&scratch.path) & 0o777, 0o600, "as a save leaves it");
}

/// The entry in forms a person may write and the daemon never does: the
/// entry gets the ID, and the file gains one run of text and nothing else.
#[tokio::test]
async fn an_entry_written_as_an_inline_table_or_dotted_keys_gets_the_id() {
    let forms = [
        "schema = 1\n[repositories]\ndemo = { repository = \"/srv/demo\", password-env = \"DEMO\" }\n",
        "schema = 1\n[repositories]\ndemo.repository = \"/srv/demo\"\ndemo.password-env = \"DEMO\"\n# after\n",
        "schema = 1\nrepositories.demo.repository = \"/srv/demo\"\nrepositories.demo.password-env = \"DEMO\"\n",
        "schema = 1\n[repositories.demo]\nrepository = \"/srv/demo\"\npassword-env = \"DEMO\"\nhooks.run-before = [\"true\"]\n",
    ];
    for form in forms {
        let scratch = Scratch::new();
        scratch.write(form);
        let read = entry(&scratch, "demo").await;
        let before = loaded(&scratch).await;
        assert_eq!(
            scratch
                .store()
                .record_id(&alias("demo"), &read, &opened())
                .await,
            Ok(()),
            "{form}"
        );
        assert_eq!(
            loaded(&scratch).await,
            before
                .record_id(&alias("demo"), &read, &opened())
                .expect("the rule records it"),
            "{form}"
        );
        let after = scratch.read();
        let added = after.len().checked_sub(form.len()).expect("only added");
        let common_start = form
            .bytes()
            .zip(after.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        assert!(
            form.as_bytes().get(common_start..) == after.as_bytes().get(common_start + added..),
            "one run of text added and nothing else changed: {form:?} became {after:?}"
        );
    }
}

/// Dotted keys of two entries written in turns: the ID can't be added to
/// the one without moving the other's keys, so nothing is written.
#[tokio::test]
async fn an_entry_whose_dotted_keys_are_interleaved_with_another_entrys_is_left_as_it_is() {
    let form = "schema = 1\n[repositories]\n\
                demo.repository = \"/srv/demo\"\n\
                usb.repository = \"/media/demo/usb\"\n\
                demo.password-env = \"DEMO\"\n\
                usb.password-env = \"DEMO_USB\"\n";
    let scratch = Scratch::new();
    scratch.write(form);
    let read = entry(&scratch, "demo").await;
    assert_eq!(
        scratch
            .store()
            .record_id(&alias("demo"), &read, &opened())
            .await,
        Err(RecordError::NotInPlace(NotInPlace::ReadBack))
    );
    assert_eq!(scratch.read(), form, "nothing is written");
    assert!(scratch.history().is_empty());
}

/// A file written with CRLF line endings keeps them: the added line ends
/// as every other line does.
#[tokio::test]
async fn a_file_with_crlf_line_endings_gains_a_crlf_line() {
    let form = "schema = 1\r\n[repositories.demo]\r\nrepository = \"/srv/demo\"\r\npassword-env = \"DEMO\"\r\n\r\n[repositories.usb]\r\nrepository = \"/media/demo/usb\"\r\npassword-env = \"DEMO_USB\"\r\n";
    let scratch = Scratch::new();
    scratch.write(form);
    let read = entry(&scratch, "demo").await;
    assert_eq!(
        scratch
            .store()
            .record_id(&alias("demo"), &read, &opened())
            .await,
        Ok(())
    );
    assert_eq!(
        scratch.read(),
        form.replacen(
            "password-env = \"DEMO\"\r\n",
            &format!(
                "password-env = \"DEMO\"\r\nrepository-id = \"{}\"\r\n",
                "07".repeat(32)
            ),
            1
        )
    );
}

#[tokio::test]
async fn an_entry_changed_on_disk_after_the_job_read_it_is_left_as_it_is() {
    let scratch = Scratch::new();
    scratch.write(HAND_WRITTEN);
    let read = entry(&scratch, "nas").await;
    let edited = HAND_WRITTEN.replace("'/srv/demo nas'", "'/srv/other nas'");
    scratch.write(&edited);
    assert_eq!(
        scratch
            .store()
            .record_id(&alias("nas"), &read, &opened())
            .await,
        Err(RecordError::NotRecorded(NotRecorded::Changed))
    );
    assert_eq!(scratch.read(), edited);
    assert!(scratch.history().is_empty());
}

/// A hand edit elsewhere in the file, made after the job read its entry,
/// stays: the line is added to the file as it is now.
#[tokio::test]
async fn an_edit_elsewhere_in_the_file_after_the_job_read_its_entry_is_kept() {
    let scratch = Scratch::new();
    scratch.write(HAND_WRITTEN);
    let read = entry(&scratch, "usb").await;
    let edited = format!("{HAND_WRITTEN}label = \"added by hand\"\n");
    scratch.write(&edited);
    scratch
        .store()
        .record_id(&alias("usb"), &read, &opened())
        .await
        .expect("the ID is recorded");
    assert_eq!(
        scratch.read(),
        edited.replacen(
            "repository = \"/media/demo/usb\"\n",
            &format!("repository = \"/media/demo/usb\"\n{}", id_line()),
            1
        )
    );
}

#[tokio::test]
async fn a_file_that_is_not_a_configuration_is_not_written() {
    let scratch = Scratch::new();
    scratch.write(HAND_WRITTEN);
    let read = entry(&scratch, "usb").await;
    let broken = format!("{HAND_WRITTEN}[[plans]]\nid = \"home\"\n");
    scratch.write(&broken);
    let recorded = scratch
        .store()
        .record_id(&alias("usb"), &read, &opened())
        .await;
    assert!(
        matches!(recorded, Err(RecordError::Store(_))),
        "{recorded:?}"
    );
    assert_eq!(scratch.read(), broken);
}
