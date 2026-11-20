use std::assert_matches;
use std::net::{IpAddr, Ipv4Addr};

use domain::Timestamp;
use domain::access::permissions::{Level, Permission};
use domain::planning::{AgentConfig, DryRunAllowlist, DryRunFolder};
use futures::FutureExt;

use super::*;
use crate::fakes::{
    FakeClock, InMemoryAdminPassword, InMemoryApiTokenStore, InMemoryConfigStore,
    InMemorySessionStore, SeededRandom,
};

const HOME: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));

fn password(text: &str) -> Password {
    Password::parse(text.to_owned()).unwrap()
}

fn access() -> Access {
    Access::new(
        Arc::new(InMemoryAdminPassword::with("demo-admin-pass")),
        Arc::new(SeededRandom::new(7)),
        Arc::new(FakeClock::new(
            Timestamp::from_second(1_800_000_000).unwrap(),
        )),
        Arc::new(InMemoryApiTokenStore::new()),
        Arc::new(InMemorySessionStore::new()),
    )
}

/// A configuration with something else in it, so a change is seen to keep
/// the rest.
fn allowlisted() -> AgentConfig {
    AgentConfig::default().with_dry_run_allowlist(
        DryRunAllowlist::new(vec![DryRunFolder::parse("/srv/demo").unwrap()]).unwrap(),
    )
}

fn over(config: AgentConfig) -> (WithoutToken, Arc<InMemoryConfigStore>) {
    let store = Arc::new(InMemoryConfigStore::new());
    store.save(&config).now_or_never().unwrap().unwrap();
    let without_token = WithoutToken::new(store.clone(), Arc::default());
    (without_token, store)
}

fn widened() -> Permissions {
    Permissions::default()
        .with(Permission::DaemonLog, Some(Level::Read))
        .unwrap()
}

#[test]
fn a_permission_not_granted_is_refused_naming_itself_and_one_granted_lets_through() {
    let read_only = Permissions::default()
        .with(Permission::BackupRuns, None)
        .unwrap();
    let (without_token, _) = over(AgentConfig::default().with_without_token(read_only));
    assert_matches!(
        without_token
            .allow(Permission::BackupRuns)
            .now_or_never()
            .unwrap(),
        Err(WithoutTokenRefused::Lacks(Permission::BackupRuns))
    );
    assert_matches!(
        without_token
            .allow(Permission::Backups)
            .now_or_never()
            .unwrap(),
        Ok(permissions) if permissions == read_only
    );
    assert_matches!(
        without_token
            .allow(Permission::DaemonLog)
            .now_or_never()
            .unwrap(),
        Err(WithoutTokenRefused::Lacks(Permission::DaemonLog))
    );
}

#[test]
fn with_sign_in_on_a_change_without_the_current_admin_password_changes_nothing() {
    let access = access();
    let (without_token, store) = over(allowlisted());
    for current in [None, Some(password("not-the-password"))] {
        let refused = without_token
            .change(Some(&access), HOME, current.as_ref(), widened())
            .now_or_never()
            .unwrap();
        assert_matches!(
            refused,
            Err(WithoutTokenChangeRefused::Password(
                PasswordRefused::WrongPassword
            ))
        );
    }
    assert_eq!(store.load().now_or_never().unwrap().unwrap(), allowlisted());
}

#[test]
fn with_sign_in_on_a_change_with_the_current_admin_password_is_kept_with_the_rest() {
    let access = access();
    let (without_token, store) = over(allowlisted());
    let changed = without_token
        .change(
            Some(&access),
            HOME,
            Some(&password("demo-admin-pass")),
            widened(),
        )
        .now_or_never()
        .unwrap();
    assert_matches!(changed, Ok(kept) if kept == widened());
    assert_eq!(
        store.load().now_or_never().unwrap().unwrap(),
        allowlisted().with_without_token(widened())
    );
}

#[test]
fn with_sign_in_off_a_change_needs_no_password() {
    let (without_token, store) = over(AgentConfig::default());
    let changed = without_token
        .change(None, HOME, None, widened())
        .now_or_never()
        .unwrap();
    assert_matches!(changed, Ok(kept) if kept == widened());
    assert_eq!(
        store
            .load()
            .now_or_never()
            .unwrap()
            .unwrap()
            .without_token(),
        widened()
    );
}
