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
use app::contract::power::PowerHarness;
use app::ports::{Network, PlatformError, PowerSupply};
use domain::execution::conditions::PowerSource;
use platform::network::{ScutilNetwork, SysfsNetwork};
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

/// Lets a reader stuck on `path` go, so the runtime can end.
fn release(path: PathBuf) {
    std::thread::spawn(move || {
        if let Ok(mut pipe) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = std::io::Write::write_all(&mut pipe, b"Mains\n");
        }
    });
}

#[tokio::test]
async fn a_read_that_stalls_is_given_up_at_the_bound() {
    let sysfs = Sysfs::new();
    let root = sysfs.machine(&[("AC", &[])]);
    fifo(&root.join("AC/type"));
    let started = Instant::now();
    let answer = SysfsPower::at(&root).with_timeout(SHORT).source().await;
    assert!(
        matches!(answer, Err(PlatformError::TimedOut { .. })),
        "{answer:?}"
    );
    assert!(started.elapsed() < SHORT * 10);
    release(root.join("AC/type"));

    let root = sysfs.machine(&[("eth0", &[])]);
    fifo(&root.join("eth0/flags"));
    let answer = SysfsNetwork::at(&root)
        .with_timeout(SHORT)
        .is_online()
        .await;
    assert!(
        matches!(answer, Err(PlatformError::TimedOut { .. })),
        "{answer:?}"
    );
    release(root.join("eth0/flags"));
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
