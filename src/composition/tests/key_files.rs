//! The secret key files in the data folder stay out of every backup that
//! walks it: by glob, or, when no glob can name the folder, by refusing the
//! backup that would take them (§16, decision 120, "A copied data folder").

use std::path::Path;

use domain::planning::Source;

use super::captured_log::{self, capture};
use super::support::runner_at_a_gate;
use super::*;

/// The data folder is named by the globs of its path as given and as it
/// resolves, once when the two are one; when a form isn't UTF-8 no glob
/// can name it, and each form is kept to refuse a backup by.
#[cfg(unix)]
#[test]
fn a_data_folder_is_named_by_globs_unless_a_form_of_its_path_isnt_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let given = Path::new("/var/demo/ladderback");
    let canonical = Path::new("/private/var/demo/ladderback");
    let one = domain::fleet::left_out_globs("/var/demo/ladderback");
    let mut both = one.clone();
    both.extend(domain::fleet::left_out_globs(
        "/private/var/demo/ladderback",
    ));
    assert_eq!(
        KeyFilesLeftOut::over(given, Some(canonical)),
        KeyFilesLeftOut::Globs(both)
    );
    assert_eq!(
        KeyFilesLeftOut::over(given, Some(given)),
        KeyFilesLeftOut::Globs(one.clone())
    );
    assert_eq!(
        KeyFilesLeftOut::over(given, None),
        KeyFilesLeftOut::Globs(one)
    );

    let unnamed = Path::new(std::ffi::OsStr::from_bytes(b"/srv/\xff/ladderback"));
    assert_eq!(
        KeyFilesLeftOut::over(unnamed, None),
        KeyFilesLeftOut::Unnamed(vec![unnamed.to_owned()])
    );
    assert_eq!(
        KeyFilesLeftOut::over(given, Some(unnamed)),
        KeyFilesLeftOut::Unnamed(vec![given.to_owned(), unnamed.to_owned()])
    );
    assert_eq!(
        KeyFilesLeftOut::over(unnamed, Some(canonical)),
        KeyFilesLeftOut::Unnamed(vec![unnamed.to_owned(), canonical.to_owned()])
    );
}

/// When no glob can name the data folder, a backup whose sources hold it
/// is refused while it holds a secret key file, or a name staged beside
/// one, and a scheduled one says so in the log by its code. With only a
/// public key file there, or with sources that don't hold the folder, the
/// backup is built as before, with no glob of the key files'.
#[tokio::test]
async fn a_backup_that_would_take_key_files_no_glob_can_name_is_refused() {
    capture();
    let (mut runner, _entered, _open_gate, scratch) = runner_at_a_gate().await;
    let data = scratch.path().join("source").join("ladderback");
    std::fs::create_dir_all(&data).expect("the data folder");
    std::fs::write(data.join("hub-recovery-key"), "public").expect("a public key file");
    runner.left_out =
        KeyFilesLeftOut::Unnamed(vec![scratch.path().join("elsewhere"), data.clone()]);
    let config = runner.config.load().await.expect("the configuration");
    let home = domain::PlanId::parse("home").expect("a plan ID");
    let iglobs = |built: Result<BuiltBackup, Unbuilt>| match built {
        Ok(BuiltBackup {
            job: Job::Backup(job),
            ..
        }) => job.filter.iglobs,
        Ok(_) => panic!("not a backup"),
        Err(error) => panic!("refused: {:?}", error.into_start_error()),
    };

    let built = runner.backup_of(&config, &home, &Trigger::Manual).await;
    assert_eq!(iglobs(built), Vec::<String>::new(), "no secret key file");

    std::fs::write(data.join("hub-tls-key.0123.tmp"), "staged").expect("a staged key file");
    let refused = runner.backup_of(&config, &home, &Trigger::Manual).await;
    assert!(
        matches!(
            refused,
            Err(Unbuilt::Refused(StartError::KeyFilesNotLeftOut))
        ),
        "a staged key file is refused"
    );
    let place = runner
        .queue
        .push(&home, domain::planning::Concurrency::default());
    PlanRuns::start(&runner, &home, Trigger::Schedule, place).await;
    let said = captured_log::lines();
    assert!(
        said.iter().any(|(level, line)| *level == log::Level::Warn
            && line.contains("the scheduled run of plan home didn't start")
            && line.contains("(key-files-not-left-out)")),
        "{said:?}"
    );

    // A data folder the plan's sources don't hold is no backup's to refuse.
    let outside = scratch.path().join("data");
    std::fs::write(outside.join("hub-key"), "secret").expect("a key file");
    runner.left_out = KeyFilesLeftOut::Unnamed(vec![outside]);
    let built = runner.backup_of(&config, &home, &Trigger::Manual).await;
    assert_eq!(iglobs(built), Vec::<String>::new(), "a folder elsewhere");
}

/// A dry run predicts what the backup does: it leaves out the globs the
/// backup leaves out, and is refused where the backup is, when its sources
/// hold a data folder no glob can name while that folder holds a secret
/// key file.
#[test]
fn a_dry_run_is_refused_where_the_backup_is() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let data = scratch.path().join("ladderback");
    std::fs::create_dir_all(&data).expect("the data folder");
    std::fs::write(data.join("hub-key"), "canary").expect("a key file");
    let source = |path: &Path| Source::parse(path.to_str().expect("UTF-8")).expect("a source");
    let holding = [source(scratch.path())];
    let elsewhere = [source(&scratch.path().join("elsewhere"))];

    let globs = domain::fleet::left_out_globs("/var/demo/ladderback");
    let named = KeyFilesLeftOut::Globs(globs.clone()).for_dry_runs();
    assert_eq!(named(&holding), Some(globs));

    let unnamed = KeyFilesLeftOut::Unnamed(vec![data]).for_dry_runs();
    assert_eq!(unnamed(&holding), None, "refused as the backup is");
    assert_eq!(
        unnamed(&elsewhere),
        Some(Vec::new()),
        "not the backup's to refuse"
    );
}
