//! The `Identity` port's contract (§7.1; §16, decision 120, "The hub key"
//! and "A copied data folder"): what the hub and its agents rely on of
//! their key files, whichever implementation keeps them. The real adapter's
//! own tests add what only files have: permissions, the text on disk, a
//! damaged file, a second process.

use domain::fleet::{
    ConnectionCounter, Fingerprint, HubAddress, HubPin, PublicKey, SeedFile, SigningSeed, TlsKey,
    TokenTls,
};

use crate::ports::{Identity, IdentityError};

/// `fresh` keeps no key file yet.
pub async fn run(fresh: &dyn Identity) {
    nothing_is_kept_at_first(fresh).await;
    a_seed_once_kept_is_never_replaced(fresh).await;
    each_seed_file_is_its_own(fresh).await;
    a_forgotten_recovery_seed_is_gone(fresh).await;
    a_recovery_key_once_kept_is_never_replaced(fresh).await;
    a_tls_key_once_kept_is_never_replaced(fresh).await;
    a_pin_is_replaced_whole(fresh).await;
    each_attempt_takes_the_next_counter(fresh).await;
    one_attempt_holds_the_counter_at_a_time(fresh).await;
}

fn seed(byte: u8) -> SigningSeed {
    SigningSeed::from_bytes([byte; 32])
}

/// RFC 8032 §7.1's keys of tests 1 and 2; `None`, after a failed
/// assertion, if the domain refuses one.
fn keys() -> Option<(PublicKey, PublicKey)> {
    let first =
        PublicKey::from_hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
    let second =
        PublicKey::from_hex("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c");
    assert!(
        first.is_ok() && second.is_ok(),
        "RFC 8032's keys: {first:?}, {second:?}"
    );
    first.ok().zip(second.ok())
}

fn tls_key(byte: u8) -> Option<TlsKey> {
    let key = TlsKey::from_der(vec![byte; 138]);
    assert!(key.is_ok(), "a TLS key of 138 bytes");
    key.ok()
}

fn pin(generation: u64) -> Option<HubPin> {
    let address = HubAddress::parse("https://hub.example:9898");
    assert!(address.is_ok(), "a hub address");
    let ((hub_key, recovery_key), address) = keys().zip(address.ok())?;
    let mut pin = HubPin::new(
        hub_key,
        TokenTls::Pinned(Fingerprint::from_digest([0x5a; 32])),
        recovery_key,
        address,
    );
    pin.generation = generation;
    Some(pin)
}

/// The seed's bytes, or `None` for no seed, and after a failed assertion
/// for an error.
fn exposed(seed: &Result<Option<SigningSeed>, IdentityError>) -> Option<[u8; 32]> {
    assert!(seed.is_ok(), "the seed couldn't be read: {seed:?}");
    seed.as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map(|seed| *seed.expose())
}

async fn nothing_is_kept_at_first(fresh: &dyn Identity) {
    for file in SeedFile::ALL {
        assert_eq!(exposed(&fresh.seed(file).await), None, "{file:?}");
    }
    assert_eq!(fresh.recovery_key().await, Ok(None));
    assert!(matches!(fresh.tls_key().await, Ok(None)));
    assert_eq!(fresh.pin().await, Ok(None));
}

async fn a_seed_once_kept_is_never_replaced(identity: &dyn Identity) {
    let kept = identity.keep_seed(SeedFile::AgentKey, seed(1)).await;
    assert_eq!(kept.map(|seed| *seed.expose()), Ok([1; 32]));
    assert_eq!(
        exposed(&identity.seed(SeedFile::AgentKey).await),
        Some([1; 32])
    );
    let again = identity.keep_seed(SeedFile::AgentKey, seed(2)).await;
    assert_eq!(
        again.map(|seed| *seed.expose()),
        Ok([1; 32]),
        "the seed found is answered, not the one given"
    );
    assert_eq!(
        exposed(&identity.seed(SeedFile::AgentKey).await),
        Some([1; 32]),
        "and it is still the one kept"
    );
}

async fn each_seed_file_is_its_own(identity: &dyn Identity) {
    let hub = identity.keep_seed(SeedFile::HubKey, seed(3)).await;
    assert_eq!(hub.map(|seed| *seed.expose()), Ok([3; 32]));
    let recovery = identity.keep_seed(SeedFile::RecoverySeed, seed(4)).await;
    assert_eq!(recovery.map(|seed| *seed.expose()), Ok([4; 32]));
    for (file, byte) in [
        (SeedFile::AgentKey, 1),
        (SeedFile::HubKey, 3),
        (SeedFile::RecoverySeed, 4),
    ] {
        assert_eq!(
            exposed(&identity.seed(file).await),
            Some([byte; 32]),
            "{file:?}"
        );
    }
}

async fn a_forgotten_recovery_seed_is_gone(identity: &dyn Identity) {
    assert_eq!(identity.forget_recovery_seed().await, Ok(()));
    assert_eq!(exposed(&identity.seed(SeedFile::RecoverySeed).await), None);
    assert_eq!(
        identity.forget_recovery_seed().await,
        Ok(()),
        "forgetting none is done"
    );
    for (file, byte) in [(SeedFile::AgentKey, 1), (SeedFile::HubKey, 3)] {
        assert_eq!(
            exposed(&identity.seed(file).await),
            Some([byte; 32]),
            "{file:?}"
        );
    }
    let new = identity.keep_seed(SeedFile::RecoverySeed, seed(5)).await;
    assert_eq!(
        new.map(|seed| *seed.expose()),
        Ok([5; 32]),
        "a recovery's new seed is kept in its place"
    );
}

async fn a_recovery_key_once_kept_is_never_replaced(identity: &dyn Identity) {
    let Some((first, second)) = keys() else {
        return;
    };
    assert_eq!(identity.keep_recovery_key(second).await, Ok(second));
    assert_eq!(identity.recovery_key().await, Ok(Some(second)));
    assert_eq!(identity.keep_recovery_key(first).await, Ok(second));
    assert_eq!(identity.recovery_key().await, Ok(Some(second)));
}

async fn a_tls_key_once_kept_is_never_replaced(identity: &dyn Identity) {
    let (Some(first), Some(second)) = (tls_key(0x30), tls_key(0x31)) else {
        return;
    };
    let kept = identity.keep_tls_key(first).await;
    assert_eq!(kept.map(|key| key.der().to_vec()), Ok(vec![0x30; 138]));
    let again = identity.keep_tls_key(second).await;
    assert_eq!(again.map(|key| key.der().to_vec()), Ok(vec![0x30; 138]));
    let read = identity.tls_key().await;
    assert_eq!(
        read.map(|key| key.map(|key| key.der().to_vec())),
        Ok(Some(vec![0x30; 138]))
    );
}

async fn a_pin_is_replaced_whole(identity: &dyn Identity) {
    let (Some(first), Some(second)) = (pin(1), pin(2)) else {
        return;
    };
    assert_eq!(identity.write_pin(&first).await, Ok(()));
    assert_eq!(identity.pin().await, Ok(Some(first)));
    assert_eq!(identity.write_pin(&second).await, Ok(()));
    assert_eq!(identity.pin().await, Ok(Some(second)));
}

async fn each_attempt_takes_the_next_counter(identity: &dyn Identity) {
    let mut counters = Vec::new();
    for _ in 0..3 {
        let attempt = identity.begin_attempt().await;
        assert!(attempt.is_ok(), "an attempt starts: {attempt:?}");
        counters.extend(attempt.map(|attempt| attempt.counter()));
    }
    assert_eq!(
        counters,
        [1, 2, 3].map(ConnectionCounter::new),
        "with no counter kept, the first attempt is 1"
    );
}

async fn one_attempt_holds_the_counter_at_a_time(identity: &dyn Identity) {
    let first = identity.begin_attempt().await;
    assert_eq!(
        first.as_ref().map(|attempt| attempt.counter()).ok(),
        Some(ConnectionCounter::new(4))
    );
    assert_eq!(
        identity
            .begin_attempt()
            .await
            .map(|attempt| attempt.counter()),
        Err(IdentityError::AttemptInFlight),
        "a second attempt is refused while the first runs"
    );
    drop(first);
    assert_eq!(
        identity
            .begin_attempt()
            .await
            .map(|attempt| attempt.counter()),
        Ok(ConnectionCounter::new(5)),
        "the refused attempt took no counter"
    );
}
