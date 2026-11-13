use std::assert_matches;

use super::*;

fn password(text: &str) -> Password {
    Password::parse(text.to_owned()).unwrap()
}

#[tokio::test]
async fn the_hash_file_meets_the_admin_password_contract() {
    let data = tempfile::tempdir().unwrap();
    app::contract::admin_password::run(&FileAdminPassword::in_data_dir(data.path())).await;
}

/// Polls `future` once, then drops it, as a server drops a request's handler
/// when its client hangs up.
fn poll_once_and_drop<F: Future>(future: F) {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
}

#[tokio::test]
async fn a_check_whose_caller_hangs_up_still_holds_the_one_hash_until_it_ends() {
    let data = tempfile::tempdir().unwrap();
    let kept = FileAdminPassword::in_data_dir(data.path());
    kept.set(&password("demo-admin-pass")).await.unwrap();
    poll_once_and_drop(kept.verify(&password("a-wrong-guess")));
    assert_eq!(
        kept.hashing.available_permits(),
        0,
        "the abandoned check's hash still runs, so no other may start"
    );
    drop(kept.hashing.acquire().await.unwrap());
    poll_once_and_drop(kept.set(&password("demo-admin-pass")));
    assert_eq!(kept.hashing.available_permits(), 0);
    drop(kept.hashing.acquire().await.unwrap());
}

#[tokio::test]
async fn a_password_another_process_sets_changes_the_fingerprint_the_daemon_sees() {
    let data = tempfile::tempdir().unwrap();
    let daemon = FileAdminPassword::in_data_dir(data.path());
    let set_password = FileAdminPassword::in_data_dir(data.path());
    set_password
        .set(&password("demo-admin-pass"))
        .await
        .unwrap();
    let before = daemon.fingerprint().unwrap();
    assert_eq!(daemon.fingerprint(), Ok(before));
    set_password
        .set(&password("demo-admin-pass"))
        .await
        .unwrap();
    assert_ne!(daemon.fingerprint(), Ok(before));
    std::fs::remove_file(data.path().join(FILE)).unwrap();
    assert_eq!(daemon.fingerprint(), Err(AdminPasswordError::NotSet));
}

#[tokio::test]
async fn only_an_argon2id_hash_is_kept_never_the_password() {
    let data = tempfile::tempdir().unwrap();
    let kept = FileAdminPassword::in_data_dir(data.path());
    assert!(!kept.is_set().unwrap());
    kept.set(&password("demo-admin-pass")).await.unwrap();
    assert!(kept.is_set().unwrap());
    let text = std::fs::read_to_string(data.path().join(FILE)).unwrap();
    assert!(text.starts_with("$argon2id$"), "{text}");
    assert!(!text.contains("demo-admin-pass"));
    assert!(!data.path().join("admin-password.partial").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn the_hash_file_is_readable_by_its_owner_alone() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let kept = FileAdminPassword::in_data_dir(data.path());
    // A leftover of an interrupted run, readable by all, is not reused.
    std::fs::write(data.path().join("admin-password.partial"), "x").unwrap();
    std::fs::set_permissions(
        data.path().join("admin-password.partial"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    kept.set(&password("demo-admin-pass")).await.unwrap();
    let mode = std::fs::metadata(data.path().join(FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn a_hash_file_others_can_read_is_refused_never_sign_in_off() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let kept = FileAdminPassword::in_data_dir(data.path());
    kept.set(&password("demo-admin-pass")).await.unwrap();
    std::fs::set_permissions(
        data.path().join(FILE),
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    assert_matches!(
        kept.is_set(),
        Err(AdminPasswordError::Unreadable(text)) if text.contains("other users")
    );
    assert_matches!(
        kept.verify(&password("demo-admin-pass")).await,
        Err(AdminPasswordError::Unreadable(_))
    );
}

#[tokio::test]
async fn a_file_that_is_not_an_argon2id_hash_is_refused_never_sign_in_off() {
    let data = tempfile::tempdir().unwrap();
    let kept = FileAdminPassword::in_data_dir(data.path());
    for text in [
        "demo-admin-pass\n",
        "$argon2i$v=19$m=16,t=2,p=1$c2FsdHNhbHQ$Zm9vYmFyYmF6\n",
        "",
    ] {
        write_hash(&data.path().join(FILE), text.trim_end()).unwrap();
        assert_matches!(
            kept.is_set(),
            Err(AdminPasswordError::Unreadable(_)),
            "{text:?}"
        );
    }
}

#[tokio::test]
async fn a_file_larger_than_a_hash_is_refused_unread() {
    let data = tempfile::tempdir().unwrap();
    let kept = FileAdminPassword::in_data_dir(data.path());
    write_hash(&data.path().join(FILE), &"x".repeat(1000)).unwrap();
    assert_matches!(
        kept.is_set(),
        Err(AdminPasswordError::Unreadable(text)) if text.contains("larger")
    );
}
