use domain::access::Password;

use crate::ports::{AdminPassword, AdminPasswordError};

/// `fresh` keeps no admin password yet.
pub async fn run(fresh: &dyn AdminPassword) {
    let passwords = [
        "demo-admin-pass",
        "demo-admin-pas",
        "Demo-admin-pass",
        "another-admin-pass",
    ]
    .map(|text| Password::parse(text.to_owned()));
    assert!(
        passwords.iter().all(Result::is_ok),
        "the suite's passwords are valid: {passwords:?}"
    );
    let [Ok(demo), Ok(shorter), Ok(other_case), Ok(another)] = passwords else {
        return;
    };
    assert_eq!(
        fresh.verify(&demo).await,
        Err(AdminPasswordError::NotSet),
        "with no password kept, nothing is checked"
    );
    assert_eq!(fresh.fingerprint(), Err(AdminPasswordError::NotSet));
    assert_eq!(fresh.set(&demo).await, Ok(()), "the password is kept");
    assert_eq!(fresh.verify(&demo).await, Ok(true));
    assert_eq!(fresh.verify(&shorter).await, Ok(false));
    assert_eq!(fresh.verify(&other_case).await, Ok(false));
    let first = fresh.fingerprint();
    assert!(
        first.is_ok(),
        "a kept password has a fingerprint: {first:?}"
    );
    let Ok(first) = first else { return };
    assert_eq!(
        fresh.fingerprint(),
        Ok(first),
        "the fingerprint holds while the password does"
    );
    assert_eq!(
        fresh.set(&another).await,
        Ok(()),
        "the password is replaced"
    );
    assert_eq!(
        fresh.verify(&demo).await,
        Ok(false),
        "a new password replaces the one before it"
    );
    assert_eq!(fresh.verify(&another).await, Ok(true));
    let second = fresh.fingerprint();
    assert!(
        second.is_ok(),
        "a kept password has a fingerprint: {second:?}"
    );
    let Ok(second) = second else { return };
    assert_ne!(first, second, "a new password has a new fingerprint");
    assert_eq!(
        fresh.set(&another).await,
        Ok(()),
        "the password is set again"
    );
    assert_ne!(
        fresh.fingerprint(),
        Ok(second),
        "setting the same password again is a new password too"
    );
}
