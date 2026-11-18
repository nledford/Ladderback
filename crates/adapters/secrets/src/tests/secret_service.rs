//! The OS store against a stand-in Secret Service on a private D-Bus: what
//! `keyring` reads from a service that answers as oo7 does (#483), and what it
//! keeps in it and deletes from it.
//!
//! The store finds the bus through `DBUS_SESSION_BUS_ADDRESS`, which a test
//! can't set in its own process, so the test runs itself again with the
//! private bus's address set, and that copy does the reading.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::Connection;
use dbus::channel::{Channel, MatchingReceiver, Sender};
use dbus::message::{MatchRule, Message};
use dbus::strings::{ErrorName, Path};
use tempfile::TempDir;

use app::ports::KeyringActivation;

use crate::keyring_store::{Failure, activation_on, os_activation, os_store};

/// Set in the copy, to what it does: the state of the stand-in's keyring, or
/// the step of the store's call.
const IN_COPY: &str = "LADDERBACK_SECRETS_STAND_IN_COPY";
const FINDS_NOTHING: &str = "tests::secret_service::a_locked_keyring_that_finds_nothing_is_locked";
const KEEPS: &str =
    "tests::secret_service::a_secret_set_in_the_keyring_reads_back_until_it_is_deleted";
const NO_SERVICE: &str =
    "tests::secret_service::a_session_bus_with_no_secret_service_on_it_is_no_keyring_service";
const ASKS_THE_SESSION: &str =
    "tests::secret_service::the_session_bus_is_the_one_asked_how_the_keyring_is_started";

/// Where the stand-in keeps its one collection, which the alias `default`
/// names.
const COLLECTION: &str = "/org/freedesktop/secrets/collection/login";

/// A Secret Service whose default collection is locked and, as oo7 does,
/// finds no items in it while it is, though the entry is there: the entry is
/// refused as a locked keyring, in the words `keyring` gives a locked
/// collection, never as missing (#483). The same service unlocked, finding
/// nothing, has no such entry; one whose collection doesn't say whether it is
/// locked leaves the missing entry unconfirmed, in the words of the call that
/// failed.
#[test]
fn a_locked_keyring_that_finds_nothing_is_locked() {
    if let Some(state) = std::env::var_os(IN_COPY) {
        let read = os_store().get("ladderback-demo", "nas");
        if state == Keyring::Locked.name() {
            let said =
                keyring::Error::NoStorageAccess(Box::new(dbus_secret_service::Error::Locked))
                    .to_string();
            assert_eq!(read, Err(Failure::Locked(said)));
        } else if state == Keyring::Silent.name() {
            assert!(
                matches!(&read, Err(Failure::Unconfirmed(why)) if why.contains("the stand-in has no such method")),
                "{read:?}"
            );
        } else {
            assert_eq!(read, Err(Failure::NoEntry));
        }
        return;
    }
    let Some(bus) = PrivateBus::start() else {
        testkit::skip!("no dbus-daemon to run a private bus with");
    };
    let held = Arc::new(Mutex::new(Held::new(Keyring::Locked)));
    let service = StandIn::serve(&bus.address, Arc::clone(&held));
    for state in [Keyring::Locked, Keyring::Unlocked, Keyring::Silent] {
        held.lock().unwrap().keyring = state;
        run_in_copy(FINDS_NOTHING, &bus.address, state.name());
    }
    service.stop();
}

/// A secret the store sets is kept as one item under the entry's service and
/// name, reads back as it was set, and is gone once the store deletes it: the
/// entry is then missing.
#[test]
fn a_secret_set_in_the_keyring_reads_back_until_it_is_deleted() {
    if let Some(step) = std::env::var_os(IN_COPY) {
        let store = os_store();
        match step.to_str() {
            Some("set") => assert_eq!(store.set("ladderback-demo", "nas", "hunter2"), Ok(())),
            Some("get") => assert_eq!(
                store.get("ladderback-demo", "nas"),
                Ok("hunter2".to_owned())
            ),
            Some("delete") => assert_eq!(store.delete("ladderback-demo", "nas"), Ok(())),
            _ => assert_eq!(store.get("ladderback-demo", "nas"), Err(Failure::NoEntry)),
        }
        return;
    }
    let Some(bus) = PrivateBus::start() else {
        testkit::skip!("no dbus-daemon to run a private bus with");
    };
    let held = Arc::new(Mutex::new(Held::new(Keyring::Unlocked)));
    let service = StandIn::serve(&bus.address, Arc::clone(&held));
    run_in_copy(KEEPS, &bus.address, "set");
    {
        let held = held.lock().unwrap();
        let named: Vec<_> = held
            .items
            .iter()
            .map(|item| (item.attribute("service"), item.attribute("username")))
            .collect();
        assert_eq!(named, [(Some("ladderback-demo"), Some("nas"))]);
    }
    run_in_copy(KEEPS, &bus.address, "get");
    run_in_copy(KEEPS, &bus.address, "delete");
    assert_eq!(held.lock().unwrap().items.len(), 0);
    run_in_copy(KEEPS, &bus.address, "gone");
    service.stop();
}

/// A session bus that no Secret Service is on yet, as at login before the
/// session's keyring starts: the read fails as no keyring service to ask,
/// which a run the schedule starts then waits out (§5), never as a keyring
/// that refused, which no run waits for.
#[test]
fn a_session_bus_with_no_secret_service_on_it_is_no_keyring_service() {
    if std::env::var_os(IN_COPY).is_some() {
        let read = os_store().get("ladderback-demo", "nas");
        assert!(matches!(&read, Err(Failure::NoService(_))), "{read:?}");
        return;
    }
    let Some(bus) = PrivateBus::start() else {
        testkit::skip!("no dbus-daemon to run a private bus with");
    };
    run_in_copy(NO_SERVICE, &bus.address, "no service");
}

/// How the Secret Service is started is what the session bus says: on
/// demand when an activation file names it, running or not; running only
/// when it owns its name and no file names it, the one answer the
/// Repositories page notes (#923); absent when neither.
#[test]
fn how_the_keyring_is_started_is_what_the_session_bus_says() {
    if PrivateBus::start().is_none() {
        testkit::skip!("no dbus-daemon to run a private bus with");
    }
    for (activatable, running, said) in [
        (true, true, KeyringActivation::OnDemand),
        (true, false, KeyringActivation::OnDemand),
        (false, true, KeyringActivation::RunningOnly),
        (false, false, KeyringActivation::Absent),
    ] {
        let bus = PrivateBus::start_with(Setup {
            activatable,
            denied: &[],
        })
        .unwrap();
        let service = running.then(|| {
            StandIn::serve(
                &bus.address,
                Arc::new(Mutex::new(Held::new(Keyring::Unlocked))),
            )
        });
        assert_eq!(
            activation_on(&connect(&bus.address)),
            said,
            "activatable: {activatable}, running: {running}"
        );
        if let Some(service) = service {
            service.stop();
        }
    }
}

/// A session bus that refuses either question leaves the answer unknown,
/// which the Repositories page notes nothing of: never running only.
#[test]
fn a_session_bus_that_refuses_the_question_leaves_it_unknown() {
    if PrivateBus::start().is_none() {
        testkit::skip!("no dbus-daemon to run a private bus with");
    }
    for denied in [&["ListActivatableNames"], &["NameHasOwner"]] {
        let bus = PrivateBus::start_with(Setup {
            activatable: false,
            denied,
        })
        .unwrap();
        let service = StandIn::serve(
            &bus.address,
            Arc::new(Mutex::new(Held::new(Keyring::Unlocked))),
        );
        assert_eq!(
            activation_on(&connect(&bus.address)),
            KeyringActivation::Unknown,
            "{denied:?} refused"
        );
        service.stop();
    }
}

/// The session's own bus is the one asked, as `DBUS_SESSION_BUS_ADDRESS`
/// names it, and a session with no bus there leaves the answer unknown.
#[test]
fn the_session_bus_is_the_one_asked_how_the_keyring_is_started() {
    if let Some(expected) = std::env::var_os(IN_COPY) {
        let said = if expected == "running only" {
            KeyringActivation::RunningOnly
        } else {
            KeyringActivation::Unknown
        };
        assert_eq!(os_activation(), said);
        return;
    }
    let Some(bus) = PrivateBus::start() else {
        testkit::skip!("no dbus-daemon to run a private bus with");
    };
    let service = StandIn::serve(
        &bus.address,
        Arc::new(Mutex::new(Held::new(Keyring::Unlocked))),
    );
    run_in_copy(ASKS_THE_SESSION, &bus.address, "running only");
    service.stop();
    let nowhere = format!(
        "unix:path={}",
        bus._directory.path().join("no-bus").display()
    );
    run_in_copy(ASKS_THE_SESSION, &nowhere, "no bus");
}

/// A connection of its own to the bus at `address`.
fn connect(address: &str) -> Connection {
    let mut channel = Channel::open_private(address).unwrap();
    channel.register().unwrap();
    Connection::from(channel)
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

/// What the stand-in holds: its keyring's state and the items kept in it.
struct Held {
    keyring: Keyring,
    items: Vec<Item>,
    /// How many items were ever created, which numbers the next one's path.
    created: usize,
}

impl Held {
    fn new(keyring: Keyring) -> Self {
        Self {
            keyring,
            items: Vec::new(),
            created: 0,
        }
    }

    /// The paths of the items whose attributes hold all of `wanted`: none
    /// while the keyring is locked, as oo7 finds.
    fn search(&self, wanted: &HashMap<String, String>) -> Vec<Path<'static>> {
        if self.keyring == Keyring::Locked {
            return Vec::new();
        }
        self.items
            .iter()
            .filter(|item| {
                wanted
                    .iter()
                    .all(|(key, value)| item.attributes.get(key) == Some(value))
            })
            .map(|item| Path::from(item.path.clone()))
            .collect()
    }
}

/// An item of the stand-in's collection, its secret kept as it was sent.
struct Item {
    path: String,
    attributes: HashMap<String, String>,
    parameters: Vec<u8>,
    value: Vec<u8>,
    content_type: String,
}

impl Item {
    fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).map(String::as_str)
    }
}

/// Runs the test named `test` again on the private bus at `address`, telling
/// the copy `what` to do, and checks that the copy passed. A copy still
/// running after its bound (a stand-in that doesn't answer) is killed and
/// fails.
fn run_in_copy(test: &str, address: &str, what: &str) {
    let mut copy = Command::new(std::env::current_exe().unwrap());
    copy.args(["--exact", test, "--nocapture"])
        .env(IN_COPY, what)
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
                "{what}: the copy was still running after {bound:?}: {}",
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
        "{what}: {stdout}{stderr}"
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
        Self::start_with(Setup {
            activatable: false,
            denied: &[],
        })
    }

    /// As `start`, set up as `setup` says.
    fn start_with(setup: Setup) -> Option<Self> {
        let directory = TempDir::new().unwrap();
        let config = directory.path().join("bus.conf");
        let socket = directory.path().join("bus");
        let services = directory.path().join("services");
        fs::create_dir(&services).unwrap();
        if setup.activatable {
            fs::write(
                services.join("org.freedesktop.secrets.service"),
                "[D-BUS Service]\nName=org.freedesktop.secrets\nExec=/bin/false\n",
            )
            .unwrap();
        }
        let denied: String = setup
            .denied
            .iter()
            .map(|member| {
                format!(
                    r#"    <deny send_destination="org.freedesktop.DBus" send_interface="org.freedesktop.DBus" send_member="{member}"/>
"#
                )
            })
            .collect();
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
  <servicedir>{}</servicedir>
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
{denied}  </policy>
</busconfig>
"#,
                socket.display(),
                services.display()
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

/// How a private bus is set up.
struct Setup {
    /// An activation file names the Secret Service.
    activatable: bool,
    /// The methods of the bus itself that it refuses to answer.
    denied: &'static [&'static str],
}

/// A Secret Service of one collection, which keeps the items created in it
/// and, as oo7 does, finds none of them while the collection is locked. It
/// keeps a secret as it was sent and never decrypts it: its public key is 1,
/// so every session shares the same key, and a secret sent in one session
/// reads in the next.
struct StandIn {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl StandIn {
    /// Serves on the bus at `address` once it owns the service's name, its
    /// keyring and items as `held` is when asked.
    fn serve(address: &str, held: Arc<Mutex<Held>>) -> Self {
        let mut channel = Channel::open_private(address).unwrap();
        channel.register().unwrap();
        let connection = Connection::from(channel);
        connection
            .request_name("org.freedesktop.secrets", false, true, true)
            .unwrap();
        connection.start_receive(
            MatchRule::new_method_call(),
            Box::new(move |call, connection| {
                let _ = connection.send(answer(&call, &mut held.lock().unwrap()));
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

/// The stand-in's answer to `call`, its keyring and items being as `held` is:
/// the methods `keyring` and the store call, and an unknown method for any
/// other.
fn answer(call: &Message, held: &mut Held) -> Message {
    let interface = call.interface().map(|name| name.to_string());
    let member = call.member().map(|name| name.to_string());
    let path = call.path().map(|path| path.to_string());
    match (interface.as_deref(), member.as_deref()) {
        (Some("org.freedesktop.Secret.Service"), Some("OpenSession")) => {
            // A plain session has no output; one with Diffie-Hellman takes
            // the service's public key: 1, whatever the client's is, makes
            // the shared secret 1 and so every session's key the same.
            let output: Box<dyn RefArg> = if call.get1::<String>().as_deref() == Some("plain") {
                Box::new(String::new())
            } else {
                Box::new(vec![1_u8])
            };
            call.method_return().append2(
                Variant(output),
                Path::from("/org/freedesktop/secrets/session/1"),
            )
        }
        (Some("org.freedesktop.Secret.Service"), Some("SearchItems")) => {
            let found = held.search(&call.get1().unwrap_or_default());
            call.method_return().append2(found, Vec::<Path>::new())
        }
        (Some("org.freedesktop.Secret.Service"), Some("ReadAlias")) => {
            let named = if call.get1::<String>().as_deref() == Some("default") {
                COLLECTION
            } else {
                "/"
            };
            call.method_return().append1(Path::from(named))
        }
        (Some("org.freedesktop.Secret.Collection"), Some("SearchItems")) => call
            .method_return()
            .append1(held.search(&call.get1().unwrap_or_default())),
        (Some("org.freedesktop.Secret.Collection"), Some("CreateItem")) => {
            let Ok((properties, (_, parameters, value, content_type), replace)) =
                call.read3::<PropMap, (Path, Vec<u8>, Vec<u8>, String), bool>()
            else {
                return invalid(call);
            };
            let attributes = attributes_of(&properties);
            if replace {
                held.items.retain(|item| item.attributes != attributes);
            }
            held.created += 1;
            let created = format!("{COLLECTION}/{}", held.created);
            held.items.push(Item {
                path: created.clone(),
                attributes,
                parameters,
                value,
                content_type,
            });
            // No prompt: "/".
            call.method_return()
                .append2(Path::from(created), Path::from("/"))
        }
        (Some("org.freedesktop.Secret.Item"), Some("GetSecret")) => {
            let (Ok(session), Some(item)) = (
                call.read1::<Path>(),
                held.items
                    .iter()
                    .find(|item| Some(&item.path) == path.as_ref()),
            ) else {
                return invalid(call);
            };
            call.method_return().append1((
                session,
                item.parameters.clone(),
                item.value.clone(),
                item.content_type.clone(),
            ))
        }
        (Some("org.freedesktop.Secret.Item"), Some("Delete")) => {
            let before = held.items.len();
            held.items.retain(|item| Some(&item.path) != path.as_ref());
            if held.items.len() == before {
                return invalid(call);
            }
            // No prompt: "/".
            call.method_return().append1(Path::from("/"))
        }
        (Some("org.freedesktop.DBus.Properties"), Some("Get"))
            if held.keyring != Keyring::Silent
                && path.as_deref() == Some(COLLECTION)
                && call
                    .read2::<String, String>()
                    .is_ok_and(|(interface, property)| {
                        interface == "org.freedesktop.Secret.Collection" && property == "Locked"
                    }) =>
        {
            call.method_return()
                .append1(Variant(held.keyring == Keyring::Locked))
        }
        _ => call.error(
            &ErrorName::from("org.freedesktop.DBus.Error.UnknownMethod"),
            c"the stand-in has no such method",
        ),
    }
}

/// The answer to a call the stand-in can't take: arguments it can't read, or
/// an item it doesn't hold.
fn invalid(call: &Message) -> Message {
    call.error(
        &ErrorName::from("org.freedesktop.DBus.Error.InvalidArgs"),
        c"the stand-in can't take this call",
    )
}

/// The attributes among an item's `properties`, as `CreateItem` sends them.
fn attributes_of(properties: &PropMap) -> HashMap<String, String> {
    let mut attributes = HashMap::new();
    if let Some(Variant(map)) = properties.get("org.freedesktop.Secret.Item.Attributes") {
        // A map reads as its keys and values in turn.
        let mut entries = map.as_iter().into_iter().flatten();
        while let (Some(key), Some(value)) = (entries.next(), entries.next()) {
            if let (Some(key), Some(value)) = (key.as_str(), value.as_str()) {
                attributes.insert(key.to_owned(), value.to_owned());
            }
        }
    }
    attributes
}
