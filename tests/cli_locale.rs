//! The daemon's own messages in a pseudolocale (technical design §8.4): what
//! `ladderback serve` says on stderr with `LANGUAGE=en-XA` or `LANGUAGE=ar-XB`,
//! when it cannot listen because the address is taken. The text of the
//! catalog is transformed; the address it was asked for and the operating
//! system's own words are not, because a person must be able to copy them.
//!
//! The pseudolocales are a feature, on by default and left out of a release
//! build. This file has both halves: with `--no-default-features` the same
//! `LANGUAGE` gets plain English, as an unknown language does.

use std::io::Read;
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const LADDERBACK: &str = env!("CARGO_BIN_EXE_ladderback");

/// How long a daemon that cannot listen gets to say so and exit.
const FAILS_WITHIN: Duration = Duration::from_secs(30);

/// What `ladderback serve` said when it was run on an address that is taken.
struct Said {
    /// The address it was asked to listen on.
    address: String,
    /// What the operating system says of binding it again.
    os_error: String,
    /// The line it printed with its name in front, which is its message.
    message: String,
}

/// Runs `ladderback serve` on an address another socket holds, with
/// `LANGUAGE=language` and no other setting that could name a locale.
fn serve_on_a_taken_address(language: &str) -> Said {
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    // The daemon opens its store and reads its config before it binds; two
    // daemons in the platform's shared folders contend on the same database
    // and that failure, not the taken address, is what would print. Each
    // daemon gets its own empty scratch folders.
    let scratch = std::env::temp_dir().join(format!(
        "ladderback-cli-locale-{}-{}",
        std::process::id(),
        CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("the scratch dir is made");
    let taken = TcpListener::bind("127.0.0.1:0").expect("a free port to take");
    let address = taken.local_addr().expect("the address taken").to_string();
    // Binding it again fails the way the daemon's bind does.
    let os_error = TcpListener::bind(&address)
        .expect_err("the address is taken")
        .to_string();
    let mut command = Command::new(LADDERBACK);
    testkit::daemon::isolate(&mut command, &scratch);
    command
        .args(["serve", "--listen", &address])
        // `LC_ALL`, `LC_MESSAGES` and `LANG` decide first: a `C` among them,
        // as a container sets, makes the program ignore `LANGUAGE`.
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANG")
        .env("LANGUAGE", language)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = spawn_gate::hold(|| command.spawn()).expect("ladderback serve started");
    let deadline = Instant::now() + FAILS_WITHIN;
    let status = loop {
        match child.try_wait().expect("the daemon can be waited for") {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the daemon still runs after {FAILS_WITHIN:?}: it took a taken address");
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("a piped stderr")
        .read_to_string(&mut stderr)
        .expect("stderr is text");
    drop(taken);
    assert!(
        !status.success(),
        "the daemon exits with a failure; stderr: {stderr}"
    );
    let message = stderr
        .lines()
        .find(|line| line.starts_with("ladderback: "))
        .unwrap_or_else(|| panic!("a message with the program's name in {stderr:?}"))
        .to_owned();
    let _ = std::fs::remove_dir_all(&scratch);
    Said {
        address,
        os_error,
        message,
    }
}

#[cfg(feature = "pseudo")]
#[test]
fn en_xa_transforms_the_message_and_leaves_the_address_and_the_systems_words_alone() {
    let said = serve_on_a_taken_address("en-XA");
    assert_eq!(
        said.message,
        format!(
            "ladderback: ƈaaƞƞooŧ ŀişŧeeƞ ooƞ {}: {}",
            said.address, said.os_error
        )
    );
}

#[cfg(feature = "pseudo")]
#[test]
fn ar_xb_transforms_the_message_and_leaves_the_address_and_the_systems_words_alone() {
    let said = serve_on_a_taken_address("ar-XB");
    assert_eq!(
        said.message,
        format!(
            "ladderback: ɔɐuuoʇ ʅısʇǝu ou {}: {}",
            said.address, said.os_error
        )
    );
}

#[cfg(not(feature = "pseudo"))]
#[test]
fn a_build_without_pseudolocales_says_it_in_english_whatever_language_is_asked_for() {
    for language in ["en-XA", "ar-XB"] {
        let said = serve_on_a_taken_address(language);
        assert_eq!(
            said.message,
            format!(
                "ladderback: cannot listen on {}: {}",
                said.address, said.os_error
            ),
            "{language}"
        );
    }
}

/// What `ladderback serve` said, after its name, when `--allowed-host` was given
/// a name it can never match, with `LANGUAGE=language`. The daemon exits before
/// it opens its store or binds, so it needs no folders of its own beyond the
/// isolated ones every daemon a test starts gets.
fn serve_with_a_bad_allowed_host(language: &str, name: &str) -> String {
    let scratch = std::env::temp_dir().join(format!(
        "ladderback-cli-locale-host-{}-{}",
        std::process::id(),
        language
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("the scratch dir is made");
    let mut command = Command::new(LADDERBACK);
    testkit::daemon::isolate(&mut command, &scratch);
    command
        .args(["serve", "--listen", "127.0.0.1:0", "--allowed-host", name])
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANG")
        .env("LANGUAGE", language)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = spawn_gate::hold(|| command.spawn())
        .expect("ladderback serve ran")
        .wait_with_output()
        .expect("ladderback serve ran");
    let _ = std::fs::remove_dir_all(&scratch);
    assert_eq!(output.status.code(), Some(2), "a refused argument exits 2");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    stderr
        .lines()
        .find(|line| line.starts_with("ladderback: "))
        .unwrap_or_else(|| panic!("a message with the program's name in {stderr:?}"))
        .to_owned()
}

#[test]
fn a_host_name_that_can_never_match_is_refused_in_the_catalogs_words() {
    assert_eq!(
        serve_with_a_bad_allowed_host("en-US", "nas.example.com:9898"),
        "ladderback: “nas.example.com:9898” is not a host name or an address without a port, \
         so --allowed-host cannot take it. Give a name such as nas.example.com, or an address \
         such as 192.168.1.5 or [fd00::1]."
    );
}

#[cfg(feature = "pseudo")]
#[test]
fn a_refused_host_name_is_transformed_in_a_pseudolocale_and_the_name_is_not() {
    for language in ["en-XA", "ar-XB"] {
        let message = serve_with_a_bad_allowed_host(language, "nas.example.com:9898");
        assert!(
            !message.is_ascii(),
            "{language} is not transformed: {message}"
        );
        for typed in ["nas.example.com:9898", "--allowed-host", "[fd00::1]"] {
            assert!(
                message.contains(typed),
                "{language} changes {typed}: {message}"
            );
        }
    }
}
