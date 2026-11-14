//! Stand-ins for the user's service manager (operations §12): a `systemctl`
//! and a `launchctl` that keep, in a scratch folder, which services they
//! run, and a list of every call with the environment it came with. A test
//! that puts [`FakeServiceManagers::bin`] first, or alone, on the `PATH` of
//! what it runs never reaches the real managers.
//!
//! Each knows only the calls Ladderback makes: `systemctl --user` with
//! `daemon-reload`, `enable <file>`, `restart <unit>` and `disable --now
//! <unit>`; `launchctl` with `print`, `enable`, `bootstrap` and `bootout` in
//! the `gui/<uid>` domain. They answer what the real ones answer to a call
//! that can't succeed: an `enable` of a missing file, a `bootstrap` of a
//! service already running, a `bootout` of one that isn't. Told to, the
//! `launchctl` keeps an agent it booted out for a while, as the real one
//! does while the agent's program is still stopping
//! ([`FakeServiceManagers::stop_slowly`]).

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
/// refusal's code (1 unless told otherwise).
const PRELUDE: &str = r#"#!/bin/sh
state='@STATE@'
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
    : > "$state/enabled-${3##*/}" ;;
  restart)
    [ -f "$state/enabled-$3" ] || { echo "Failed to restart $3: Unit $3 not found." >&2; exit 5; }
    : > "$state/runs-$3" ;;
  disable)
    [ "$3" = --now ] || { echo "only disable --now is faked" >&2; exit 64; }
    /bin/rm -f "$state/enabled-$4" "$state/runs-$4" ;;
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
    [ -f "$state/runs-$label" ] || { echo "Could not find service \"$label\" in domain for user gui" >&2; exit 113; } ;;
  enable) ;;
  bootstrap)
    [ -f "$3" ] || { echo "Bootstrap failed: 2: No such file or directory" >&2; exit 2; }
    label=${3##*/}; label=${label%.plist}
    [ ! -f "$state/runs-$label" ] || { echo "Bootstrap failed: 5: Input/output error" >&2; exit 5; }
    : > "$state/runs-$label" ;;
  bootout)
    [ -f "$state/runs-${2##*/}" ] || { echo "Boot-out failed: 3: No such process" >&2; exit 3; }
    if [ -f "$state/stop-slowly" ]; then
      /bin/cp "$state/stop-slowly" "$state/stopping-${2##*/}"
    else
      /bin/rm -f "$state/runs-${2##*/}"
    fi ;;
  *) echo "unknown command $1" >&2; exit 64 ;;
esac
"#;

impl FakeServiceManagers {
    /// Writes both stand-ins into `root/bin`, keeping their state in
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
        for (name, body) in [("systemctl", SYSTEMCTL), ("launchctl", LAUNCHCTL)] {
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

    /// The folder holding `systemctl` and `launchctl`.
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
            .filter(|line| line.starts_with("systemctl ") || line.starts_with("launchctl "))
            .map(str::to_owned)
            .collect()
    }

    /// Whether the service `name` runs: a systemd unit's name
    /// (`ladderback.service`) or a launchd label.
    pub fn runs(&self, name: &str) -> bool {
        self.state().join(format!("runs-{name}")).exists()
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
}
