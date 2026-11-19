use std::fs;

use domain::fleet::{
    COUNTER_LOCK_FILE, ConnectionCounter, Fingerprint, HubAddress, KeyFile, TokenTls,
};

use super::*;

/// RFC 8032 §7.1, test 1's key.
const KEY: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

/// A canary, never a real key (§13.9).
const CANARY_SEED: [u8; 32] = *b"canary-seed-canary-seed-canary-s";

fn identity() -> (tempfile::TempDir, FileIdentity) {
    let data = tempfile::tempdir().unwrap();
    let identity = FileIdentity::in_data_dir(data.path());
    (data, identity)
}

fn path(data: &tempfile::TempDir, file: KeyFile) -> PathBuf {
    data.path().join(file.file_name())
}

/// Writes `text` as `file`, its owner's alone, as Ladderback writes it.
fn write(data: &tempfile::TempDir, file: KeyFile, text: impl AsRef<[u8]>) {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut written = options.open(path(data, file)).unwrap();
    std::io::Write::write_all(&mut written, text.as_ref()).unwrap();
}

fn pin() -> HubPin {
    HubPin::new(
        PublicKey::from_hex(KEY).unwrap(),
        TokenTls::Pinned(Fingerprint::from_digest([0x5a; 32])),
        PublicKey::from_hex(KEY).unwrap(),
        HubAddress::parse("https://hub.example:9898").unwrap(),
    )
}

/// The names in the data folder, sorted.
fn names(data: &tempfile::TempDir) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(data.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn files_meet_the_identity_contract() {
    let (_data, identity) = identity();
    app::contract::identity::run(&identity).await;
}

/// Decision 120: `hub-key` of one line, the seed as 64 lower-case hex
/// digits; the others as this module says.
#[tokio::test]
async fn each_file_holds_its_text() {
    let (data, identity) = identity();
    identity
        .keep_seed(SeedFile::HubKey, SigningSeed::from_bytes([0xab; 32]))
        .await
        .unwrap();
    identity
        .keep_recovery_key(PublicKey::from_hex(KEY).unwrap())
        .await
        .unwrap();
    identity
        .keep_tls_key(TlsKey::from_der((0..=99).collect()).unwrap())
        .await
        .unwrap();
    identity.write_pin(&pin()).await.unwrap();
    drop(identity.begin_attempt().await.unwrap());
    let read = |file| fs::read_to_string(path(&data, file)).unwrap();
    assert_eq!(read(KeyFile::HubKey), format!("{}\n", "ab".repeat(32)));
    assert_eq!(read(KeyFile::RecoveryKey), format!("{KEY}\n"));
    assert_eq!(
        read(KeyFile::HubTlsKey),
        "-----BEGIN PRIVATE KEY-----\n\
         AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4v\n\
         MDEyMzQ1Njc4OTo7PD0+P0BBQkNERUZHSElKS0xNTk9QUVJTVFVWV1hZWltcXV5f\n\
         YGFiYw==\n\
         -----END PRIVATE KEY-----\n"
    );
    assert_eq!(read(KeyFile::HubPin), pin().to_text());
    assert_eq!(read(KeyFile::ConnectionCounter), "1\n");
}

#[cfg(unix)]
#[tokio::test]
async fn every_file_is_its_owners_alone() {
    use std::os::unix::fs::PermissionsExt;

    let (data, identity) = identity();
    for file in SeedFile::ALL {
        identity
            .keep_seed(file, SigningSeed::from_bytes(CANARY_SEED))
            .await
            .unwrap();
    }
    identity
        .keep_recovery_key(PublicKey::from_hex(KEY).unwrap())
        .await
        .unwrap();
    identity
        .keep_tls_key(TlsKey::from_der(vec![0x30; 138]).unwrap())
        .await
        .unwrap();
    identity.write_pin(&pin()).await.unwrap();
    identity.write_pin(&pin()).await.unwrap();
    drop(identity.begin_attempt().await.unwrap());
    drop(identity.begin_attempt().await.unwrap());
    let mut expected: Vec<String> = KeyFile::ALL.map(|file| file.file_name().to_owned()).into();
    expected.push(COUNTER_LOCK_FILE.to_owned());
    expected.sort();
    assert_eq!(names(&data), expected, "nothing staged is left behind");
    for name in expected {
        let mode = fs::metadata(data.path().join(&name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{name}");
    }
}

/// A file a person restored by hand, without its line break and its
/// owner's alone, is read.
#[tokio::test]
async fn files_written_by_hand_are_read() {
    let (data, identity) = identity();
    write(&data, KeyFile::AgentKey, "cd".repeat(32));
    write(&data, KeyFile::RecoveryKey, KEY);
    write(&data, KeyFile::ConnectionCounter, "41");
    write(
        &data,
        KeyFile::HubTlsKey,
        "-----BEGIN PRIVATE KEY-----\nMDEy\n-----END PRIVATE KEY-----",
    );
    let seed = identity.seed(SeedFile::AgentKey).await.unwrap().unwrap();
    assert_eq!(seed.expose(), &[0xcd; 32]);
    assert_eq!(
        identity.recovery_key().await,
        Ok(Some(PublicKey::from_hex(KEY).unwrap()))
    );
    assert_eq!(identity.tls_key().await.unwrap().unwrap().der(), b"012");
    assert_eq!(
        identity.begin_attempt().await.unwrap().counter(),
        ConnectionCounter::new(42)
    );
}

/// A damaged file is an error, never silently replaced: a new key would
/// lose every pairing, and a counter that started again would make this
/// agent look like a copy of itself.
#[tokio::test]
async fn a_damaged_file_is_an_error_and_left_alone() {
    let (data, identity) = identity();
    let damaged = [
        (KeyFile::AgentKey, "AB".repeat(32)),
        (KeyFile::HubKey, format!("{}\n\n", "ab".repeat(32))),
        (KeyFile::RecoverySeed, "ab".repeat(31)),
        (KeyFile::RecoveryKey, "00".repeat(32)),
        (
            KeyFile::HubTlsKey,
            "-----BEGIN CERTIFICATE-----\nMDEy\n-----END CERTIFICATE-----\n".to_owned(),
        ),
        (
            KeyFile::HubPin,
            pin().to_text().replace("generation", "Generation"),
        ),
        (KeyFile::ConnectionCounter, "-1\n".to_owned()),
    ];
    for (file, text) in &damaged {
        write(&data, *file, text);
    }
    fn error<T>(file: KeyFile) -> Result<T, IdentityError> {
        Err(IdentityError::Damaged { file })
    }
    for seed_file in SeedFile::ALL {
        let file = seed_file.key_file();
        assert_eq!(
            identity.seed(seed_file).await.map(|_| ()),
            error(file),
            "{file}"
        );
        assert_eq!(
            identity
                .keep_seed(seed_file, SigningSeed::from_bytes([1; 32]))
                .await
                .map(|_| ()),
            error(file),
            "{file}"
        );
    }
    assert_eq!(identity.recovery_key().await, error(KeyFile::RecoveryKey));
    assert_eq!(
        identity
            .keep_recovery_key(PublicKey::from_hex(KEY).unwrap())
            .await,
        error(KeyFile::RecoveryKey)
    );
    assert_eq!(
        identity.tls_key().await.map(|_| ()),
        error(KeyFile::HubTlsKey)
    );
    assert_eq!(identity.pin().await, error(KeyFile::HubPin));
    assert_eq!(
        identity.begin_attempt().await.map(|_| ()),
        error(KeyFile::ConnectionCounter)
    );
    for (file, text) in &damaged {
        assert_eq!(
            &fs::read_to_string(path(&data, *file)).unwrap(),
            text,
            "{file}"
        );
    }
}

/// A key file other users can read or change is refused, never used, and
/// left as it is: Ladderback writes each one `0600`, so another mode means
/// it was changed after, and a key others can read or replace is no longer
/// this machine's alone (#1137).
#[cfg(unix)]
#[tokio::test]
async fn a_key_file_others_can_read_or_change_is_refused_and_left_alone() {
    use std::os::unix::fs::PermissionsExt;

    let pem = "-----BEGIN PRIVATE KEY-----\nMDEy\n-----END PRIVATE KEY-----\n";
    for mode in [0o640, 0o604, 0o620, 0o602, 0o610, 0o601, 0o666] {
        let (data, identity) = identity();
        let files = [
            (KeyFile::AgentKey, "cd".repeat(32)),
            (KeyFile::HubKey, "ef".repeat(32)),
            (KeyFile::RecoverySeed, "12".repeat(32)),
            (KeyFile::RecoveryKey, KEY.to_owned()),
            (KeyFile::HubTlsKey, pem.to_owned()),
            (KeyFile::HubPin, pin().to_text()),
            (KeyFile::ConnectionCounter, "41\n".to_owned()),
        ];
        for (file, text) in &files {
            write(&data, *file, text);
            fs::set_permissions(path(&data, *file), fs::Permissions::from_mode(mode)).unwrap();
        }
        let exposed = |file| Err(IdentityError::Exposed { file, mode });
        for seed_file in SeedFile::ALL {
            assert_eq!(
                identity.seed(seed_file).await.map(|_| ()),
                exposed(seed_file.key_file()),
                "{mode:o}"
            );
        }
        assert_eq!(
            identity
                .keep_seed(SeedFile::AgentKey, SigningSeed::from_bytes([1; 32]))
                .await
                .map(|_| ()),
            exposed(KeyFile::AgentKey),
            "{mode:o}"
        );
        assert_eq!(
            identity.recovery_key().await.map(|_| ()),
            exposed(KeyFile::RecoveryKey),
            "{mode:o}"
        );
        assert_eq!(
            identity.tls_key().await.map(|_| ()),
            exposed(KeyFile::HubTlsKey),
            "{mode:o}"
        );
        assert_eq!(
            identity.pin().await.map(|_| ()),
            exposed(KeyFile::HubPin),
            "{mode:o}"
        );
        assert_eq!(
            identity.begin_attempt().await.map(|_| ()),
            exposed(KeyFile::ConnectionCounter),
            "{mode:o}"
        );
        for (file, text) in &files {
            let at = path(&data, *file);
            assert_eq!(&fs::read_to_string(&at).unwrap(), text, "{file}");
            let left = fs::metadata(&at).unwrap().permissions().mode() & 0o777;
            assert_eq!(left, mode, "{file}");
        }
    }
}

/// A key file only its owner can read or change is read whatever else its
/// mode says: no group or world bit is what counts (#1137).
#[cfg(unix)]
#[tokio::test]
async fn a_key_file_only_its_owner_can_use_is_read_whatever_its_owner_bits() {
    use std::os::unix::fs::PermissionsExt;

    for mode in [0o400, 0o600, 0o700] {
        let (data, identity) = identity();
        write(&data, KeyFile::AgentKey, "cd".repeat(32));
        fs::set_permissions(
            path(&data, KeyFile::AgentKey),
            fs::Permissions::from_mode(mode),
        )
        .unwrap();
        assert!(
            identity.seed(SeedFile::AgentKey).await.unwrap().is_some(),
            "{mode:o}"
        );
    }
}

/// A file far past what it can hold isn't read whole.
#[tokio::test]
async fn an_oversized_file_is_damaged() {
    let (data, identity) = identity();
    let long = "ab".repeat(1 << 20);
    write(&data, KeyFile::HubKey, &long);
    write(&data, KeyFile::HubPin, &long);
    write(&data, KeyFile::HubTlsKey, &long);
    assert_eq!(
        identity.seed(SeedFile::HubKey).await.map(|_| ()),
        Err(IdentityError::Damaged {
            file: KeyFile::HubKey
        })
    );
    assert_eq!(
        identity.pin().await,
        Err(IdentityError::Damaged {
            file: KeyFile::HubPin
        })
    );
    assert_eq!(
        identity.tls_key().await.map(|_| ()),
        Err(IdentityError::Damaged {
            file: KeyFile::HubTlsKey
        })
    );
}

/// I4: an error names the file, never what it holds.
#[tokio::test]
async fn an_error_never_shows_the_files_text() {
    let (data, identity) = identity();
    let canary = String::from_utf8(CANARY_SEED.to_vec()).unwrap();
    write(&data, KeyFile::HubKey, format!("{canary}\n"));
    let error = identity.seed(SeedFile::HubKey).await.unwrap_err();
    assert!(!format!("{error:?}").contains("canary"), "{error:?}");
}

#[tokio::test]
async fn a_missing_data_folder_is_made() {
    let root = tempfile::tempdir().unwrap();
    let identity = FileIdentity::in_data_dir(&root.path().join("data"));
    let kept = identity
        .keep_seed(SeedFile::AgentKey, SigningSeed::from_bytes([7; 32]))
        .await
        .unwrap();
    assert_eq!(kept.expose(), &[7; 32]);
    identity.write_pin(&pin()).await.unwrap();
    assert_eq!(identity.pin().await, Ok(Some(pin())));
}

/// Two processes on one data folder: each its own handle, so only the
/// file's lock stands between them.
#[tokio::test]
async fn an_attempt_holds_the_counter_against_another_process() {
    let (data, first) = identity();
    let second = FileIdentity::in_data_dir(data.path());
    let attempt = first.begin_attempt().await.unwrap();
    assert_eq!(
        fs::read_to_string(path(&data, KeyFile::ConnectionCounter)).unwrap(),
        "1\n",
        "written before the attempt starts"
    );
    assert_eq!(
        second.begin_attempt().await.map(|_| ()),
        Err(IdentityError::AttemptInFlight)
    );
    drop(attempt);
    assert_eq!(
        second.begin_attempt().await.unwrap().counter(),
        ConnectionCounter::new(2),
        "the counter carries over to another process"
    );
}

#[tokio::test]
async fn a_counter_at_the_most_it_holds_starts_no_attempt() {
    let (data, identity) = identity();
    let most = format!("{}\n", u64::MAX);
    write(&data, KeyFile::ConnectionCounter, &most);
    assert_eq!(
        identity.begin_attempt().await.map(|_| ()),
        Err(IdentityError::CounterExhausted)
    );
    assert_eq!(
        fs::read_to_string(path(&data, KeyFile::ConnectionCounter)).unwrap(),
        most
    );
    assert_eq!(
        identity.begin_attempt().await.map(|_| ()),
        Err(IdentityError::CounterExhausted),
        "the refused attempt let go of the lock"
    );
}

#[tokio::test]
async fn concurrent_first_starts_agree_on_one_seed() {
    let (data, _identity) = identity();
    let folder = data.path().to_owned();
    let tasks: Vec<_> = (0..8u8)
        .map(|byte| {
            let identity = FileIdentity::in_data_dir(&folder);
            tokio::spawn(async move {
                *identity
                    .keep_seed(SeedFile::HubKey, SigningSeed::from_bytes([byte; 32]))
                    .await
                    .unwrap()
                    .expose()
            })
        })
        .collect();
    let mut seeds = Vec::new();
    for task in tasks {
        seeds.push(task.await.unwrap());
    }
    assert!(seeds.iter().all(|seed| *seed == seeds[0]), "{seeds:?}");
    assert_eq!(names(&data), ["hub-key"], "only the key file remains");
}

#[tokio::test]
async fn forgetting_the_recovery_seed_deletes_its_file_alone() {
    let (data, identity) = identity();
    for file in SeedFile::ALL {
        identity
            .keep_seed(file, SigningSeed::from_bytes([9; 32]))
            .await
            .unwrap();
    }
    identity.forget_recovery_seed().await.unwrap();
    assert_eq!(names(&data), ["agent-key", "hub-key"]);
}

/// A write cut short by a crash leaves its staged copy, which holds the
/// seed too: forgetting the seed forgets it, and leaves another file's
/// staged copy alone.
#[tokio::test]
async fn forgetting_the_recovery_seed_deletes_its_staged_copies_too() {
    let (data, identity) = identity();
    identity
        .keep_seed(SeedFile::RecoverySeed, SigningSeed::from_bytes([9; 32]))
        .await
        .unwrap();
    for staged in [
        "hub-recovery-seed.0190a1b2c3d4.tmp",
        "hub-recovery-seed.0190a1b2c3d5.tmp",
        "hub-key.0190a1b2c3d6.tmp",
    ] {
        fs::write(data.path().join(staged), "canary\n").unwrap();
    }
    identity.forget_recovery_seed().await.unwrap();
    assert_eq!(names(&data), ["hub-key.0190a1b2c3d6.tmp"]);
}
