//! The power and network adapters meet the ports' contracts (technical
//! design §3.2), each on a machine the test makes up: a folder laid out as
//! the kernel's, or a stand-in program that prints what `pmset` or `scutil`
//! would. Both adapters run on every platform this way; the system's own
//! sources are only asked for an answer, since their state isn't the test's.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use app::contract;
use app::contract::network::NetworkHarness;
use app::contract::network_link::NetworkLinkHarness;
use app::contract::power::PowerHarness;
use app::ports::{Network, NetworkLink, PlatformError, PowerSupply};
use domain::execution::conditions::PowerSource;
use domain::planning::{CurrentNetworkName, Link};
use platform::network::{ScutilNetwork, SysfsNetwork};
use platform::network_link::{KernelLink, RouteLink};
use platform::power::{PmsetPower, SysfsPower};

/// A short bound for the stand-ins that hang, so the test doesn't.
const SHORT: Duration = Duration::from_millis(300);

/// Folders laid out as `/sys/class/<class>`, one per machine.
struct Sysfs {
    scratch: tempfile::TempDir,
    machines: AtomicUsize,
}

impl Sysfs {
    fn new() -> Self {
        Self {
            scratch: tempfile::tempdir().expect("a scratch folder"),
            machines: AtomicUsize::new(0),
        }
    }

    /// A fresh class folder holding `entries`, each a name and its
    /// attributes.
    fn machine(&self, entries: &[(&str, &[(&str, &str)])]) -> PathBuf {
        let number = self.machines.fetch_add(1, Ordering::Relaxed);
        let root = self.scratch.path().join(number.to_string());
        std::fs::create_dir(&root).expect("a class folder");
        for (name, attributes) in entries {
            let entry = root.join(name);
            std::fs::create_dir(&entry).expect("an entry");
            for (attribute, value) in *attributes {
                std::fs::write(entry.join(attribute), format!("{value}\n")).expect("an attribute");
            }
        }
        root
    }

    /// A path to a file where a folder should be.
    fn not_a_folder(&self) -> PathBuf {
        let number = self.machines.fetch_add(1, Ordering::Relaxed);
        let file = self.scratch.path().join(number.to_string());
        std::fs::write(&file, "").expect("a file");
        file
    }
}

const MAINS_ONLINE: (&str, &[(&str, &str)]) = ("AC", &[("type", "Mains"), ("online", "1")]);
const MAINS_OFFLINE: (&str, &[(&str, &str)]) = ("AC", &[("type", "Mains"), ("online", "0")]);
const DISCHARGING: (&str, &[(&str, &str)]) = (
    "BAT0",
    &[
        ("type", "Battery"),
        ("status", "Discharging"),
        ("scope", "System"),
    ],
);
const CHARGING: (&str, &[(&str, &str)]) = ("BAT0", &[("type", "Battery"), ("status", "Charging")]);

struct SysfsPowerHarness(Sysfs);

impl SysfsPowerHarness {
    fn power(&self, entries: &[(&str, &[(&str, &str)])]) -> Arc<dyn PowerSupply> {
        Arc::new(SysfsPower::at(self.0.machine(entries)))
    }
}

impl PowerHarness for SysfsPowerHarness {
    fn on_ac(&self) -> Arc<dyn PowerSupply> {
        self.power(&[MAINS_ONLINE, CHARGING])
    }

    fn on_battery(&self) -> Arc<dyn PowerSupply> {
        self.power(&[MAINS_OFFLINE, DISCHARGING])
    }

    fn without_battery(&self) -> Arc<dyn PowerSupply> {
        self.power(&[])
    }

    fn battery_not_discharging(&self) -> Arc<dyn PowerSupply> {
        self.power(&[("BAT0", &[("type", "Battery"), ("status", "Full")])])
    }

    fn unrecognized(&self) -> Arc<dyn PowerSupply> {
        self.power(&[
            ("AC", &[("type", "Mains"), ("online", "maybe")]),
            DISCHARGING,
        ])
    }

    fn unreadable(&self) -> Arc<dyn PowerSupply> {
        Arc::new(SysfsPower::at(self.0.not_a_folder()))
    }
}

#[tokio::test]
async fn the_kernels_power_supplies_meet_the_power_contract() {
    contract::power::run(&SysfsPowerHarness(Sysfs::new())).await;
}

async fn sysfs_power(entries: &[(&str, &[(&str, &str)])]) -> Result<PowerSource, PlatformError> {
    SysfsPowerHarness(Sysfs::new())
        .power(entries)
        .source()
        .await
}

#[tokio::test]
async fn a_peripherals_battery_running_down_is_not_the_machines() {
    let mouse = (
        "hidpp_battery_0",
        &[
            ("type", "Battery"),
            ("status", "Discharging"),
            ("scope", "Device"),
        ][..],
    );
    assert_eq!(sysfs_power(&[mouse]).await, Ok(PowerSource::Ac));
    assert_eq!(
        sysfs_power(&[mouse, DISCHARGING]).await,
        Ok(PowerSource::Battery)
    );
}

#[tokio::test]
async fn any_supply_online_is_ac_power_though_others_are_not() {
    let port = (
        "ucsi-source-psy-USBC000:001",
        &[("type", "USB"), ("online", "0")][..],
    );
    let adapter = ("ADP1", &[("type", "Mains"), ("online", "1")][..]);
    assert_eq!(
        sysfs_power(&[port, adapter, DISCHARGING]).await,
        Ok(PowerSource::Ac),
        "a battery some firmware says runs down while a charger is online"
    );
    assert_eq!(
        sysfs_power(&[("usb", &[("type", "USB_PD"), ("online", "2")])]).await,
        Ok(PowerSource::Ac),
        "a USB supply's online counts every value but 0"
    );
}

#[tokio::test]
async fn a_supply_offline_with_no_battery_running_down_is_not_battery() {
    let port = (
        "ucsi-source-psy-USBC000:001",
        &[("type", "USB"), ("online", "0")][..],
    );
    assert_eq!(
        sysfs_power(&[port]).await,
        Ok(PowerSource::Ac),
        "a desktop with an idle USB-C port"
    );
    assert_eq!(
        sysfs_power(&[port, DISCHARGING]).await,
        Ok(PowerSource::Battery)
    );
}

#[tokio::test]
async fn a_ups_running_down_is_battery_power() {
    let ups = ("ups", &[("type", "UPS"), ("status", "Discharging")][..]);
    assert_eq!(sysfs_power(&[ups]).await, Ok(PowerSource::Battery));
}

#[tokio::test]
async fn an_entry_without_a_type_or_the_attribute_read_is_passed_over() {
    let nameless = ("odd", &[("online", "1")][..]);
    let silent = ("AC", &[("type", "Mains")][..]);
    let unsure = ("BAT1", &[("type", "Battery")][..]);
    assert_eq!(
        sysfs_power(&[nameless, silent, unsure, DISCHARGING]).await,
        Ok(PowerSource::Battery)
    );
}

#[tokio::test]
async fn a_machine_with_no_power_supply_folder_is_on_ac_power() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let power = SysfsPower::at(scratch.path().join("power_supply"));
    assert_eq!(power.source().await, Ok(PowerSource::Ac));
}

#[tokio::test]
async fn a_battery_status_this_adapter_does_not_know_is_unrecognized() {
    let odd = ("BAT0", &[("type", "Battery"), ("status", "Resting")][..]);
    assert!(matches!(
        sysfs_power(&[odd]).await,
        Err(PlatformError::Unrecognized { .. })
    ));
}

#[tokio::test]
async fn an_attribute_too_long_or_not_text_is_unrecognized() {
    let sysfs = Sysfs::new();
    let long = sysfs.machine(&[("BAT0", &[("type", "Battery")])]);
    std::fs::write(long.join("BAT0/status"), "Discharging".repeat(1_000_000))
        .expect("a long attribute");
    let answer = SysfsPower::at(long).source().await;
    assert!(
        matches!(&answer, Err(PlatformError::Unrecognized { detail, .. }) if detail.len() < 200),
        "{answer:?}"
    );

    let binary = sysfs.machine(&[("BAT0", &[("type", "Battery")])]);
    std::fs::write(binary.join("BAT0/status"), [0xff, 0xfe, b'\n']).expect("an attribute");
    let answer = SysfsPower::at(binary).source().await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "{answer:?}"
    );
}

#[tokio::test]
async fn a_folder_of_too_many_entries_is_unrecognized() {
    let sysfs = Sysfs::new();
    let root = sysfs.machine(&[]);
    for n in 0..=1024 {
        std::fs::create_dir(root.join(format!("BAT{n}"))).expect("an entry");
    }
    let answer = SysfsPower::at(&root).source().await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "{answer:?}"
    );
    let answer = SysfsNetwork::at(&root).is_online().await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "{answer:?}"
    );
}

/// Makes `path` a named pipe: opening it for reading waits for a writer, as
/// a stalled read would.
fn fifo(path: &Path) {
    let mut mkfifo = std::process::Command::new("mkfifo");
    mkfifo.arg(path);
    let made = spawn_gate::hold(|| mkfifo.spawn())
        .and_then(|mut child| child.wait())
        .is_ok_and(|status| status.success());
    assert!(made, "mkfifo makes {}", path.display());
}

/// Lets the read given up on `path` go, so the runtime, which waits for its
/// blocking tasks, can end. Opening the pipe for writing waits for its
/// reader, so this returns once the reader has come to the pipe, and the
/// scratch folder may go after that, never before: a writer that came to
/// the pipe after its folder went could never reach the reader. Only a read
/// given up is still running (the adapters answer anything else once the
/// read has ended), so a read that answered holds no pipe to let go.
fn release<T>(path: &Path, answer: &Result<T, PlatformError>) {
    if !matches!(answer, Err(PlatformError::TimedOut { .. })) {
        return;
    }
    let mut pipe = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("the read given up opens the pipe");
    std::io::Write::write_all(&mut pipe, b"Mains\n").expect("a line for the reader");
}

#[tokio::test]
async fn a_read_that_stalls_is_given_up_at_the_bound() {
    let sysfs = Sysfs::new();
    let root = sysfs.machine(&[("AC", &[])]);
    fifo(&root.join("AC/type"));
    let started = Instant::now();
    let answer = SysfsPower::at(&root).with_timeout(SHORT).source().await;
    let took = started.elapsed();
    // Before the asserts: a test that panicked with the reader stalled
    // would wait for it on unwinding, and never end.
    release(&root.join("AC/type"), &answer);
    assert!(
        matches!(answer, Err(PlatformError::TimedOut { .. })),
        "{answer:?}"
    );
    assert!(took < SHORT * 10, "gave up after {took:?}");

    let root = sysfs.machine(&[("eth0", &[])]);
    fifo(&root.join("eth0/flags"));
    let answer = SysfsNetwork::at(&root)
        .with_timeout(SHORT)
        .is_online()
        .await;
    release(&root.join("eth0/flags"), &answer);
    assert!(
        matches!(answer, Err(PlatformError::TimedOut { .. })),
        "{answer:?}"
    );
}

/// Network interfaces as the kernel lists them. `flags` holds what the
/// interface was set to (up 0x1, loopback 0x8), never whether it runs:
/// `operstate` says that, and `carrier` where the state is `unknown`. These
/// are a working wired port's and loopback's own values.
const ETH_UP: (&str, &[(&str, &str)]) = (
    "eth0",
    &[("flags", "0x1003"), ("operstate", "up"), ("carrier", "1")],
);
const ETH_DOWN: (&str, &[(&str, &str)]) = ("eth0", &[("flags", "0x1002"), ("operstate", "down")]);
const LOOPBACK: (&str, &[(&str, &str)]) = (
    "lo",
    &[("flags", "0x9"), ("operstate", "unknown"), ("carrier", "1")],
);

struct SysfsNetworkHarness(Sysfs);

impl SysfsNetworkHarness {
    fn network(&self, entries: &[(&str, &[(&str, &str)])]) -> Arc<dyn Network> {
        Arc::new(SysfsNetwork::at(self.0.machine(entries)))
    }
}

impl NetworkHarness for SysfsNetworkHarness {
    fn online(&self) -> Arc<dyn Network> {
        self.network(&[LOOPBACK, ETH_UP])
    }

    fn offline(&self) -> Arc<dyn Network> {
        self.network(&[LOOPBACK, ETH_DOWN])
    }

    fn only_loopback(&self) -> Arc<dyn Network> {
        self.network(&[LOOPBACK])
    }

    fn unrecognized(&self) -> Arc<dyn Network> {
        self.network(&[LOOPBACK, ("eth0", &[("flags", "up")])])
    }

    fn unreadable(&self) -> Arc<dyn Network> {
        Arc::new(SysfsNetwork::at(self.0.not_a_folder()))
    }
}

#[tokio::test]
async fn the_kernels_network_interfaces_meet_the_network_contract() {
    contract::network::run(&SysfsNetworkHarness(Sysfs::new())).await;
}

async fn sysfs_online(entries: &[(&str, &[(&str, &str)])]) -> Result<bool, PlatformError> {
    SysfsNetworkHarness(Sysfs::new())
        .network(entries)
        .is_online()
        .await
}

#[tokio::test]
async fn an_interface_up_without_a_working_link_is_offline() {
    for (state, carrier) in [("down", "0"), ("dormant", "1"), ("lowerlayerdown", "0")] {
        let wifi = (
            "wlan0",
            &[
                ("flags", "0x1003"),
                ("operstate", state),
                ("carrier", carrier),
            ][..],
        );
        assert_eq!(sysfs_online(&[LOOPBACK, wifi]).await, Ok(false), "{state}");
    }
}

#[tokio::test]
async fn an_interface_switched_off_is_offline_whatever_else_it_says() {
    let off = (
        "eth0",
        &[("flags", "0x1002"), ("operstate", "up"), ("carrier", "1")][..],
    );
    assert_eq!(sysfs_online(&[LOOPBACK, off]).await, Ok(false));
}

#[tokio::test]
async fn any_interface_up_with_a_working_link_is_online() {
    let wifi = (
        "wlan0",
        &[("flags", "0x1003"), ("operstate", "up"), ("carrier", "1")][..],
    );
    assert_eq!(sysfs_online(&[LOOPBACK, ETH_DOWN, wifi]).await, Ok(true));
}

/// A tunnel or a point-to-point link reports its state as `unknown`: its
/// carrier says whether it works.
#[tokio::test]
async fn an_interface_of_unknown_state_is_online_with_a_carrier() {
    let ppp = |carrier| {
        (
            "ppp0",
            [
                ("flags", "0x10d1"),
                ("operstate", "unknown"),
                ("carrier", carrier),
            ],
        )
    };
    let (name, with) = ppp("1");
    assert_eq!(sysfs_online(&[LOOPBACK, (name, &with)]).await, Ok(true));
    let (name, without) = ppp("0");
    assert_eq!(sysfs_online(&[LOOPBACK, (name, &without)]).await, Ok(false));
    let silent = ("tun0", &[("flags", "0x1091"), ("operstate", "unknown")][..]);
    assert_eq!(sysfs_online(&[LOOPBACK, silent]).await, Ok(false));
}

#[tokio::test]
async fn an_interface_without_flags_or_state_is_passed_over() {
    let bare = ("eth1", &[][..]);
    assert_eq!(sysfs_online(&[bare, ETH_UP]).await, Ok(true));
    assert_eq!(sysfs_online(&[bare]).await, Ok(false));
    let stateless = ("eth1", &[("flags", "0x1003")][..]);
    assert_eq!(sysfs_online(&[LOOPBACK, stateless]).await, Ok(false));
}

#[tokio::test]
async fn a_state_or_carrier_the_kernel_does_not_write_is_unrecognized() {
    let sideways = (
        "eth0",
        &[("flags", "0x1003"), ("operstate", "sideways")][..],
    );
    let answer = sysfs_online(&[LOOPBACK, sideways]).await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "{answer:?}"
    );
    let odd_carrier = (
        "tun0",
        &[
            ("flags", "0x1091"),
            ("operstate", "unknown"),
            ("carrier", "2"),
        ][..],
    );
    let answer = sysfs_online(&[LOOPBACK, odd_carrier]).await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "{answer:?}"
    );
}

#[tokio::test]
async fn flags_too_large_for_the_kernels_are_unrecognized() {
    let huge = ("eth0", &[("flags", "0x1ffffffff")][..]);
    assert!(matches!(
        sysfs_online(&[huge]).await,
        Err(PlatformError::Unrecognized { .. })
    ));
}

#[tokio::test]
async fn a_machine_with_no_network_folder_is_unreadable() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let network = SysfsNetwork::at(scratch.path().join("net"));
    assert!(matches!(
        network.is_online().await,
        Err(PlatformError::Unreadable { .. })
    ));
}

/// A stand-in program: `sh` printing `text` exactly.
fn printing(text: &str) -> [String; 2] {
    ["-c".to_owned(), format!("cat <<'EOF'\n{text}\nEOF")]
}

const PMSET_AC: &str = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=4653155)\t95%; charging; 0:21 remaining present: true";
const PMSET_BATTERY: &str = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653155)\t80%; discharging; 4:02 remaining present: true";
const PMSET_DESKTOP: &str = "Now drawing from 'AC Power'";
const PMSET_FULL: &str = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=4653155)\t100%; charged; 0:00 remaining present: true";

struct PmsetHarness;

impl PmsetHarness {
    fn power(text: &str) -> Arc<dyn PowerSupply> {
        Arc::new(PmsetPower::running("/bin/sh", printing(text)))
    }
}

impl PowerHarness for PmsetHarness {
    fn on_ac(&self) -> Arc<dyn PowerSupply> {
        Self::power(PMSET_AC)
    }

    fn on_battery(&self) -> Arc<dyn PowerSupply> {
        Self::power(PMSET_BATTERY)
    }

    fn without_battery(&self) -> Arc<dyn PowerSupply> {
        Self::power(PMSET_DESKTOP)
    }

    fn battery_not_discharging(&self) -> Arc<dyn PowerSupply> {
        Self::power(PMSET_FULL)
    }

    fn unrecognized(&self) -> Arc<dyn PowerSupply> {
        Self::power("Now drawing from 'Solar Power'")
    }

    fn unreadable(&self) -> Arc<dyn PowerSupply> {
        Arc::new(PmsetPower::running(
            "/bin/sh",
            ["-c", "echo busy >&2; exit 1"],
        ))
    }
}

#[tokio::test]
async fn pmset_meets_the_power_contract() {
    contract::power::run(&PmsetHarness).await;
}

#[tokio::test]
async fn pmset_on_a_ups_is_battery_power() {
    let answer = PmsetHarness::power("Now drawing from 'UPS Power'")
        .source()
        .await;
    assert_eq!(answer, Ok(PowerSource::Battery));
}

#[tokio::test]
async fn pmset_saying_nothing_of_its_source_is_unrecognized() {
    for text in ["", "Now drawing from 'AC Power", "drawing from 'AC Power'"] {
        let answer = PmsetHarness::power(text).source().await;
        assert!(
            matches!(answer, Err(PlatformError::Unrecognized { .. })),
            "{text:?}: {answer:?}"
        );
    }
}

const SCUTIL_ONLINE: &str = "Network information

IPv4 network interface information
     en0 : flags      : 0x5 (IPv4,DNS)
           address    : 192.0.2.25
           reach      : 0x00000002 (Reachable)

   REACH : flags 0x00000002 (Reachable)

IPv6 network interface information
   No IPv6 states found

   REACH : flags 0x00000000 (Not Reachable)

Network interfaces: en0
";

const SCUTIL_OFFLINE: &str = "Network information

IPv4 network interface information
   No IPv4 states found

   REACH : flags 0x00000000 (Not Reachable)

IPv6 network interface information
   No IPv6 states found

   REACH : flags 0x00000000 (Not Reachable)

Network interfaces:
";

struct ScutilHarness;

impl ScutilHarness {
    fn network(text: &str) -> Arc<dyn Network> {
        Arc::new(ScutilNetwork::running("/bin/sh", printing(text)))
    }
}

impl NetworkHarness for ScutilHarness {
    fn online(&self) -> Arc<dyn Network> {
        Self::network(SCUTIL_ONLINE)
    }

    fn offline(&self) -> Arc<dyn Network> {
        Self::network(SCUTIL_OFFLINE)
    }

    fn only_loopback(&self) -> Arc<dyn Network> {
        // scutil never lists the loopback interface.
        Self::network(SCUTIL_OFFLINE)
    }

    fn unrecognized(&self) -> Arc<dyn Network> {
        Self::network("Network information\n\n   REACH : flags often\n")
    }

    fn unreadable(&self) -> Arc<dyn Network> {
        Arc::new(ScutilNetwork::running(
            "/nonexistent/ladderback-demo/scutil",
            ["--nwi"],
        ))
    }
}

#[tokio::test]
async fn scutil_meets_the_network_contract() {
    contract::network::run(&ScutilHarness).await;
}

#[tokio::test]
async fn scutil_reachable_only_through_a_connection_still_to_make_is_offline() {
    let text = SCUTIL_OFFLINE.replacen("0x00000000", "0x00000006", 1);
    assert_eq!(ScutilHarness::network(&text).is_online().await, Ok(false));
    let text = SCUTIL_OFFLINE.replacen("0x00000000", "0x00000003", 1);
    assert_eq!(ScutilHarness::network(&text).is_online().await, Ok(true));
}

#[tokio::test]
async fn scutil_with_no_reachability_is_unrecognized() {
    let answer = ScutilHarness::network("Network information\n")
        .is_online()
        .await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "{answer:?}"
    );
}

/// The program sees none of the daemon's environment: here it answers
/// online only when `HOME` is unset.
#[tokio::test]
async fn a_program_runs_with_no_environment() {
    let script = format!(
        "if [ -z \"${{HOME+set}}\" ]; then cat <<'EOF'\n{SCUTIL_ONLINE}\nEOF\nelse cat <<'EOF'\n{SCUTIL_OFFLINE}\nEOF\nfi"
    );
    let network = ScutilNetwork::running("/bin/sh", ["-c", script.as_str()]);
    assert_eq!(network.is_online().await, Ok(true));
}

#[tokio::test]
async fn a_program_that_hangs_is_stopped_at_the_bound() {
    let started = Instant::now();
    let power = PmsetPower::running("/bin/sh", ["-c", "sleep 30"]).with_timeout(SHORT);
    let answer = power.source().await;
    assert!(
        matches!(answer, Err(PlatformError::TimedOut { .. })),
        "{answer:?}"
    );
    let network = ScutilNetwork::running("/bin/sh", ["-c", "sleep 30"]).with_timeout(SHORT);
    let answer = network.is_online().await;
    assert!(
        matches!(answer, Err(PlatformError::TimedOut { .. })),
        "{answer:?}"
    );
    assert!(started.elapsed() < SHORT * 10);
}

#[tokio::test]
async fn a_program_that_prints_without_end_is_unrecognized() {
    let started = Instant::now();
    let power = PmsetPower::running("/bin/sh", ["-c", "yes 'Now drawing from'"]);
    let answer = power.source().await;
    assert!(
        matches!(&answer, Err(PlatformError::Unrecognized { detail, .. }) if detail.len() < 200),
        "{answer:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "given up once the output passed its cap, not at the timeout"
    );
}

/// The bound covers the whole call, the wait for the spawn gate too: here
/// another start holds the gate for longer than the bound (a start whose
/// `exec` stalls does, decision 58).
#[tokio::test]
async fn a_program_that_cannot_start_in_time_is_given_up_at_the_bound() {
    let (held, gate_held) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        spawn_gate::hold(|| {
            let _ = held.send(());
            std::thread::sleep(SHORT * 4);
        });
    });
    gate_held.recv().expect("the gate is held");
    let started = Instant::now();
    let power = PmsetPower::running("/bin/sh", printing(PMSET_AC)).with_timeout(SHORT);
    let answer = power.source().await;
    let took = started.elapsed();
    holder.join().expect("the holder lets go");
    assert!(
        matches!(answer, Err(PlatformError::TimedOut { .. })),
        "{answer:?}"
    );
    assert!(took < SHORT * 3, "gave up after {took:?}");
}

/// What a source said goes into the error for the agent's log, so a
/// control character in it (a terminal escape) is written out, not kept.
#[tokio::test]
async fn what_a_source_said_is_repeated_without_its_control_characters() {
    let power = PmsetPower::running("/bin/sh", printing("Now drawing from '\u{1b}[2JSolar'"));
    let answer = power.source().await;
    assert!(
        matches!(
            &answer,
            Err(PlatformError::Unrecognized { detail, .. })
                if !detail.chars().any(char::is_control) && detail.contains("[2JSolar")
        ),
        "{answer:?}"
    );
}

/// The machine's own sources answer; what they say is the machine's.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_kernels_own_sources_answer() {
    let power = SysfsPower::system().source().await;
    assert!(power.is_ok(), "{power:?}");
    let network = SysfsNetwork::system().is_online().await;
    assert!(network.is_ok(), "{network:?}");
}

/// The machine's own sources answer; what they say is the machine's.
#[cfg(target_os = "macos")]
#[tokio::test]
async fn pmset_and_scutil_answer() {
    let power = PmsetPower::system().source().await;
    assert!(power.is_ok(), "{power:?}");
    let network = ScutilNetwork::system().is_online().await;
    assert!(network.is_ok(), "{network:?}");
}

#[tokio::test]
async fn the_system_sources_are_this_platforms() {
    let power = platform::power::system_power().source().await;
    assert!(power.is_ok(), "{power:?}");
}

#[tokio::test]
async fn the_system_network_is_this_platforms() {
    let network = platform::network::system_network().is_online().await;
    assert!(network.is_ok(), "{network:?}");
}

/// The kernel's routing tables and `/sys/class/net`, made up, one machine
/// per call: each interface a folder under `devices/`, linked from `net/` as
/// the kernel links it, a virtual one under `devices/virtual/net/`.
struct KernelLinkHarness {
    scratch: tempfile::TempDir,
    machines: AtomicUsize,
    /// The NetworkManager the machines' names are read from.
    #[cfg(target_os = "linux")]
    network_manager: Option<stand_in::NetworkManager>,
}

/// One made-up interface: its name, whether it is virtual, its attributes,
/// and the folders in it (`phy80211`, `wireless`).
type Interface<'a> = (&'a str, bool, &'a [(&'a str, &'a str)], &'a [&'a str]);

const ROUTE_HEADER: &str =
    "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT";

/// An IPv4 default route out of `interface` at `metric`, with a route to its
/// own subnet beside it.
fn default_route(interface: &str, metric: u32) -> String {
    format!(
        "{ROUTE_HEADER}\n{interface}\t0001A8C0\t00000000\t0001\t0\t0\t{metric}\t00FFFFFF\t0\t0\t0\n\
         {interface}\t00000000\t0101A8C0\t0003\t0\t0\t{metric}\t00000000\t0\t0\t0\n"
    )
}

/// IPv6's unreachable default, which every Linux machine lists, out of `lo`.
const IPV6_UNREACHABLE: &str = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200       lo\n";

const ETH0: Interface<'static> = ("eth0", false, &[("type", "1")], &[]);
const WLAN0: Interface<'static> = ("wlp2s0", false, &[("type", "1")], &["phy80211"]);
const TUN0: Interface<'static> = ("tun0", true, &[("type", "65534")], &[]);
const BRIDGE: Interface<'static> = ("br0", true, &[("type", "1")], &["bridge"]);

impl KernelLinkHarness {
    fn new() -> Self {
        Self {
            scratch: tempfile::tempdir().expect("a scratch folder"),
            machines: AtomicUsize::new(0),
            #[cfg(target_os = "linux")]
            network_manager: None,
        }
    }

    /// A harness whose machines' names a stand-in NetworkManager gives, on
    /// a bus of its own; `None` where no `dbus-daemon` can run one.
    #[cfg(target_os = "linux")]
    fn with_network_manager() -> Option<Self> {
        let network_manager = stand_in::NetworkManager::start()?;
        Some(Self {
            network_manager: Some(network_manager),
            ..Self::new()
        })
    }

    /// A machine whose default route leaves by a Wi-Fi interface of its
    /// own, which the stand-in NetworkManager says is `device`; `None` makes
    /// no device of it.
    #[cfg(target_os = "linux")]
    fn on_wifi(&self, device: Option<stand_in::Device>) -> Arc<dyn NetworkLink> {
        let interface = format!("wl{}", self.machines.load(Ordering::Relaxed));
        let network_manager = self
            .network_manager
            .as_ref()
            .expect("a stand-in NetworkManager");
        if let Some(device) = device {
            network_manager.add(&interface, device);
        }
        self.machine(
            &default_route(&interface, 600),
            Some(IPV6_UNREACHABLE),
            &[(interface.as_str(), false, &[("type", "1")], &["phy80211"])],
        )
    }

    fn machine(
        &self,
        route: &str,
        ipv6_route: Option<&str>,
        interfaces: &[Interface<'_>],
    ) -> Arc<dyn NetworkLink> {
        let link = self.kernel_link(route, ipv6_route, interfaces);
        #[cfg(target_os = "linux")]
        let link = match &self.network_manager {
            Some(network_manager) => link.with_bus(network_manager.address()),
            None => link,
        };
        Arc::new(link)
    }

    /// The reader of a made-up machine, asking the system bus for names.
    fn kernel_link(
        &self,
        route: &str,
        ipv6_route: Option<&str>,
        interfaces: &[Interface<'_>],
    ) -> KernelLink {
        let number = self.machines.fetch_add(1, Ordering::Relaxed);
        let root = self.scratch.path().join(number.to_string());
        let net = root.join("net");
        std::fs::create_dir_all(&net).expect("a class folder");
        for (name, is_virtual, attributes, folders) in interfaces {
            let device = if *is_virtual {
                root.join("devices/virtual/net").join(name)
            } else {
                root.join("devices/pci0000:00/0000:00:1f.6/net").join(name)
            };
            std::fs::create_dir_all(&device).expect("an interface");
            for (attribute, value) in *attributes {
                std::fs::write(device.join(attribute), format!("{value}\n")).expect("an attribute");
            }
            for folder in *folders {
                std::fs::create_dir(device.join(folder)).expect("a folder");
            }
            let relative = device.strip_prefix(&root).expect("under the machine");
            std::os::unix::fs::symlink(Path::new("..").join(relative), net.join(name))
                .expect("a link");
        }
        std::fs::write(root.join("route"), route).expect("the IPv4 routes");
        if let Some(text) = ipv6_route {
            std::fs::write(root.join("ipv6_route"), text).expect("the IPv6 routes");
        }
        KernelLink::at(root.join("route"), root.join("ipv6_route"), net)
    }
}

impl NetworkLinkHarness for KernelLinkHarness {
    fn wired(&self) -> Arc<dyn NetworkLink> {
        self.machine(
            &default_route("eth0", 100),
            Some(IPV6_UNREACHABLE),
            &[ETH0, WLAN0],
        )
    }

    fn wifi(&self) -> Arc<dyn NetworkLink> {
        self.machine(
            &default_route("wlp2s0", 600),
            Some(IPV6_UNREACHABLE),
            &[ETH0, WLAN0],
        )
    }

    fn tunnel(&self) -> Arc<dyn NetworkLink> {
        self.machine(&default_route("tun0", 50), None, &[ETH0, TUN0])
    }

    fn no_route(&self) -> Arc<dyn NetworkLink> {
        self.machine(
            &format!("{ROUTE_HEADER}\n"),
            Some(IPV6_UNREACHABLE),
            &[ETH0],
        )
    }

    fn unrecognized(&self) -> Arc<dyn NetworkLink> {
        self.machine(&format!("{ROUTE_HEADER}\neth0\tdefault\n"), None, &[ETH0])
    }

    fn unreadable(&self) -> Arc<dyn NetworkLink> {
        let root = self.scratch.path().join("unreadable");
        std::fs::create_dir_all(root.join("route")).expect("a folder where the routes should be");
        Arc::new(KernelLink::at(
            root.join("route"),
            root.join("ipv6_route"),
            root.join("net"),
        ))
    }

    #[cfg(target_os = "linux")]
    fn on_network(&self, name: &str) -> Arc<dyn NetworkLink> {
        self.on_wifi(Some(stand_in::Device::wifi(stand_in::Ssid::Bytes(
            name.as_bytes().to_vec(),
        ))))
    }

    #[cfg(target_os = "linux")]
    fn on_no_network(&self) -> Arc<dyn NetworkLink> {
        self.on_wifi(Some(stand_in::Device::wifi(stand_in::Ssid::None)))
    }

    /// NetworkManager hides nothing, but a hidden network's access point
    /// may have no name yet: an empty one.
    #[cfg(target_os = "linux")]
    fn name_hidden(&self) -> Arc<dyn NetworkLink> {
        self.on_wifi(Some(stand_in::Device::wifi(stand_in::Ssid::Bytes(
            Vec::new(),
        ))))
    }

    /// A bus with no NetworkManager on it.
    #[cfg(target_os = "linux")]
    fn no_name_reader(&self) -> Arc<dyn NetworkLink> {
        let link = self.kernel_link(
            &default_route("wlp2s0", 600),
            Some(IPV6_UNREACHABLE),
            &[WLAN0],
        );
        let bus = self.scratch.path().join("no-bus");
        Arc::new(link.with_bus(format!("unix:path={}", bus.display())))
    }

    /// The access point's name as text, not bytes.
    #[cfg(target_os = "linux")]
    fn name_unrecognized(&self, name: &str) -> Arc<dyn NetworkLink> {
        self.on_wifi(Some(stand_in::Device::wifi(stand_in::Ssid::Text(
            name.to_owned(),
        ))))
    }

    #[cfg(not(target_os = "linux"))]
    fn on_network(&self, _name: &str) -> Arc<dyn NetworkLink> {
        unreachable!("the contract runs on Linux only")
    }

    #[cfg(not(target_os = "linux"))]
    fn on_no_network(&self) -> Arc<dyn NetworkLink> {
        unreachable!("the contract runs on Linux only")
    }

    #[cfg(not(target_os = "linux"))]
    fn name_hidden(&self) -> Arc<dyn NetworkLink> {
        unreachable!("the contract runs on Linux only")
    }

    #[cfg(not(target_os = "linux"))]
    fn no_name_reader(&self) -> Arc<dyn NetworkLink> {
        unreachable!("the contract runs on Linux only")
    }

    #[cfg(not(target_os = "linux"))]
    fn name_unrecognized(&self, _name: &str) -> Arc<dyn NetworkLink> {
        unreachable!("the contract runs on Linux only")
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_kernels_routes_and_interfaces_meet_the_network_link_contract() {
    let Some(harness) = KernelLinkHarness::with_network_manager() else {
        testkit::skip!("no dbus-daemon here to run a stand-in NetworkManager on");
    };
    contract::network_link::run(&harness).await;
}

#[cfg(not(target_os = "linux"))]
#[tokio::test]
async fn the_kernels_routes_and_interfaces_meet_the_network_link_contract() {
    testkit::skip!(
        "the kernel's reader asks NetworkManager for the network's name, which is read only on Linux"
    );
}

/// The name is read of the interface the default route leaves by: one that
/// isn't Wi-Fi has none, and a Wi-Fi interface NetworkManager doesn't know
/// as Wi-Fi is no answer.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_name_is_the_wi_fi_network_the_default_route_leaves_by() {
    let Some(harness) = KernelLinkHarness::with_network_manager() else {
        testkit::skip!("no dbus-daemon here to run a stand-in NetworkManager on");
    };
    assert_eq!(harness.wired().network_name().await, Ok(None));
    let mut device = stand_in::Device::wifi(stand_in::Ssid::Bytes(b"Office".to_vec()));
    device.kind = 1;
    for link in [harness.on_wifi(Some(device)), harness.on_wifi(None)] {
        let answer = link.network_name().await;
        assert!(
            matches!(answer, Err(PlatformError::Unrecognized { .. })),
            "{answer:?}"
        );
    }
    let answer = harness.no_route().network_name().await;
    assert!(
        matches!(answer, Err(PlatformError::Unreadable { .. })),
        "a route gone since the link was read is no answer, not \"no network\": {answer:?}"
    );
}

/// The kernel says what the link is before NetworkManager is asked: a
/// machine off Wi-Fi has no network though it has no NetworkManager, as on
/// macOS, and only a Wi-Fi one finds no answer.
#[tokio::test]
async fn a_machine_off_wi_fi_needs_no_networkmanager_to_have_no_network() {
    let harness = KernelLinkHarness::new();
    let bus = format!(
        "unix:path={}",
        harness.scratch.path().join("no-bus").display()
    );
    let wired = harness
        .kernel_link(&default_route("eth0", 100), None, &[ETH0])
        .with_bus(bus.clone());
    assert_eq!(wired.network_name().await, Ok(None));
    let wifi = harness
        .kernel_link(&default_route("wlp2s0", 600), None, &[WLAN0])
        .with_bus(bus);
    let answer = wifi.network_name().await;
    assert!(
        matches!(answer, Err(PlatformError::Unreadable { .. })),
        "{answer:?}"
    );
}

/// A name read given up names what it was waiting on: NetworkManager when
/// the bus doesn't answer, not the routing table the read began at, and the
/// routing table when that is what stalls.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_name_read_given_up_names_what_stalled() {
    let harness = KernelLinkHarness::new();
    // A bus that takes the connection and never answers it.
    let socket = harness.scratch.path().join("silent-bus");
    let silent = std::os::unix::net::UnixListener::bind(&socket).expect("a socket");
    let bus = format!("unix:path={}", socket.display());
    let answer = harness
        .kernel_link(&default_route("wlp2s0", 600), None, &[WLAN0])
        .with_bus(bus.clone())
        .with_timeout(SHORT)
        .network_name()
        .await;
    // Before the asserts: closing the socket ends the call given up, which
    // the runtime waits for when the test ends.
    drop(silent);
    assert_eq!(
        answer,
        Err(PlatformError::TimedOut {
            from: format!("NetworkManager on {bus}")
        })
    );

    let machine = harness.machines.load(Ordering::Relaxed);
    let link = harness
        .kernel_link(&default_route("wlp2s0", 600), None, &[WLAN0])
        .with_bus(bus)
        .with_timeout(SHORT);
    let route = harness
        .scratch
        .path()
        .join(machine.to_string())
        .join("route");
    std::fs::remove_file(&route).expect("the routes written");
    fifo(&route);
    let answer = link.network_name().await;
    release(&route, &answer);
    assert_eq!(
        answer,
        Err(PlatformError::TimedOut {
            from: route.display().to_string()
        })
    );
}

/// NetworkManager on a bus of its own: a `dbus-daemon` with a configuration
/// of the test's, and a thread that answers `Properties.Get` for the devices
/// the test adds, as NetworkManager's D-Bus API does.
#[cfg(target_os = "linux")]
mod stand_in {
    use std::io::BufRead;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use dbus::arg::Variant;
    use dbus::blocking::Connection;
    use dbus::channel::{Channel, MatchingReceiver, Sender};
    use dbus::message::MatchRule;
    use dbus::{Message, Path};

    const SERVICE: &str = "org.freedesktop.NetworkManager";
    const DEVICES: &str = "/org/freedesktop/NetworkManager/Devices/";
    const ACCESS_POINTS: &str = "/org/freedesktop/NetworkManager/AccessPoint/";

    /// What the access point says its name is.
    #[derive(Clone)]
    pub enum Ssid {
        /// No access point: the device has joined no network.
        None,
        Bytes(Vec<u8>),
        /// Text where NetworkManager gives bytes.
        Text(String),
    }

    #[derive(Clone)]
    pub struct Device {
        pub interface: String,
        pub kind: u32,
        pub ssid: Ssid,
    }

    impl Device {
        pub fn wifi(ssid: Ssid) -> Self {
            Self {
                interface: String::new(),
                kind: 2,
                ssid,
            }
        }
    }

    pub struct NetworkManager {
        _scratch: tempfile::TempDir,
        daemon: std::process::Child,
        address: String,
        devices: Arc<Mutex<Vec<Device>>>,
        stop: Arc<AtomicBool>,
        answering: Option<std::thread::JoinHandle<()>>,
    }

    impl NetworkManager {
        /// Starts the bus and the stand-in on it; `None` when there is no
        /// `dbus-daemon` to start.
        pub fn start() -> Option<Self> {
            let scratch = tempfile::tempdir().expect("a scratch folder");
            let config = scratch.path().join("bus.conf");
            std::fs::write(
                &config,
                format!(
                    "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                     <busconfig><type>session</type><listen>unix:path={}</listen><auth>EXTERNAL</auth>\
                     <policy context=\"default\"><allow send_destination=\"*\"/><allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
                    scratch.path().join("bus").display()
                ),
            )
            .expect("the bus's configuration");
            let mut daemon = std::process::Command::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--print-address"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .ok()?;
            let mut address = String::new();
            std::io::BufReader::new(daemon.stdout.take().expect("piped"))
                .read_line(&mut address)
                .expect("the bus's address");
            let address = address.trim().to_owned();
            let devices = Arc::new(Mutex::new(Vec::<Device>::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (ready, owned) = std::sync::mpsc::channel();
            let answering = {
                let (address, devices, stop) = (address.clone(), devices.clone(), stop.clone());
                std::thread::spawn(move || {
                    let mut channel = Channel::open_private(&address).expect("the stand-in's bus");
                    channel.register().expect("a name on the bus");
                    let connection = Connection::from(channel);
                    connection
                        .request_name(SERVICE, false, true, true)
                        .expect("NetworkManager's name");
                    connection.start_receive(
                        MatchRule::new_method_call(),
                        Box::new(move |call, connection| {
                            let devices = devices.lock().unwrap_or_else(PoisonError::into_inner);
                            let _ = connection.send(answer(&call, &devices));
                            true
                        }),
                    );
                    let _ = ready.send(());
                    while !stop.load(Ordering::Relaxed) {
                        connection
                            .process(Duration::from_millis(20))
                            .expect("the stand-in's bus");
                    }
                })
            };
            owned.recv().expect("the stand-in owns its name");
            Some(Self {
                _scratch: scratch,
                daemon,
                address,
                devices,
                stop,
                answering: Some(answering),
            })
        }

        pub fn address(&self) -> String {
            self.address.clone()
        }

        /// A device for `interface`.
        pub fn add(&self, interface: &str, device: Device) {
            self.devices
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Device {
                    interface: interface.to_owned(),
                    ..device
                });
        }
    }

    impl Drop for NetworkManager {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(answering) = self.answering.take() {
                let _ = answering.join();
            }
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    /// NetworkManager's answer to `call`: a property of the manager, of a
    /// device or of an access point, read through `Properties.Get`.
    fn answer(call: &Message, devices: &[Device]) -> Message {
        let unknown = || {
            call.error(
                &"org.freedesktop.DBus.Error.UnknownProperty".into(),
                c"no such property",
            )
        };
        let (Some(path), Some(member), Ok((interface, property))) =
            (call.path(), call.member(), call.read2::<&str, &str>())
        else {
            return unknown();
        };
        if &*member != "Get" {
            return unknown();
        }
        let reply = call.method_return();
        let path = &*path;
        if path == "/org/freedesktop/NetworkManager" && property == "Devices" {
            let paths: Vec<Path<'static>> = (0..devices.len())
                .map(|index| Path::from(format!("{DEVICES}{index}")))
                .collect();
            return reply.append1(Variant(paths));
        }
        if let Some(index) = path.strip_prefix(DEVICES) {
            let Some(device) = index
                .parse::<usize>()
                .ok()
                .and_then(|index| devices.get(index))
            else {
                return unknown();
            };
            return match (interface, property) {
                (_, "Interface") => reply.append1(Variant(device.interface.clone())),
                (_, "DeviceType") => reply.append1(Variant(device.kind)),
                (_, "ActiveAccessPoint") => reply.append1(Variant(match device.ssid {
                    Ssid::None => Path::from("/"),
                    _ => Path::from(format!("{ACCESS_POINTS}{index}")),
                })),
                _ => unknown(),
            };
        }
        if let Some(index) = path.strip_prefix(ACCESS_POINTS) {
            let Some(device) = index
                .parse::<usize>()
                .ok()
                .and_then(|index| devices.get(index))
            else {
                return unknown();
            };
            return match (&device.ssid, property) {
                (Ssid::Bytes(bytes), "Ssid") => reply.append1(Variant(bytes.clone())),
                (Ssid::Text(text), "Ssid") => reply.append1(Variant(text.clone())),
                _ => unknown(),
            };
        }
        unknown()
    }
}

#[tokio::test]
async fn the_kernel_with_no_default_route_says_no_route() {
    let harness = KernelLinkHarness::new();
    assert_eq!(harness.no_route().link().await, Ok(Link::NoRoute));
    // A machine with IPv6 switched off has no IPv6 table at all.
    let link = harness.machine(&format!("{ROUTE_HEADER}\n"), None, &[ETH0]);
    assert_eq!(link.link().await, Ok(Link::NoRoute));
}

#[tokio::test]
async fn the_default_route_of_lowest_metric_in_either_family_is_the_link() {
    let harness = KernelLinkHarness::new();
    let ipv6_wired = format!(
        "00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000064 00000001 00000000 00000003     eth0\n{IPV6_UNREACHABLE}"
    );
    let link = harness.machine(
        &default_route("wlp2s0", 600),
        Some(&ipv6_wired),
        &[ETH0, WLAN0],
    );
    assert_eq!(link.link().await, Ok(Link::Wired));
    let link = harness.machine(
        &default_route("wlp2s0", 20),
        Some(&ipv6_wired),
        &[ETH0, WLAN0],
    );
    assert_eq!(link.link().await, Ok(Link::WiFi));
}

#[tokio::test]
async fn a_default_route_that_is_down_is_no_route() {
    let harness = KernelLinkHarness::new();
    let down =
        format!("{ROUTE_HEADER}\neth0\t00000000\t0101A8C0\t0002\t0\t0\t100\t00000000\t0\t0\t0\n");
    assert_eq!(
        harness.machine(&down, None, &[ETH0]).link().await,
        Ok(Link::NoRoute)
    );
}

#[tokio::test]
async fn a_virtual_ethernet_interface_is_not_wired() {
    let harness = KernelLinkHarness::new();
    let link = harness.machine(&default_route("br0", 100), None, &[ETH0, BRIDGE]);
    assert_eq!(link.link().await, Ok(Link::Other));
}

#[tokio::test]
async fn a_wireless_extensions_interface_is_wi_fi() {
    let harness = KernelLinkHarness::new();
    let old = ("wlan0", false, &[("type", "1")][..], &["wireless"][..]);
    let link = harness.machine(&default_route("wlan0", 100), None, &[old]);
    assert_eq!(link.link().await, Ok(Link::WiFi));
}

#[tokio::test]
async fn an_interface_name_that_leaves_the_folder_is_unrecognized() {
    let harness = KernelLinkHarness::new();
    for name in ["..", "../eth0", "eth0/..", ""] {
        let route = format!(
            "{ROUTE_HEADER}\n{name}\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n"
        );
        let answer = harness.machine(&route, None, &[ETH0]).link().await;
        assert!(
            matches!(answer, Err(PlatformError::Unrecognized { .. })),
            "{name:?}: {answer:?}"
        );
    }
}

#[tokio::test]
async fn a_default_route_out_of_an_interface_not_listed_is_unrecognized() {
    let harness = KernelLinkHarness::new();
    let answer = harness
        .machine(&default_route("eth9", 100), None, &[ETH0])
        .link()
        .await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "{answer:?}"
    );
}

/// `route -n get default` on a Mac whose default route leaves by `interface`.
fn route_output(interface: &str) -> String {
    format!(
        "   route to: default\ndestination: default\n       mask: default\n    gateway: 192.168.1.1\n  interface: {interface}\n      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>\n"
    )
}

/// `networksetup -listallhardwareports` as a Mac on Ethernet printed it
/// (#452), cut to some of its ports, its addresses made up: `en10` is a USB
/// 2.5 GbE adapter, `en4` an Ethernet adapter with no cable, `en1` a
/// Thunderbolt port.
const HARDWARE_PORTS: &str = "\nHardware Port: Ethernet Adapter (en4)\nDevice: en4\nEthernet Address: 02:00:00:00:00:04\n\nHardware Port: USB 10/100/1G/2.5G LAN\nDevice: en10\nEthernet Address: 02:00:00:00:00:0a\n\nHardware Port: Thunderbolt Bridge\nDevice: bridge0\nEthernet Address: 02:00:00:00:00:05\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: 02:00:00:00:00:00\n\nHardware Port: Thunderbolt 1\nDevice: en1\nEthernet Address: 02:00:00:00:00:01\n\nVLAN Configurations\n===================\n";

/// A stand-in for `ifconfig <interface>`, which `sh` is given as `$1`: the
/// `media:` and `status:` lines the same Mac printed for its ports, and the
/// error `ifconfig` gives for an interface that doesn't exist.
fn ifconfig() -> [String; 3] {
    let script = r#"case "$1" in
en10) printf '\tmedia: autoselect (2500Base-T <full-duplex>)\n\tstatus: active\n' ;;
en4) printf '\tmedia: none\n\tstatus: inactive\n' ;;
en1) printf '\tmedia: autoselect <full-duplex>\n\tstatus: inactive\n' ;;
bridge0) printf '\tmedia: <unknown type>\n\tstatus: inactive\n' ;;
en0) printf '\tmedia: autoselect\n\tstatus: active\n' ;;
*) echo "ifconfig: interface $1 does not exist" >&2; exit 1 ;;
esac"#;
    ["-c".to_owned(), script.to_owned(), "ifconfig".to_owned()]
}

/// `system_profiler -json SPAirPortDataType` as macOS 27 prints it, cut to
/// what the reader looks at: the Wi-Fi port `en0`, whose current network is
/// `current` (`None` leaves the block out), and `awdl0`, whose block has no
/// name.
fn profile(current: Option<serde_json::Value>) -> String {
    let mut en0 = serde_json::json!({
        "_name": "en0",
        "spairport_status_information": "spairport_status_connected",
    });
    if let Some(current) = current {
        en0["spairport_current_network_information"] = current;
    }
    serde_json::json!({ "SPAirPortDataType": [{
        "spairport_airport_interfaces": [en0, {
            "_name": "awdl0",
            "spairport_current_network_information": {
                "spairport_network_type": "spairport_network_type_station",
            },
        }],
        "spairport_software_information": {},
    }]})
    .to_string()
}

/// The report of a Mac whose `en0` has joined a network named `name`.
fn profile_on(name: serde_json::Value) -> String {
    profile(Some(serde_json::json!({
        "_name": name,
        "spairport_network_type": "spairport_network_type_station",
    })))
}

struct RouteLinkHarness;

impl RouteLinkHarness {
    fn link(route: &str, ports: &str) -> Arc<dyn NetworkLink> {
        Self::named(route, ports, &profile_on("Office".into()))
    }

    /// A Mac whose Wi-Fi report is `profile`, its default route out of
    /// `en0`.
    fn on_wifi(profile: &str) -> Arc<dyn NetworkLink> {
        Self::named(&route_output("en0"), HARDWARE_PORTS, profile)
    }

    fn named(route: &str, ports: &str, profile: &str) -> Arc<dyn NetworkLink> {
        Arc::new(RouteLink::running(
            ("/bin/sh", &printing(route)[..]),
            ("/bin/sh", &printing(ports)[..]),
            ("/bin/sh", &ifconfig()[..]),
            ("/bin/sh", &printing(profile)[..]),
        ))
    }
}

impl NetworkLinkHarness for RouteLinkHarness {
    fn wired(&self) -> Arc<dyn NetworkLink> {
        Self::link(&route_output("en10"), HARDWARE_PORTS)
    }

    fn wifi(&self) -> Arc<dyn NetworkLink> {
        Self::link(&route_output("en0"), HARDWARE_PORTS)
    }

    fn tunnel(&self) -> Arc<dyn NetworkLink> {
        Self::link(&route_output("utun3"), HARDWARE_PORTS)
    }

    fn no_route(&self) -> Arc<dyn NetworkLink> {
        Arc::new(RouteLink::running(
            (
                "/bin/sh",
                &[
                    "-c".to_owned(),
                    "echo 'route: writing to routing socket: not in table' >&2; exit 1".to_owned(),
                ][..],
            ),
            ("/bin/sh", &printing(HARDWARE_PORTS)[..]),
            ("/bin/sh", &ifconfig()[..]),
            ("/bin/sh", &printing(&profile(None))[..]),
        ))
    }

    fn unrecognized(&self) -> Arc<dyn NetworkLink> {
        Self::link(
            "   route to: default\ndestination: default\n",
            HARDWARE_PORTS,
        )
    }

    fn unreadable(&self) -> Arc<dyn NetworkLink> {
        Arc::new(RouteLink::running(
            ("/nonexistent/ladderback-demo/route", &["-n".to_owned()][..]),
            ("/bin/sh", &printing(HARDWARE_PORTS)[..]),
            ("/bin/sh", &ifconfig()[..]),
            ("/bin/sh", &printing(&profile(None))[..]),
        ))
    }

    fn on_network(&self, name: &str) -> Arc<dyn NetworkLink> {
        Self::on_wifi(&profile_on(name.into()))
    }

    fn on_no_network(&self) -> Arc<dyn NetworkLink> {
        Self::on_wifi(&profile(None))
    }

    fn name_hidden(&self) -> Arc<dyn NetworkLink> {
        Self::on_wifi(&profile_on("<redacted>".into()))
    }

    fn no_name_reader(&self) -> Arc<dyn NetworkLink> {
        Arc::new(RouteLink::running(
            ("/bin/sh", &printing(&route_output("en0"))[..]),
            ("/bin/sh", &printing(HARDWARE_PORTS)[..]),
            ("/bin/sh", &ifconfig()[..]),
            (
                "/nonexistent/ladderback-demo/system_profiler",
                &["-json".to_owned()][..],
            ),
        ))
    }

    /// The name where the report has a list of names.
    fn name_unrecognized(&self, name: &str) -> Arc<dyn NetworkLink> {
        Self::on_wifi(&profile_on(serde_json::json!([name])))
    }
}

#[tokio::test]
async fn route_and_networksetup_meet_the_network_link_contract() {
    contract::network_link::run(&RouteLinkHarness).await;
}

/// A port that isn't Wi-Fi is wired only when its medium is Ethernet, whatever
/// its name: a bridge, an Ethernet adapter with no link and a Thunderbolt
/// port are other links.
#[tokio::test]
async fn a_bridge_or_a_port_with_no_ethernet_medium_is_other() {
    for interface in ["bridge0", "en4", "en1"] {
        let link = RouteLinkHarness::link(&route_output(interface), HARDWARE_PORTS);
        assert_eq!(link.link().await, Ok(Link::Other), "{interface}");
    }
}

/// `ifconfig` is asked only of a hardware port that isn't Wi-Fi, so an
/// `ifconfig` that fails leaves Wi-Fi, a tunnel and the network's name
/// readable, and makes only such a port's link an error.
#[tokio::test]
async fn ifconfig_is_asked_only_of_a_port_that_is_not_wi_fi() {
    let without_ifconfig = |interface: &str| {
        RouteLink::running(
            ("/bin/sh", &printing(&route_output(interface))[..]),
            ("/bin/sh", &printing(HARDWARE_PORTS)[..]),
            (
                "/nonexistent/ladderback-demo/ifconfig",
                &["-a".to_owned()][..],
            ),
            ("/bin/sh", &printing(&profile_on("Office".into()))[..]),
        )
    };
    let answer = without_ifconfig("en10").link().await;
    assert!(
        matches!(answer, Err(PlatformError::Unreadable { .. })),
        "{answer:?}"
    );
    assert_eq!(without_ifconfig("en10").network_name().await, Ok(None));
    assert_eq!(without_ifconfig("en0").link().await, Ok(Link::WiFi));
    assert_eq!(without_ifconfig("utun3").link().await, Ok(Link::Other));
}

#[tokio::test]
async fn networksetup_listing_no_port_is_unrecognized() {
    let answer = RouteLinkHarness::link(&route_output("en0"), "VLAN Configurations\n")
        .link()
        .await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "{answer:?}"
    );
}

/// A name macOS leaves out or empties is withheld, never "no network": a
/// rule's `except` must not hold on it (decision 80).
#[tokio::test]
async fn a_name_the_report_leaves_out_or_empties_is_no_answer() {
    let no_name = profile(Some(serde_json::json!({
        "spairport_network_type": "spairport_network_type_station",
    })));
    for report in [
        profile_on("".into()),
        profile_on("a name of more than thirty-two bytes".into()),
        no_name,
    ] {
        let answer = RouteLinkHarness::on_wifi(&report).network_name().await;
        assert!(
            matches!(answer, Err(PlatformError::Unrecognized { .. })),
            "{answer:?}"
        );
    }
}

/// The name read is the Wi-Fi port's, when the default route leaves by it:
/// a route out of another port has none, whatever the report says. A report
/// with no block for the Wi-Fi port, or two, says nothing of it, and one
/// that isn't JSON or isn't the Wi-Fi report is not understood.
#[tokio::test]
async fn the_name_is_the_wi_fi_port_s_when_the_default_route_leaves_by_it() {
    let link = RouteLinkHarness::named(
        &route_output("en4"),
        HARDWARE_PORTS,
        &profile_on("Office".into()),
    );
    assert_eq!(link.network_name().await, Ok(None));
    let two = profile_on("Office".into()).replace("\"awdl0\"", "\"en0\"");
    let none = profile_on("Office".into()).replace("\"en0\"", "\"en1\"");
    for report in [
        none,
        two,
        "Wi-Fi:\n  Current Network Information:".to_owned(),
        "{\"SPUSBDataType\": []}".to_owned(),
    ] {
        let answer = RouteLinkHarness::on_wifi(&report).network_name().await;
        assert!(
            matches!(answer, Err(PlatformError::Unrecognized { .. })),
            "{answer:?}"
        );
    }
}

/// The report runs past the 64 KiB another program is held to: it lists
/// the networks around. It is read up to 1 MiB.
#[tokio::test]
async fn a_wi_fi_report_is_read_up_to_a_mebibyte() {
    // The report with `bytes` of padding first, printed by a script: an
    // argument that long is more than Linux passes.
    let padded = |bytes: usize| {
        let report = profile_on("Office".into());
        let rest = report.strip_prefix('{').expect("an object");
        let script = format!(
            "printf '{{\"padding\":\"'; head -c {bytes} /dev/zero | tr '\\0' x; printf '\",'; cat <<'EOF'\n{rest}\nEOF"
        );
        RouteLink::running(
            ("/bin/sh", &printing(&route_output("en0"))[..]),
            ("/bin/sh", &printing(HARDWARE_PORTS)[..]),
            ("/bin/sh", &ifconfig()[..]),
            ("/bin/sh", &["-c".to_owned(), script][..]),
        )
    };
    assert_eq!(
        padded(200 * 1024).network_name().await,
        Ok(CurrentNetworkName::new(b"Office"))
    );
    let answer = padded(1024 * 1024).network_name().await;
    assert!(
        matches!(answer, Err(PlatformError::Unrecognized { .. })),
        "{answer:?}"
    );
}

#[tokio::test]
async fn the_system_link_is_this_platforms() {
    let link = platform::network_link::system_link().link().await;
    assert!(
        matches!(link, Ok(_) | Err(PlatformError::Unreadable { .. })),
        "{link:?}"
    );
}
