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
use domain::planning::Link;
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
        }
    }

    fn machine(
        &self,
        route: &str,
        ipv6_route: Option<&str>,
        interfaces: &[Interface<'_>],
    ) -> Arc<dyn NetworkLink> {
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
        Arc::new(KernelLink::at(
            root.join("route"),
            root.join("ipv6_route"),
            net,
        ))
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

    fn wired_reads_as(&self) -> Link {
        Link::Wired
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
}

#[tokio::test]
async fn the_kernels_routes_and_interfaces_meet_the_network_link_contract() {
    contract::network_link::run(&KernelLinkHarness::new()).await;
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

/// `networksetup -listallhardwareports` as the Mac this was written on
/// prints it, its addresses made up.
const HARDWARE_PORTS: &str = "\nHardware Port: Ethernet Adapter (en4)\nDevice: en4\nEthernet Address: 02:00:00:00:00:04\n\nHardware Port: Thunderbolt Bridge\nDevice: bridge0\nEthernet Address: 02:00:00:00:00:05\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: 02:00:00:00:00:00\n\nVLAN Configurations\n===================\n";

struct RouteLinkHarness;

impl RouteLinkHarness {
    fn link(route: &str, ports: &str) -> Arc<dyn NetworkLink> {
        Arc::new(RouteLink::running(
            ("/bin/sh", &printing(route)[..]),
            ("/bin/sh", &printing(ports)[..]),
        ))
    }
}

impl NetworkLinkHarness for RouteLinkHarness {
    fn wired(&self) -> Arc<dyn NetworkLink> {
        Self::link(&route_output("en4"), HARDWARE_PORTS)
    }

    fn wired_reads_as(&self) -> Link {
        // No Mac on Ethernet has been seen yet (decision 80): a port that
        // isn't Wi-Fi is only that.
        Link::NotWiFi
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
        ))
    }
}

#[tokio::test]
async fn route_and_networksetup_meet_the_network_link_contract() {
    contract::network_link::run(&RouteLinkHarness).await;
}

#[tokio::test]
async fn a_thunderbolt_bridge_is_not_wi_fi() {
    let link = RouteLinkHarness::link(&route_output("bridge0"), HARDWARE_PORTS);
    assert_eq!(link.link().await, Ok(Link::NotWiFi));
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

#[tokio::test]
async fn the_system_link_is_this_platforms() {
    let link = platform::network_link::system_link().link().await;
    assert!(
        matches!(link, Ok(_) | Err(PlatformError::Unreadable { .. })),
        "{link:?}"
    );
}
