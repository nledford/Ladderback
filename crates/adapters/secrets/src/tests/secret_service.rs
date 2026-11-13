//! The OS store against a stand-in Secret Service on a private D-Bus: what
//! `keyring` reads from a service that answers as oo7 does (#483).
//!
//! The store finds the bus through `DBUS_SESSION_BUS_ADDRESS`, which a test
//! can't set in its own process, so the test runs itself again with the
//! private bus's address set, and that copy does the reading.

use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use dbus::arg::{RefArg, Variant};
use dbus::blocking::Connection;
use dbus::channel::{Channel, MatchingReceiver, Sender};
use dbus::message::{MatchRule, Message};
use dbus::strings::{ErrorName, Path};
use tempfile::TempDir;

use crate::keyring_store::{Failure, os_store};

/// Set in the copy, to the state of the stand-in's keyring.
const IN_COPY: &str = "LADDERBACK_SECRETS_STAND_IN_COPY";
const NAME: &str =
    "tests::secret_service::a_locked_keyring_that_finds_nothing_is_no_keyring_service";

/// Where the stand-in keeps its one collection, which the alias `default`
/// names.
const COLLECTION: &str = "/org/freedesktop/secrets/collection/login";

/// A Secret Service whose default collection is locked and, as oo7 does,
/// finds no items in it while it is, though the entry is there: the entry is
/// refused as no keyring service to ask, in the words `keyring` gives a
/// locked collection, never as missing (#483). The same service unlocked,
/// finding nothing, has no such entry, and so has one whose collection
/// doesn't say whether it is locked.
#[test]
fn a_locked_keyring_that_finds_nothing_is_no_keyring_service() {
    if let Some(state) = std::env::var_os(IN_COPY) {
        let read = os_store().get("ladderback-demo", "nas");
        if state == Keyring::Locked.name() {
            let said =
                keyring::Error::NoStorageAccess(Box::new(dbus_secret_service::Error::Locked))
                    .to_string();
            assert_eq!(read, Err(Failure::NoService(said)));
        } else {
            assert_eq!(read, Err(Failure::NoEntry));
        }
        return;
    }
    let Some(bus) = PrivateBus::start() else {
        testkit::skip!("no dbus-daemon to run a private bus with");
    };
    let keyring = Arc::new(Mutex::new(Keyring::Locked));
    let service = StandIn::serve(&bus.address, Arc::clone(&keyring));
    for state in [Keyring::Locked, Keyring::Unlocked, Keyring::Silent] {
        *keyring.lock().unwrap() = state;
        read_in_copy(&bus.address, state.name());
    }
    service.stop();
}

/// The stand-in's keyring.
#[derive(Clone, Copy, PartialEq)]
enum Keyring {
    /// The collection under the alias `default` is locked.
    Locked,
    /// The collection under the alias `default` is unlocked.
    Unlocked,
    /// The collection under the alias `default` doesn't say whether it is
    /// locked.
    Silent,
}

impl Keyring {
    /// The state's name, as the copy is told it.
    fn name(self) -> &'static str {
        match self {
            Self::Locked => "locked",
            Self::Unlocked => "unlocked",
            Self::Silent => "silent",
        }
    }
}

/// Runs this test again on the private bus at `address`, with the stand-in's
/// keyring `state`, and checks that the copy passed. A copy still running
/// after its bound (a stand-in that doesn't answer) is killed and fails.
fn read_in_copy(address: &str, state: &str) {
    let mut copy = Command::new(std::env::current_exe().unwrap());
    copy.args(["--exact", NAME, "--nocapture"])
        .env(IN_COPY, state)
        .env("DBUS_SESSION_BUS_ADDRESS", address)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_gate::hold(|| copy.spawn()).unwrap();
    let bound = Duration::from_secs(10);
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > bound {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "{state}: the copy was still running after {bound:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // "1 passed" shows that the copy ran the test, and did not just match none.
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{state}: {stdout}{stderr}"
    );
}

/// A session bus of the test's own, which no other program is on.
struct PrivateBus {
    daemon: Child,
    address: String,
    /// Holds the bus's socket and configuration until the bus is dropped.
    _directory: TempDir,
}

impl PrivateBus {
    /// `None` when this machine has no `dbus-daemon`.
    fn start() -> Option<Self> {
        let directory = TempDir::new().unwrap();
        let config = directory.path().join("bus.conf");
        let socket = directory.path().join("bus");
        // dbus-daemon refuses a message no rule allows to be sent or
        // received, even the bus's own reply to a client's `Hello`.
        fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                socket.display()
            ),
        )
        .unwrap();
        let mut start = Command::new("dbus-daemon");
        start
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let daemon = match spawn_gate::hold(|| start.spawn()) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            started => started.unwrap(),
        };
        // Held before the address is read, so a failed read kills the bus.
        let mut bus = Self {
            daemon,
            address: String::new(),
            _directory: directory,
        };
        BufReader::new(bus.daemon.stdout.take().unwrap())
            .read_line(&mut bus.address)
            .unwrap();
        bus.address.truncate(bus.address.trim_end().len());
        assert!(!bus.address.is_empty(), "the bus gave no address");
        Some(bus)
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// A Secret Service of one collection, which holds no item it can find:
/// what oo7 answers for a locked collection, and an empty one unlocked.
struct StandIn {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl StandIn {
    /// Serves on the bus at `address` once it owns the service's name, its
    /// keyring as `keyring` is when asked.
    fn serve(address: &str, keyring: Arc<Mutex<Keyring>>) -> Self {
        let mut channel = Channel::open_private(address).unwrap();
        channel.register().unwrap();
        let connection = Connection::from(channel);
        connection
            .request_name("org.freedesktop.secrets", false, true, true)
            .unwrap();
        connection.start_receive(
            MatchRule::new_method_call(),
            Box::new(move |call, connection| {
                let _ = connection.send(answer(&call, *keyring.lock().unwrap()));
                true
            }),
        );
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                connection.process(Duration::from_millis(20)).unwrap();
            }
        });
        Self { stop, thread }
    }

    fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.join().unwrap();
    }
}

/// The stand-in's answer to `call`, its keyring being as `keyring` is: the
/// methods `keyring` and the store call, and an unknown method for any other.
fn answer(call: &Message, keyring: Keyring) -> Message {
    let interface = call.interface().map(|name| name.to_string());
    let member = call.member().map(|name| name.to_string());
    let path = call.path().map(|path| path.to_string());
    match (interface.as_deref(), member.as_deref()) {
        (Some("org.freedesktop.Secret.Service"), Some("OpenSession")) => {
            // A plain session has no output; one with Diffie-Hellman takes
            // the service's public key, which no secret here is sent under.
            let output: Box<dyn RefArg> = if call.get1::<String>().as_deref() == Some("plain") {
                Box::new(String::new())
            } else {
                Box::new(vec![2_u8])
            };
            call.method_return().append2(
                Variant(output),
                Path::from("/org/freedesktop/secrets/session/1"),
            )
        }
        (Some("org.freedesktop.Secret.Service"), Some("SearchItems")) => call
            .method_return()
            .append2(Vec::<Path>::new(), Vec::<Path>::new()),
        (Some("org.freedesktop.Secret.Service"), Some("ReadAlias")) => {
            let named = if call.get1::<String>().as_deref() == Some("default") {
                COLLECTION
            } else {
                "/"
            };
            call.method_return().append1(Path::from(named))
        }
        (Some("org.freedesktop.Secret.Collection"), Some("SearchItems")) => {
            call.method_return().append1(Vec::<Path>::new())
        }
        (Some("org.freedesktop.DBus.Properties"), Some("Get"))
            if keyring != Keyring::Silent
                && path.as_deref() == Some(COLLECTION)
                && call
                    .read2::<String, String>()
                    .is_ok_and(|(interface, property)| {
                        interface == "org.freedesktop.Secret.Collection" && property == "Locked"
                    }) =>
        {
            call.method_return()
                .append1(Variant(keyring == Keyring::Locked))
        }
        _ => call.error(
            &ErrorName::from("org.freedesktop.DBus.Error.UnknownMethod"),
            c"the stand-in has no such method",
        ),
    }
}
