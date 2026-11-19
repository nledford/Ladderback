//! Stand-ins for the user's service manager (operations §12): a `systemctl`
//! and a `launchctl` that keep, in a scratch folder, which services they
//! run, a `loginctl` that says whether the user lingers, and a list of every
//! call with the environment it came with. A test that puts
//! [`FakeServiceManagers::bin`] first, or alone, on the `PATH` of what it
//! runs never reaches the real managers.
//!
//! Each knows only the calls Ladderback and `just install-restart` make:
//! `systemctl --user` with `daemon-reload`, `enable <file>`, `restart
//! <unit>`, `disable --now <unit>` and `show -p MainPID --value <unit>`;
//! `launchctl` with `print`, `enable`, `bootstrap` and `bootout` in the
//! `gui/<uid>` domain; `loginctl show-user <uid> -p Linger`. They answer
//! what the real ones answer to a call that can't succeed: an `enable` of a
//! missing file, a `bootstrap` of a service already running, a `bootout` of
//! one that isn't. Each start of a service gives it a new process ID, which
//! `show` and `print` name as the real ones do, unless told to keep the one
//! it had ([`FakeServiceManagers::keep_process_on_restart`]). Told to, the
//! `launchctl` keeps an agent it booted out for a while, as the real one does
//! while the agent's program is still stopping
//! ([`FakeServiceManagers::stop_slowly`]), or stops answering once it has
//! booted one out ([`FakeServiceManagers::refuse_after_bootout`]), the
//! `systemctl` takes a while to return from a call that stops the unit, as the
//! real one waits for the stop ([`FakeServiceManagers::stop_taking`]), and the
//! `loginctl` says the user lingers ([`FakeServiceManagers::linger`]) or fails
//! ([`FakeServiceManagers::refuse_linger_check`]).
//!
//! Each keeps the service file it loaded, at `enable` or `bootstrap`, until
//! it lets the service go, at `disable --now` or `bootout`. A service whose
//! program ends of itself ([`FakeServiceManagers::exits`]) is started again
//! when that file says to, as the real manager reads it.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The stand-ins, and the folder where they keep their state.
#[derive(Clone, Debug)]
pub struct FakeServiceManagers {
    root: PathBuf,
}

/// Every call, its arguments on one line, then its environment as `export
/// -p` prints it. Then, when told to, it hangs or refuses, ending with the
/// refusal's code (1 unless told otherwise). `start` runs the service it
/// names with the next process ID, or the last one again when told to keep
/// it, and writes the ID in the service's `runs-` file.
const PRELUDE: &str = r#"#!/bin/sh
state='@STATE@'
start() {
  pid=4100
  [ ! -f "$state/last-pid" ] || read -r pid < "$state/last-pid"
  [ -f "$state/same-pid" ] || pid=$((pid + 1))
  printf '%s\n' "$pid" > "$state/last-pid"
  printf '%s\n' "$pid" > "$state/runs-$1"
}
{ printf '%s' "${0##*/}"; printf ' %s' "$@"; printf '\n'; export -p; } >> "$state/calls"
if [ -f "$state/hang" ]; then exec /bin/sleep 60; fi
if [ -f "$state/refusal" ]; then
  IFS= read -r words < "$state/refusal"
  code=1
  [ ! -f "$state/refusal-code" ] || read -r code < "$state/refusal-code"
  printf '%s\n' "$words" >&2
  exit "$code"
fi
"#;

const SYSTEMCTL: &str = r#"[ "$1" = --user ] || { echo "only --user is faked" >&2; exit 64; }
case "$2" in
  daemon-reload) ;;
  enable)
    [ -f "$3" ] || { echo "Failed to enable unit: Unit file $3 does not exist." >&2; exit 1; }
    : > "$state/enabled-${3##*/}"
    /bin/cp "$3" "$state/loaded-${3##*/}" ;;
  restart)
    [ -f "$state/enabled-$3" ] || { echo "Failed to restart $3: Unit $3 not found." >&2; exit 5; }
    [ ! -f "$state/stop-takes" ] || { read -r took < "$state/stop-takes"; /bin/sleep "$took"; }
    start "$3" ;;
  disable)
    [ "$3" = --now ] || { echo "only disable --now is faked" >&2; exit 64; }
    [ ! -f "$state/stop-takes" ] || { read -r took < "$state/stop-takes"; /bin/sleep "$took"; }
    /bin/rm -f "$state/enabled-$4" "$state/runs-$4" "$state/loaded-$4" ;;
  show)
    [ "$3" = -p ] && [ "$4" = MainPID ] && [ "$5" = --value ] || { echo "only show -p MainPID --value is faked" >&2; exit 64; }
    if [ -f "$state/runs-$6" ]; then /bin/cat "$state/runs-$6"; else echo 0; fi ;;
  *) echo "unknown command $2" >&2; exit 64 ;;
esac
"#;

const LAUNCHCTL: &str = r#"case "$1" in
  print)
    label=${2##*/}
    if [ -f "$state/stopping-$label" ]; then
      read -r left < "$state/stopping-$label"
      if [ "$left" -gt 0 ]; then
        printf '%s\n' "$((left - 1))" > "$state/stopping-$label"
      else
        /bin/rm -f "$state/stopping-$label" "$state/runs-$label"
      fi
    fi
    [ -f "$state/runs-$label" ] || { echo "Could not find service \"$label\" in domain for user gui" >&2; exit 113; }
    read -r pid < "$state/runs-$label"
    printf '%s = {\n\tstate = running\n\tpid = %s\n}\n' "$2" "$pid" ;;
  enable) ;;
  bootstrap)
    [ -f "$3" ] || { echo "Bootstrap failed: 2: No such file or directory" >&2; exit 2; }
    label=${3##*/}; label=${label%.plist}
    [ ! -f "$state/runs-$label" ] || { echo "Bootstrap failed: 5: Input/output error" >&2; exit 5; }
    /bin/cp "$3" "$state/loaded-$label"
    start "$label" ;;
  bootout)
    [ -f "$state/runs-${2##*/}" ] || { echo "Boot-out failed: 3: No such process" >&2; exit 3; }
    /bin/rm -f "$state/loaded-${2##*/}"
    if [ -f "$state/stop-slowly" ]; then
      /bin/cp "$state/stop-slowly" "$state/stopping-${2##*/}"
    else
      /bin/rm -f "$state/runs-${2##*/}"
    fi
    # Armed as this call ends, so the refusal reaches only the calls after it.
    [ ! -f "$state/refusal-after-bootout" ] || /bin/mv "$state/refusal-after-bootout" "$state/refusal" ;;
  *) echo "unknown command $1" >&2; exit 64 ;;
esac
"#;

/// The user doesn't linger unless told to, as on a machine where nobody ran
/// `loginctl enable-linger`.
const LOGINCTL: &str = r#"[ "$#" = 4 ] && [ "$1" = show-user ] && [ "$3" = -p ] && [ "$4" = Linger ] || { echo "only show-user <uid> -p Linger is faked" >&2; exit 64; }
if [ -f "$state/linger-refusal" ]; then
  IFS= read -r words < "$state/linger-refusal"
  printf '%s\n' "$words" >&2
  exit 1
fi
if [ -f "$state/lingers" ]; then echo Linger=yes; else echo Linger=no; fi
"#;

impl FakeServiceManagers {
    /// Writes the stand-ins into `root/bin`, keeping their state in
    /// `root/state`.
    ///
    /// # Panics
    ///
    /// When the folder can't be written.
    pub fn install(root: &Path) -> Self {
        let fakes = Self {
            root: root.to_path_buf(),
        };
        fs::create_dir_all(fakes.bin()).expect("the stand-ins' folder");
        fs::create_dir_all(fakes.state()).expect("the stand-ins' state folder");
        let prelude = PRELUDE.replace("@STATE@", &fakes.state().display().to_string());
        for (name, body) in [
            ("systemctl", SYSTEMCTL),
            ("launchctl", LAUNCHCTL),
            ("loginctl", LOGINCTL),
        ] {
            let path = fakes.bin().join(name);
            // Written while no child starts: a child forked meanwhile would
            // hold the file open for writing, and running it would fail with
            // "text file busy".
            spawn_gate::hold(|| {
                fs::write(&path, format!("{prelude}{body}")).expect("a stand-in is written");
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                    .expect("a stand-in is made runnable");
            });
        }
        fakes
    }

    /// The folder holding `systemctl`, `launchctl` and `loginctl`.
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    /// Every call so far, each its arguments on one line (`systemctl --user
    /// daemon-reload`) and then the environment it came with.
    pub fn calls(&self) -> String {
        fs::read_to_string(self.state().join("calls")).unwrap_or_default()
    }

    /// The calls so far, their arguments only, one to a line.
    pub fn commands(&self) -> Vec<String> {
        self.calls()
            .lines()
            .filter(|line| {
                ["systemctl ", "launchctl ", "loginctl "]
                    .iter()
                    .any(|program| line.starts_with(program))
            })
            .map(str::to_owned)
            .collect()
    }

    /// Whether the service `name` runs: a systemd unit's name
    /// (`ladderback.service`) or a launchd label.
    pub fn runs(&self, name: &str) -> bool {
        self.state().join(format!("runs-{name}")).exists()
    }

    /// The process ID the service `name` runs under, while it runs.
    pub fn process(&self, name: &str) -> Option<u32> {
        fs::read_to_string(self.state().join(format!("runs-{name}")))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Ends the service `name`'s process, as when it crashed and its manager
    /// gave up starting it again.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn end(&self, name: &str) {
        match fs::remove_file(self.state().join(format!("runs-{name}"))) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("the service's process ends: {error}"),
        }
    }

    /// The service `name`'s program ends with `status` of itself, not
    /// because its manager stopped it: a crash, a restart's status, or a
    /// SIGTERM another program sent. A manager that still has the service
    /// starts it again, with a new process, when the service file it loaded
    /// says to (`starts_again`, below); one that has let it go (`disable
    /// --now`, `bootout`) starts nothing.
    ///
    /// # Panics
    ///
    /// When the state folder can't be read or written.
    pub fn exits(&self, name: &str, status: u8) {
        self.end(name);
        let file = match fs::read_to_string(self.state().join(format!("loaded-{name}"))) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("the service file the manager loaded: {error}"),
        };
        if starts_again(&file, status) {
            self.start(name);
        }
    }

    /// Starts the service `name` as the stand-ins' `start` does: with the
    /// next process ID, or the last one again when told to keep it.
    fn start(&self, name: &str) {
        let last = fs::read_to_string(self.state().join("last-pid"))
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok())
            .unwrap_or(4100);
        let pid = if self.state().join("same-pid").exists() {
            last
        } else {
            last + 1
        };
        fs::write(self.state().join("last-pid"), format!("{pid}\n")).expect("the last process ID");
        fs::write(
            self.state().join(format!("runs-{name}")),
            format!("{pid}\n"),
        )
        .expect("the service runs");
    }

    /// Makes each start from now on keep the process ID the last one gave,
    /// as if the restart never happened.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn keep_process_on_restart(&self) {
        fs::write(self.state().join("same-pid"), "").expect("the kept process");
    }

    /// Makes every call from now on fail with exit code 1, saying `words` on
    /// its stderr.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn refuse(&self, words: &str) {
        self.refuse_ending(words, 1);
    }

    /// Makes every call from now on fail with exit code `code`, saying
    /// `words` on its stderr.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn refuse_ending(&self, words: &str, code: u8) {
        fs::write(self.state().join("refusal-code"), format!("{code}\n"))
            .expect("the refusal's code");
        fs::write(self.state().join("refusal"), format!("{words}\n")).expect("the refusal");
    }

    /// Makes every call after the next `launchctl bootout` that boots an
    /// agent out fail with exit code 1, saying `words` on its stderr, as a
    /// launchd that stops answering once it has been told to boot the agent
    /// out. The `bootout` itself answers as before.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn refuse_after_bootout(&self, words: &str) {
        fs::write(self.state().join("refusal-code"), "1\n").expect("the refusal's code");
        fs::write(
            self.state().join("refusal-after-bootout"),
            format!("{words}\n"),
        )
        .expect("the refusal after the boot-out");
    }

    /// Makes every call from now on hang for a minute.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn hang(&self) {
        fs::write(self.state().join("hang"), "").expect("the hang");
    }

    /// Makes each `bootout` from now on return while the agent is still
    /// stopping, as launchd's does: launchd keeps the agent for the next
    /// `checks` calls of `print`, which find it loaded, and refuses a
    /// `bootstrap` of its label meanwhile ("Bootstrap failed: 5:
    /// Input/output error"). The call after those finds it gone.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn stop_slowly(&self, checks: u32) {
        fs::write(self.state().join("stop-slowly"), format!("{checks}\n")).expect("the slow stop");
    }

    /// Makes `loginctl` say from now on that the user lingers.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn linger(&self) {
        fs::write(self.state().join("lingers"), "").expect("the lingering");
    }

    /// Makes each `loginctl` call from now on fail with exit code 1, saying
    /// `words` on its stderr; `systemctl` and `launchctl` answer as before.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn refuse_linger_check(&self, words: &str) {
        fs::write(self.state().join("linger-refusal"), format!("{words}\n"))
            .expect("the linger check's refusal");
    }

    /// Makes each `systemctl restart` and `disable --now` from now on take
    /// `took` before it returns, as the real one does while the unit it
    /// stops is still stopping.
    ///
    /// # Panics
    ///
    /// When the state folder can't be written.
    pub fn stop_taking(&self, took: std::time::Duration) {
        fs::write(
            self.state().join("stop-takes"),
            format!("{:.3}\n", took.as_secs_f64()),
        )
        .expect("the stop's length");
    }
}

/// Whether a manager that loaded `file` starts its service again once the
/// program ends with `status` of itself, as the manager's manual says for
/// the settings a service file of Ladderback's can hold.
///
/// - launchd (`launchd.plist(5)`): `KeepAlive` true keeps the agent running
///   whatever ended it; a `KeepAlive` dictionary whose `SuccessfulExit` is
///   false starts it again only after a status other than 0, and one whose
///   `SuccessfulExit` is true only after 0. With no `KeepAlive`, never.
/// - systemd (`systemd.service(5)`): after a status `RestartForceExitStatus=`
///   names, whatever `Restart=` says; otherwise `Restart=always` after any
///   status, `on-failure` after an unclean one (neither 0 nor one
///   `SuccessExitStatus=` names), and `no`, the default, never.
fn starts_again(file: &str, status: u8) -> bool {
    if file.starts_with("<?xml") {
        launchd_starts_again(file, status)
    } else {
        systemd_starts_again(file, status)
    }
}

fn launchd_starts_again(plist: &str, status: u8) -> bool {
    let Some((_, keep_alive)) = plist.split_once("<key>KeepAlive</key>") else {
        return false;
    };
    let keep_alive = keep_alive.trim_start();
    if keep_alive.starts_with("<true/>") {
        return true;
    }
    let Some(dict) = keep_alive.strip_prefix("<dict>") else {
        return false;
    };
    let dict = dict.split_once("</dict>").map_or(dict, |(dict, _)| dict);
    match dict
        .split_once("<key>SuccessfulExit</key>")
        .map(|(_, value)| value.trim_start())
    {
        Some(value) if value.starts_with("<false/>") => status != 0,
        Some(value) if value.starts_with("<true/>") => status == 0,
        _ => false,
    }
}

fn systemd_starts_again(unit: &str, status: u8) -> bool {
    let setting = |key: &str| {
        unit.lines()
            .filter_map(|line| line.strip_prefix(key))
            .next_back()
    };
    let names_status = |key: &str| {
        setting(key).is_some_and(|statuses| {
            statuses
                .split_whitespace()
                .any(|named| named.parse::<u8>() == Ok(status))
        })
    };
    let clean = status == 0 || names_status("SuccessExitStatus=");
    names_status("RestartForceExitStatus=")
        || match setting("Restart=") {
            Some("always") => true,
            Some("on-failure") => !clean,
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use super::starts_again;

    const PLIST_HEAD: &str =
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n";

    fn plist(keep_alive: &str) -> String {
        format!(
            "{PLIST_HEAD}\t<key>RunAtLoad</key>\n\t<true/>\n{keep_alive}\t<key>ExitTimeOut</key>\n\t<integer>75</integer>\n</dict>\n</plist>\n"
        )
    }

    fn unit(restart: &str) -> String {
        format!(
            "[Service]\nType=exec\n{restart}RestartSec=10\n\n[Install]\nWantedBy=default.target\n"
        )
    }

    #[test]
    fn launchd_keeps_an_agent_alive_whatever_ended_it() {
        let file = plist("\t<key>KeepAlive</key>\n\t<true/>\n");
        for status in [0, 1, 75] {
            assert!(starts_again(&file, status), "status {status}");
        }
    }

    #[test]
    fn launchd_with_successful_exit_false_starts_again_only_after_a_failure() {
        let file = plist(
            "\t<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<false/>\n\t</dict>\n",
        );
        assert!(!starts_again(&file, 0));
        assert!(starts_again(&file, 1));
        assert!(starts_again(&file, 75));
    }

    #[test]
    fn launchd_with_successful_exit_true_starts_again_only_after_a_success() {
        let file = plist(
            "\t<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<true/>\n\t</dict>\n",
        );
        assert!(starts_again(&file, 0));
        assert!(!starts_again(&file, 1));
    }

    #[test]
    fn launchd_without_keep_alive_never_starts_again() {
        let file = plist("");
        assert!(!starts_again(&file, 0));
        assert!(!starts_again(&file, 1));
        let other = plist("\t<key>KeepAlive</key>\n\t<false/>\n");
        assert!(!starts_again(&other, 1));
    }

    #[test]
    fn systemd_restart_always_starts_again_after_any_status() {
        let file = unit("Restart=always\nSuccessExitStatus=75\n");
        for status in [0, 1, 75] {
            assert!(starts_again(&file, status), "status {status}");
        }
    }

    #[test]
    fn systemd_on_failure_starts_again_only_after_an_unclean_status() {
        let file = unit("Restart=on-failure\nSuccessExitStatus=75\n");
        assert!(!starts_again(&file, 0));
        assert!(starts_again(&file, 1));
        // 75 is clean here, so only a force brings the unit back.
        assert!(!starts_again(&file, 75));
        let forced = unit("Restart=on-failure\nRestartForceExitStatus=75\nSuccessExitStatus=75\n");
        assert!(starts_again(&forced, 75));
        assert!(!starts_again(&forced, 0));
    }

    #[test]
    fn systemd_without_restart_starts_again_only_on_a_forced_status() {
        let file = unit("");
        assert!(!starts_again(&file, 1));
        let forced = unit("RestartForceExitStatus=3 75\n");
        assert!(starts_again(&forced, 75));
        assert!(starts_again(&forced, 3));
        assert!(!starts_again(&forced, 1));
    }
}
