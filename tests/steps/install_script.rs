//! Steps for `features/cli/install_script.feature`: `scripts/install.sh`, read
//! by bash from its standard input as `curl … | bash` gives it, with an
//! environment of its own: a scratch home and a `PATH` that holds stand-ins
//! alone, so no real tool is installed, built or downloaded, and no real
//! package manager or `sudo` is reached.
//!
//! The stand-ins keep, in the scenario's scratch folder, a list of every call
//! (`install/state/calls`, one line each, the command's name and its
//! arguments) and the answers a scenario chose: the version each tool
//! reports, the system `uname` names, the glibc, processors and free memory
//! `getconf` reports, the toolchains rustup has (`install/state/toolchains`).
//! Each cargo call also records the Rust it builds with, chosen as rustup and
//! cargo choose it (`install/state/builds`). Tools a stand-in installs
//! (`cargo install`, rustup's installer) are copied from
//! `install/state/templates` into `~/.cargo/bin`, as the real ones would be.
//! `curl` serves trunk's release archive from `install/state`: a tarball
//! holding a trunk stand-in, built when a scenario removes trunk. It serves
//! Ladderback's newest release from `install/state/release`, the archives and
//! `SHA256SUMS` a scenario wrote there, and answers 404, as GitHub does, for
//! a file that isn't there. The `sha256sum` stand-in gives the genuine trunk
//! archive the SHA-256 the script pins for the file's name, read from the
//! script, and any other file its real SHA-256, so a tampered archive fails
//! the script's check.
//! `git` is the real one behind a stand-in that logs its calls, and the home
//! folder's git configuration sends the script's clone of Ladderback's URL to
//! `upstream`, a local repository with releases of the scenario's choosing.
//! The prompt's terminal is a file the scenario writes, or none at all:
//! `LADDERBACK_INSTALL_TTY` names it, so no real terminal is ever asked.

use std::assert_matches;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cucumber::{given, then, when};
use sha2::{Digest, Sha256};

use super::interop_steps::{Ran, ran};
use super::{Agent, blocking};

/// The script under test, as the repository holds it.
const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/install.sh");

/// The address the script clones Ladderback from.
const URL: &str = "https://github.com/nledford/Ladderback.git";

/// The version of wasm-bindgen the `just` stand-in says the checkout pins.
const WASM_BINDGEN: &str = "0.2.105";

/// The Rust each release pins in its `rust-toolchain.toml`.
const PINNED: &str = "1.100.0";

/// The system the stand-in rustup's toolchains are for.
const HOST: &str = "x86_64-unknown-linux-gnu";

/// The web UI's stylesheet as a release that tracks it holds it.
const RELEASED_STYLESHEET: &str = "/* built */\n";

#[derive(Debug, Default)]
pub(super) struct InstallScriptE2e {
    /// Variables the scenario adds to the script's environment.
    env: Vec<(String, String)>,
    /// How many calls the stand-ins had when the scenario's last run began.
    calls_before: usize,
    /// The file the scenario changed in the source folder, and its text.
    changed: Option<(PathBuf, String)>,
    /// The file the scenario put in a folder of its own, and its text.
    notes: Option<(PathBuf, String)>,
}

/// Every call, its name and arguments on one line.
const PRELUDE: &str = r#"#!/bin/sh
state='@STATE@'
{ printf '%s' "${0##*/}"; for arg in "$@"; do printf ' %s' "$arg"; done; printf '\n'; } >> "$state/calls"
"#;

const NODE: &str = r#"[ "$1" = --version ] || exit 64
printf 'v%s\n' "$(/bin/cat "$state/node-version")"
"#;

const RUSTC: &str = r#"case "$1" in
  --version) printf 'rustc %s (stand-in)\n' "$(/bin/cat "$state/rustc-version")" ;;
  --print) [ "$2" = sysroot ] || exit 64; printf '%s\n' "$state/sysroot" ;;
  *) exit 64 ;;
esac
"#;

/// Installs a `ladderback` that reports the version the checked-out
/// `Cargo.toml` holds, into `bin` under `--root` or `~/.cargo`, as `cargo
/// install --path .` would. Its arguments are `cargo install`'s. As cargo
/// does, it lists what it installed in the root's `.crates.toml`, and
/// refuses to overwrite a `ladderback` it didn't install, such as a release
/// archive's, without `--force`.
const INSTALL_LADDERBACK: &str = r#"root="$HOME/.cargo"
force=0
while [ $# -gt 0 ]; do
  case "$1" in
    --root) root=$2; shift ;;
    --root=*) root=${1#--root=} ;;
    --force) force=1 ;;
  esac
  shift
done
tracked=0
if /usr/bin/grep -qx ladderback "$root/.crates.toml" 2>/dev/null; then tracked=1; fi
if [ -e "$root/bin/ladderback" ] && [ "$tracked" = 0 ] && [ "$force" = 0 ]; then
  echo 'error: binary `ladderback` already exists in destination' >&2
  echo 'Add --force to overwrite' >&2
  exit 101
fi
version=$(/usr/bin/sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml)
/bin/mkdir -p "$root/bin"
printf '#!/bin/sh\necho "ladderback %s"\n' "$version" > "$root/bin/ladderback"
/bin/chmod 755 "$root/bin/ladderback"
[ "$tracked" = 1 ] || echo ladderback >> "$root/.crates.toml"
"#;

/// `cargo +stable install --locked [-j <n>] <tool>…` installs the tools'
/// stand-ins; `cargo install --locked --path .`, as `just install` runs it,
/// installs Ladderback's; the wasm-bindgen build is only recorded.
///
/// Each call first records the Rust it builds with: the toolchain rustup
/// chooses for it (`+<name>`, then RUSTUP_TOOLCHAIN, then the channel of the
/// nearest `rust-toolchain.toml` from the working folder up, then rustup's
/// default), unless RUSTC or CARGO_BUILD_RUSTC names a compiler for cargo to
/// run instead, RUSTC first.
const CARGO: &str = r#"rust=default
dir=$PWD
while :; do
  if [ -f "$dir/rust-toolchain.toml" ]; then
    rust=$(/usr/bin/sed -n 's/^channel = "\(.*\)"$/\1/p' "$dir/rust-toolchain.toml")
    break
  fi
  [ -n "$dir" ] || break
  dir=${dir%/*}
done
[ -z "$RUSTUP_TOOLCHAIN" ] || rust=$RUSTUP_TOOLCHAIN
case "$1" in +*) rust=${1#+} ;; esac
[ -z "$CARGO_BUILD_RUSTC" ] || rust=$CARGO_BUILD_RUSTC
[ -z "$RUSTC" ] || rust=$RUSTC
printf '%s %s\n' "$rust" "$*" >> "$state/builds"
if [ "$1" = +stable ] && [ "$2" = install ] && [ "$3" = --locked ]; then
  shift 3
  if [ "$1" = -j ]; then shift 2; fi
  /bin/mkdir -p "$HOME/.cargo/bin"
  for tool in "$@"; do
    tool=${tool%@*}
    /bin/cp "$state/templates/$tool" "$HOME/.cargo/bin/$tool" || exit 1
    [ "$tool" != just ] || echo 1.43.0 > "$state/just-version"
  done
  exit 0
fi
[ "$1 $2 $3" = "install --locked wasm-bindgen-cli" ] && exit 0
if [ "$1 $2 $3 $4" = "install --locked --path ." ]; then
  shift 4
  exec /bin/sh "$state/templates/install-ladderback" "$@"
fi
echo "the cargo stand-in doesn't know this call" >&2
exit 64
"#;

/// rustup, with the toolchains it has: a file each in
/// `install/state/toolchains`, named as `rustup toolchain list` names the
/// toolchain, that lists its components as `rustup component list
/// --installed` does. `toolchain install` adds the toolchain, or the
/// components and targets it lacks.
const RUSTUP: &str = r#"host='@HOST@'
toolchains="$state/toolchains"
case "$1" in
  --version) echo 'rustup 1.28.2 (stand-in)' ;;
  toolchain)
    case "$2" in
      list)
        # As the real one does, it marks the default toolchain: the first.
        mark=' (default)'
        for file in "$toolchains"/*; do
          [ -f "$file" ] || continue
          printf '%s%s\n' "${file##*/}" "$mark"
          mark=''
        done
        [ -z "$mark" ] || echo 'no installed toolchains'
        ;;
      install)
        file="$toolchains/$3-$host"
        shift 3
        /bin/mkdir -p "$toolchains"
        printf 'cargo-%s\nrustc-%s\nrust-std-%s\n' "$host" "$host" "$host" >> "$file"
        while [ $# -gt 0 ]; do
          case "$1" in
            --no-self-update) ;;
            --component | --target)
              kind=$1
              old=$IFS
              IFS=,
              for item in $2; do
                if [ "$kind" = --component ]; then
                  printf '%s-%s\n' "$item" "$host"
                else
                  printf 'rust-std-%s\n' "$item"
                fi
              done >> "$file"
              IFS=$old
              shift
              ;;
            *) echo "the rustup stand-in doesn't know $1" >&2; exit 64 ;;
          esac
          shift
        done
        ;;
      *) exit 64 ;;
    esac
    ;;
  component)
    [ "$2 $3 $4" = "list --installed --toolchain" ] || exit 64
    if [ ! -f "$toolchains/$5-$host" ]; then
      echo "error: toolchain '$5' is not installed" >&2
      exit 1
    fi
    /bin/cat "$toolchains/$5-$host"
    ;;
  *) exit 64 ;;
esac
"#;

/// `just install` rewrites the web UI's stylesheet, as the UI's build does
/// (Trunk's `pre_build` hook regenerates `crates/web/tw-out.css`, which
/// releases up to v0.2.0 track), then installs Ladderback with cargo, unless
/// the scenario made the build fail.
const JUST: &str = r#"case "$1" in
  --version) printf 'just %s\n' "$(/bin/cat "$state/just-version")" ;;
  _wasm-bindgen-version) echo '@WASM_BINDGEN@' ;;
  install)
    shift
    printf '/* rebuilt */\n' >> crates/web/tw-out.css || exit 1
    if [ -e "$state/build-fails" ]; then echo "error: the build failed" >&2; exit 1; fi
    exec cargo install --locked --path . "$@" ;;
  *) exit 64 ;;
esac
"#;

const TRUNK: &str = r#"[ "$1" = --version ] || exit 64
echo 'trunk 0.21.14'
"#;

/// The trunk inside the release archive the `curl` stand-in serves.
const RELEASED_TRUNK: &str = r#"[ "$1" = --version ] || exit 64
echo 'trunk (from its release archive)'
"#;

/// `xcode-select -p` names the Command Line Tools' folder when they are
/// installed.
const XCODE_SELECT: &str = r#"[ "$1" = -p ] || exit 64
echo /Library/Developer/CommandLineTools
"#;

/// `xcode-select -p` on a Mac without the Command Line Tools.
const NO_COMMAND_LINE_TOOLS: &str = r#"echo "xcode-select: error: unable to get active developer directory" >&2
exit 2
"#;

/// A cargo or rustc the system's packages installed: it knows no `+stable`
/// and has no WebAssembly target, so it is recorded and refuses.
const SYSTEM_RUST: &str = r#"printf '%s\n' "${0##*/}" >> "$state/system-rust"
echo "the system's ${0##*/} can't build this" >&2
exit 1
"#;

const UNAME: &str = r#"case "$1" in
  -s) /bin/cat "$state/uname-s" ;;
  -m) /bin/cat "$state/uname-m" ;;
  *) exit 64 ;;
esac
"#;

/// `getconf`: the glibc (none, as on musl, without `install/state/glibc`),
/// the width of the system's programs, the processors online, and the free
/// memory in pages of 4 KiB.
const GETCONF: &str = r#"case "$1" in
  GNU_LIBC_VERSION) [ -f "$state/glibc" ] || exit 1; printf 'glibc %s\n' "$(/bin/cat "$state/glibc")" ;;
  LONG_BIT) /bin/cat "$state/long-bit" ;;
  _NPROCESSORS_ONLN) /bin/cat "$state/processors" ;;
  _AVPHYS_PAGES) /bin/cat "$state/free-pages" ;;
  PAGESIZE) echo 4096 ;;
  *) exit 64 ;;
esac
"#;

/// Saves trunk's release archive for a URL under trunk's releases, a file of
/// Ladderback's newest release for a URL under its releases, and rustup's
/// installer, the `rustup-init` stand-in, for any other, where `-o` says. A
/// release file that isn't there is a 404, as `--fail` reports it from
/// GitHub with curl 8.7 over HTTP/2: exit 56, not the 22 curl's manual gives.
/// With `--write-out`, it prints the HTTP status, as `%{http_code}` does.
/// A URL that starts with the one `install/state/curl-stalls` names stalls:
/// given a stall limit (`--speed-limit` and `--speed-time`), it is cut off
/// as the real curl cuts it off, exit 28 and curl's message, leaving the
/// part it got in the file; without one, the real download would never end,
/// which the stand-in reports as an exit of its own rather than hang the
/// scenario. One that starts with the URL `install/state/curl-fails` names
/// gets a server error, after curl's retries. Where `install/state/release-http`
/// is a folder, a release URL redirects to plain HTTP, which serves that
/// folder's files: curl follows it, unless `--proto` allows only HTTPS,
/// which holds for redirects too, when it refuses as the real curl 8.7.1
/// does, exit 1, the redirect's status as the last one.
const CURL: &str = r#"out=''
url=''
speed_limit=''
speed_time=''
write_out=''
proto=''
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    --speed-limit) speed_limit=$2; shift ;;
    --speed-time) speed_time=$2; shift ;;
    --write-out) write_out=$2; shift ;;
    --proto) proto=$2; shift ;;
    https://*) url=$1 ;;
  esac
  shift
done
[ -n "$out" ] || exit 64
status() {
  [ -z "$write_out" ] || printf '%s' "$1"
}
stalls=''
[ ! -f "$state/curl-stalls" ] || stalls=$(/bin/cat "$state/curl-stalls")
if [ -n "$stalls" ]; then
  case "$url" in
    "$stalls"*)
      if [ -n "$speed_limit" ] && [ -n "$speed_time" ]; then
        printf 'the first bytes' > "$out"
        printf 'curl: (28) Operation too slow. Less than %s bytes/sec transferred the last %s seconds\n' "$speed_limit" "$speed_time" >&2
        status 000
        exit 28
      fi
      echo "the curl stand-in: with no --speed-limit and --speed-time this stalled download would never end" >&2
      exit 97
      ;;
  esac
fi
fails=''
[ ! -f "$state/curl-fails" ] || fails=$(/bin/cat "$state/curl-fails")
if [ -n "$fails" ]; then
  case "$url" in
    "$fails"*)
      echo 'curl: (22) The requested URL returned error: 503' >&2
      status 503
      exit 22
      ;;
  esac
fi
case "$url" in
  https://github.com/nledford/Ladderback/releases/latest/download/*)
    file="$state/release/${url##*/}"
    if [ -d "$state/release-http" ]; then
      if [ "$proto" = '=https' ]; then
        echo 'curl: (1) Protocol "http" disabled (in redirect)' >&2
        status 302
        exit 1
      fi
      file="$state/release-http/${url##*/}"
    fi
    if [ ! -f "$file" ]; then
      echo 'curl: (56) The requested URL returned error: 404' >&2
      status 404
      exit 56
    fi
    /bin/cp "$file" "$out" ;;
  https://github.com/trunk-rs/trunk/releases/download/*) /bin/cp "$state/trunk-served.tar.gz" "$out" ;;
  *) /bin/cp "$state/templates/rustup-init" "$out" ;;
esac || exit 1
status 200
"#;

/// The genuine trunk archive has the SHA-256 the script pins for the file's
/// name (`install/state/pinned/<name>`); any other file its real SHA-256.
const SHA256SUM: &str = r#"pinned="$state/pinned/${1##*/}"
if /usr/bin/cmp -s "$1" "$state/trunk-genuine.tar.gz" && [ -f "$pinned" ]; then
  printf '%s  %s\n' "$(/bin/cat "$pinned")" "$1"
elif [ -x /usr/bin/sha256sum ]; then
  /usr/bin/sha256sum "$1"
else
  /usr/bin/shasum -a 256 "$1"
fi
"#;

/// rustup's installer: installs the rustup, cargo and rustc stand-ins.
const RUSTUP_INIT: &str = r#"/bin/mkdir -p "$HOME/.cargo/bin"
for tool in rustup cargo rustc; do
  /bin/cp "$state/templates/$tool" "$HOME/.cargo/bin/$tool" || exit 1
done
"#;

/// What must never run: recorded, and refused.
const NEVER: &str = "exit 1\n";

/// The stand-ins for tools the build needs, by name.
const TOOLS: &[(&str, &str)] = &[
    ("cc", "exit 0\n"),
    ("node", NODE),
    ("npm", "echo 10.9.0\n"),
    ("rustup", RUSTUP),
    ("cargo", CARGO),
    ("rustc", RUSTC),
    ("just", JUST),
    ("trunk", TRUNK),
    ("uname", UNAME),
    ("getconf", GETCONF),
    ("curl", CURL),
    ("sha256sum", SHA256SUM),
    ("xcode-select", XCODE_SELECT),
    ("sudo", NEVER),
];

/// The package managers a system has: present, never to be run.
const PACKAGE_MANAGERS: &[&str] = &["brew", "apt-get", "dnf", "pacman"];

/// Real tools the script and its stand-ins need, linked into the stand-ins'
/// folder.
const REAL_TOOLS: &[&str] = &[
    "sh", "cat", "mkdir", "mktemp", "mv", "rm", "tar", "gzip", "install",
];

/// Real tools linked in where the test's system has them: GNU tar needs xz
/// to unpack a `.tar.xz`, which macOS's tar unpacks itself.
const REAL_TOOLS_IF_ANY: &[&str] = &["xz"];

/// Where the newest release's files are, for the `curl` stand-in to serve.
const RELEASE_URL: &str = "https://github.com/nledford/Ladderback/releases/";

fn home(agent: &Agent) -> PathBuf {
    agent.scratch().join("home")
}

fn bin(agent: &Agent) -> PathBuf {
    agent.scratch().join("install/bin")
}

fn state(agent: &Agent) -> PathBuf {
    agent.scratch().join("install/state")
}

fn upstream(agent: &Agent) -> PathBuf {
    agent.scratch().join("upstream")
}

/// Where the script keeps the source when nothing says otherwise.
fn source(agent: &Agent) -> PathBuf {
    home(agent).join(".local/share/ladderback/source")
}

/// The stand-in rustup's file for the Rust the release pins.
fn pinned_rust(agent: &Agent) -> PathBuf {
    state(agent).join(format!("toolchains/{PINNED}-{HOST}"))
}

/// What `rustup component list --installed` lists for the Rust the release
/// pins, with the components and targets its `rust-toolchain.toml` names,
/// leaving out the line `without` names, if any.
fn pinned_rust_components(without: Option<&str>) -> String {
    ["cargo", "clippy", "rust-std", "rustc", "rustfmt"]
        .map(|component| format!("{component}-{HOST}"))
        .into_iter()
        .chain(["rust-std-wasm32-unknown-unknown".to_owned()])
        .filter(|line| Some(line.as_str()) != without)
        .map(|line| line + "\n")
        .collect()
}

/// `{home}` in a step's text stands for the scenario's home folder.
fn expand(agent: &Agent, text: &str) -> String {
    text.replace("{home}", &home(agent).display().to_string())
}

/// `name`'s full path, from the test's own `PATH`.
fn real(name: &str) -> PathBuf {
    real_if_any(name).unwrap_or_else(|| panic!("{name} is on the PATH"))
}

fn real_if_any(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").expect("a PATH");
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Writes a runnable file. Written while no child starts: a child forked
/// meanwhile would hold the file open for writing, and running it would
/// fail with "text file busy" (decision 58).
fn write_runnable(path: &Path, text: &str) {
    spawn_gate::hold(|| {
        fs::write(path, text).expect("a stand-in is written");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("a stand-in is made runnable");
    });
}

fn stand_in(agent: &Agent, body: &str) -> String {
    let prelude = PRELUDE.replace("@STATE@", &state(agent).display().to_string());
    format!(
        "{prelude}{}",
        body.replace("@WASM_BINDGEN@", WASM_BINDGEN)
            .replace("@HOST@", HOST)
    )
}

fn set(agent: &Agent, name: &str, value: &str) {
    fs::write(state(agent).join(name), format!("{value}\n")).expect("a stand-in's setting");
}

fn remove_tool(agent: &Agent, name: &str) {
    fs::remove_file(bin(agent).join(name)).expect("the stand-in was there");
}

/// Every call since the scenario's last run began.
fn calls_since_run(agent: &Agent) -> Vec<String> {
    calls(agent)
        .into_iter()
        .skip(agent.install.calls_before)
        .collect()
}

fn calls(agent: &Agent) -> Vec<String> {
    fs::read_to_string(state(agent).join("calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Runs the real git, outside the stand-ins, with the scenario's home.
async fn git(agent: &Agent, dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new(real("git"));
    command
        .args(["-c", "user.name=Ladderback", "-c", "user.email="])
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(["-c", "tag.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", home(agent))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = blocking(move || {
        spawn_gate::hold(|| command.spawn())
            .expect("git runs")
            .wait_with_output()
            .expect("git ran")
    })
    .await;
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A release: a commit whose `Cargo.toml` holds the tag's version, with the
/// web UI's stylesheet tracked, as releases up to v0.2.0 have it, tagged.
async fn release(agent: &Agent, tag: &str) {
    let dir = upstream(agent);
    fs::create_dir_all(dir.join("crates/web")).expect("the web UI's folder");
    fs::write(dir.join("crates/web/tw-out.css"), RELEASED_STYLESHEET)
        .expect("the web UI's stylesheet");
    fs::write(dir.join("Justfile"), "install *args:\n").expect("the release's Justfile");
    let version = tag.trim_start_matches('v');
    fs::write(
        dir.join("Cargo.toml"),
        format!("[workspace.package]\nversion = \"{version}\"\nrust-version = \"{PINNED}\"\n"),
    )
    .expect("the release's Cargo.toml");
    fs::write(
        dir.join("rust-toolchain.toml"),
        format!(
            "[toolchain]\nchannel = \"{PINNED}\"\ncomponents = [\"rustfmt\", \"clippy\"]\ntargets = [\"wasm32-unknown-unknown\"]\n"
        ),
    )
    .expect("the release's rust-toolchain.toml");
    git(
        agent,
        &dir,
        &[
            "add",
            "Cargo.toml",
            "rust-toolchain.toml",
            "Justfile",
            "crates",
        ],
    )
    .await;
    git(agent, &dir, &["commit", "--quiet", "-m", tag]).await;
    git(agent, &dir, &["tag", tag]).await;
}

#[given(expr = "Ladderback has released {string}, {string} and {string}")]
async fn has_released(agent: &mut Agent, first: String, second: String, third: String) {
    let dir = upstream(agent);
    fs::create_dir_all(&dir).expect("the upstream folder");
    fs::create_dir_all(home(agent)).expect("the home folder");
    // The script's clone of Ladderback's address reaches the local copy.
    fs::write(
        home(agent).join(".gitconfig"),
        format!("[url \"file://{}\"]\n\tinsteadOf = {URL}\n", dir.display()),
    )
    .expect("the home folder's git configuration");
    git(agent, &dir, &["init", "--quiet"]).await;
    for tag in [first, second, third] {
        release(agent, &tag).await;
    }
}

#[given(expr = "Ladderback has since released {string}")]
async fn has_since_released(agent: &mut Agent, tag: String) {
    release(agent, &tag).await;
}

#[given(expr = "Ladderback has since released {string}, which pins no Rust")]
async fn has_since_released_unpinned(agent: &mut Agent, tag: String) {
    let dir = upstream(agent);
    git(agent, &dir, &["rm", "--quiet", "rust-toolchain.toml"]).await;
    git(agent, &dir, &["commit", "--quiet", "-m", &tag]).await;
    git(agent, &dir, &["tag", &tag]).await;
}

/// A release that no longer tracks the web UI's stylesheet and ignores it,
/// as releases after v0.2.0 do (#381). The web UI's folder keeps its other
/// files, so the checkout leaves the folder there for the build.
#[given(
    expr = "Ladderback has since released {string}, which leaves the web UI's stylesheet to its build"
)]
async fn has_since_released_untracked_stylesheet(agent: &mut Agent, tag: String) {
    let dir = upstream(agent);
    fs::write(dir.join(".gitignore"), "/crates/web/tw-out.css\n")
        .expect("the release's .gitignore");
    fs::write(dir.join("crates/web/index.html"), "<!doctype html>\n").expect("the web UI's page");
    git(
        agent,
        &dir,
        &["rm", "--cached", "--quiet", "crates/web/tw-out.css"],
    )
    .await;
    git(agent, &dir, &["add", ".gitignore", "crates/web/index.html"]).await;
    git(agent, &dir, &["commit", "--quiet", "-m", &tag]).await;
    git(agent, &dir, &["tag", &tag]).await;
}

#[given("Ladderback's only releases are release candidates")]
async fn only_release_candidates(agent: &mut Agent) {
    let dir = upstream(agent);
    let tags = git(agent, &dir, &["tag", "--list"]).await;
    for tag in tags.lines().filter(|tag| !tag.contains('-')) {
        git(agent, &dir, &["tag", "-d", tag]).await;
    }
}

#[given("every tool the build needs is installed")]
fn every_tool_installed(agent: &mut Agent) {
    let templates = state(agent).join("templates");
    fs::create_dir_all(bin(agent)).expect("the stand-ins' folder");
    fs::create_dir_all(&templates).expect("the stand-ins' templates");
    fs::create_dir_all(state(agent).join("sysroot/lib/rustlib/wasm32-unknown-unknown"))
        .expect("the stand-in Rust's WebAssembly target");
    for (name, body) in TOOLS {
        let text = stand_in(agent, body);
        write_runnable(&bin(agent).join(name), &text);
        write_runnable(&templates.join(name), &text);
    }
    write_runnable(
        &templates.join("rustup-init"),
        &stand_in(agent, RUSTUP_INIT),
    );
    write_runnable(
        &templates.join("install-ladderback"),
        &stand_in(agent, INSTALL_LADDERBACK),
    );
    let git = format!("exec '{}' \"$@\"\n", real("git").display());
    write_runnable(&bin(agent).join("git"), &stand_in(agent, &git));
    for name in REAL_TOOLS {
        std::os::unix::fs::symlink(real(name), bin(agent).join(name))
            .expect("a real tool is linked");
    }
    for name in REAL_TOOLS_IF_ANY {
        if let Some(path) = real_if_any(name) {
            std::os::unix::fs::symlink(path, bin(agent).join(name)).expect("a real tool is linked");
        }
    }
    // The oldest Node.js the script accepts, so every scenario that installs
    // shows that it is enough.
    set(agent, "node-version", "26.0.0");
    set(agent, "just-version", "1.43.0");
    set(agent, "rustc-version", "1.100.0");
    set(agent, "uname-s", "Linux");
    set(agent, "uname-m", "x86_64");
    set(agent, "glibc", "2.39");
    set(agent, "long-bit", "64");
    fs::create_dir_all(state(agent).join("toolchains")).expect("rustup's toolchains");
    fs::write(pinned_rust(agent), pinned_rust_components(None))
        .expect("rustup has the Rust the release pins");
}

#[given(
    regex = r"^the system is (macOS|macOS with Homebrew|Debian or Ubuntu|Fedora|Arch Linux|another Linux)$"
)]
fn system_is(agent: &mut Agent, system: String) {
    let (kernel, machine, manager) = match system.as_str() {
        "macOS" => ("Darwin", "arm64", None),
        "macOS with Homebrew" => ("Darwin", "arm64", Some("brew")),
        "Debian or Ubuntu" => ("Linux", "x86_64", Some("apt-get")),
        "Fedora" => ("Linux", "x86_64", Some("dnf")),
        "Arch Linux" => ("Linux", "x86_64", Some("pacman")),
        _ => ("Linux", "x86_64", None),
    };
    set(agent, "uname-s", kernel);
    set(agent, "uname-m", machine);
    if let Some(manager) = manager {
        write_runnable(&bin(agent).join(manager), &stand_in(agent, NEVER));
    }
}

#[given(expr = "the system is Linux on {string} with glibc {string}")]
fn linux_with_glibc(agent: &mut Agent, machine: String, glibc: String) {
    set(agent, "uname-s", "Linux");
    set(agent, "uname-m", &machine);
    set(agent, "glibc", &glibc);
}

#[given(regex = r#"^the system is (Linux|macOS) on "([^"]+)"$"#)]
fn system_on(agent: &mut Agent, os: String, machine: String) {
    set(
        agent,
        "uname-s",
        if os == "macOS" { "Darwin" } else { "Linux" },
    );
    set(agent, "uname-m", &machine);
}

/// A musl system's getconf knows no glibc.
#[given(expr = "the system is Linux on {string} with musl, not glibc")]
fn linux_with_musl(agent: &mut Agent, machine: String) {
    set(agent, "uname-s", "Linux");
    set(agent, "uname-m", &machine);
    fs::remove_file(state(agent).join("glibc")).expect("the stand-in's glibc");
}

/// 32-bit Raspberry Pi OS: a 64-bit kernel, whose `uname -m` says
/// `aarch64`, running 32-bit programs.
#[given(expr = "the system is Linux on {string}, running 32-bit programs")]
fn linux_running_32_bit(agent: &mut Agent, machine: String) {
    set(agent, "uname-s", "Linux");
    set(agent, "uname-m", &machine);
    set(agent, "long-bit", "32");
}

#[given(expr = "the system is Linux on {string} with {int} processors and {int} GB of free memory")]
fn linux_with_memory(agent: &mut Agent, machine: String, processors: u32, gigabytes: u64) {
    set(agent, "uname-s", "Linux");
    set(agent, "uname-m", &machine);
    set(agent, "processors", &processors.to_string());
    set(
        agent,
        "free-pages",
        &(gigabytes * 1024 * 1024 * 1024 / 4096).to_string(),
    );
}

#[given("there is no C compiler")]
fn no_c_compiler(agent: &mut Agent) {
    remove_tool(agent, "cc");
}

#[given("there is no git")]
fn no_git(agent: &mut Agent) {
    remove_tool(agent, "git");
}

#[given("there is no curl")]
fn no_curl(agent: &mut Agent) {
    remove_tool(agent, "curl");
}

#[given("there is no Node.js")]
fn no_node(agent: &mut Agent) {
    remove_tool(agent, "node");
    remove_tool(agent, "npm");
}

#[given(expr = "Node.js is version {string}")]
fn node_is(agent: &mut Agent, version: String) {
    set(agent, "node-version", &version);
}

#[when(expr = "the owner replaces Node.js with version {string}")]
fn node_replaced(agent: &mut Agent, version: String) {
    set(agent, "node-version", &version);
}

#[given(expr = "just is version {string}")]
fn just_is(agent: &mut Agent, version: String) {
    set(agent, "just-version", &version);
}

#[given("just and trunk aren't installed")]
async fn no_just_or_trunk(agent: &mut Agent) {
    remove_tool(agent, "just");
    trunk_removed(agent).await;
}

#[given(regex = r"^(just|trunk) isn't installed$")]
async fn tool_not_installed(agent: &mut Agent, tool: String) {
    if tool == "trunk" {
        trunk_removed(agent).await;
    } else {
        remove_tool(agent, &tool);
    }
}

/// Removes trunk, and readies trunk's release as the `curl` stand-in serves
/// it: a tarball holding a trunk stand-in, whose SHA-256 is the one the
/// script pins for each archive's name.
async fn trunk_removed(agent: &Agent) {
    remove_tool(agent, "trunk");
    let archive = state(agent).join("trunk-genuine.tar.gz");
    let trunk = stand_in(agent, RELEASED_TRUNK);
    trunk_archive(agent, "trunk-release", &trunk, &archive).await;
    fs::copy(&archive, state(agent).join("trunk-served.tar.gz")).expect("the served archive");
    let pinned = state(agent).join("pinned");
    fs::create_dir_all(&pinned).expect("the pinned SHA-256s' folder");
    for (target, sha) in trunk_pins().1 {
        fs::write(pinned.join(format!("trunk-{target}.tar.gz")), sha).expect("a pinned SHA-256");
    }
}

/// Writes `archive`, a gzipped tarball holding `trunk` alone, as trunk's
/// release archives do, with `text` in it; `folder` is the scratch folder,
/// under the state folder, it is packed from.
async fn trunk_archive(agent: &Agent, folder: &str, text: &str, archive: &Path) {
    let folder = state(agent).join(folder);
    fs::create_dir_all(&folder).expect("the archive's folder");
    write_runnable(&folder.join("trunk"), text);
    pack(archive, "-czf", &folder, "trunk").await;
}

/// Packs `member` of `folder` into `archive` with the real tar, `create`
/// saying how (`-czf`, `-cJf`).
async fn pack(archive: &Path, create: &str, folder: &Path, member: &str) {
    let mut command = Command::new(real("tar"));
    command
        .arg(create)
        .arg(archive)
        .arg("-C")
        .arg(folder)
        .arg(member)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = blocking(move || {
        spawn_gate::hold(|| command.spawn())
            .expect("tar runs")
            .wait_with_output()
            .expect("tar ran")
    })
    .await;
    assert!(
        output.status.success(),
        "tar: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The trunk version the script pins (`TRUNK_VERSION=<version>`), and the
/// SHA-256 it pins for each target's archive (`trunk_target=<target>
/// trunk_sha=<sha>` on one line).
fn trunk_pins() -> (Option<String>, Vec<(String, String)>) {
    let script = fs::read_to_string(SCRIPT).expect("the install script");
    let mut version = None;
    let mut pins = Vec::new();
    for line in script.lines() {
        let (mut target, mut sha) = (None, None);
        for word in line.split_whitespace() {
            if let Some(value) = word.strip_prefix("TRUNK_VERSION=") {
                version = Some(value.to_owned());
            } else if let Some(value) = word.strip_prefix("trunk_target=") {
                target = Some(value);
            } else if let Some(value) = word.strip_prefix("trunk_sha=") {
                sha = Some(value);
            }
        }
        if let (Some(target), Some(sha)) = (target, sha)
            && sha.len() == 64
            && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            pins.push((target.to_owned(), sha.to_owned()));
        }
    }
    (version, pins)
}

/// The archive served instead is a well-formed one, with another trunk in
/// it, so only the SHA-256 tells it apart.
#[given("trunk's release has been replaced by another archive with another trunk in it")]
async fn trunk_replaced(agent: &mut Agent) {
    let other = stand_in(agent, "echo 'trunk (not from its release)'\n");
    let served = state(agent).join("trunk-served.tar.gz");
    trunk_archive(agent, "trunk-other", &other, &served).await;
}

#[given("trunk's release can't be downloaded")]
fn trunk_unavailable(agent: &mut Agent) {
    fs::remove_file(state(agent).join("trunk-served.tar.gz")).expect("the served archive");
}

/// The `curl` stand-in stalls on every URL that starts with this one.
#[given(
    regex = r"^the download of (trunk's release|rustup's installer|the release's archive) stalls$"
)]
fn download_stalls(agent: &mut Agent, download: String) {
    let url = match download.as_str() {
        "trunk's release" => "https://github.com/trunk-rs/trunk/releases/".to_owned(),
        "rustup's installer" => "https://sh.rustup.rs".to_owned(),
        _ => format!("{RELEASE_URL}latest/download/ladderback-"),
    };
    set(agent, "curl-stalls", &url);
}

/// The `curl` stand-in gets a server error for every URL that starts with
/// this one.
#[given("the download of the release's archive fails with a server error")]
fn release_download_fails(agent: &mut Agent) {
    set(
        agent,
        "curl-fails",
        &format!("{RELEASE_URL}latest/download/ladderback-"),
    );
}

/// The targets each release publishes an archive for.
const ARCHIVE_TARGETS: [&str; 2] = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"];

/// A release with an archive for each Linux target, as the release attaches
/// them: `ladderback-<target>.tar.xz`, holding `ladderback-<target>/ladderback`,
/// a stand-in that reports the tag's version, and `SHA256SUMS`, their
/// checksums as `sha256sum` writes them, for the `curl` stand-in to serve.
#[given(
    expr = "Ladderback has since released {string}, with archives for Linux and their checksums"
)]
async fn has_since_released_with_archives(agent: &mut Agent, tag: String) {
    release(agent, &tag).await;
    let version = tag.trim_start_matches('v');
    let mut sums = String::new();
    for target in ARCHIVE_TARGETS {
        let binary = stand_in(
            agent,
            &format!("# The release's binary for {target}.\necho 'ladderback {version}'\n"),
        );
        let archive = release_archive(agent, "release-build", "release", target, &binary).await;
        let digest = Sha256::digest(fs::read(&archive).expect("the release's archive"));
        writeln!(sums, "{digest:x}  ladderback-{target}.tar.xz").expect("a line of SHA256SUMS");
    }
    fs::write(state(agent).join("release/SHA256SUMS"), sums).expect("the release's SHA256SUMS");
}

/// The owner's install from the release's archive, on the stand-ins' x86_64
/// Linux, checked as it is in a scenario of its own.
#[given(expr = "the owner has installed {string} from the release's archive")]
async fn has_installed_archive(agent: &mut Agent, tag: String) {
    has_since_released_with_archives(agent, tag).await;
    run_script(agent, &[]).await;
    let ran = ran(agent);
    assert_eq!(ran.code, Some(0), "the first run: {}", ran.stderr);
    release_archive_installed(
        agent,
        "x86_64-unknown-linux-gnu".to_owned(),
        "{home}/.cargo/bin".to_owned(),
    );
}

/// A release published before its archives are attached: the `curl`
/// stand-in has none of its files to serve, nor an earlier release's, as
/// `releases/latest` names the new one.
#[given(expr = "Ladderback has since released {string}, whose archives aren't attached yet")]
async fn has_since_released_without_archives(agent: &mut Agent, tag: String) {
    release(agent, &tag).await;
    match fs::remove_dir_all(state(agent).join("release")) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            panic!("an earlier release's files: {error}")
        }
        _ => {}
    }
}

/// A release part way through attaching its files: SHA256SUMS is there, the
/// archive for `target` isn't yet.
#[given(
    expr = "Ladderback has since released {string}, with its SHA256SUMS attached but not the archive for {string}"
)]
async fn has_since_released_without_archive(agent: &mut Agent, tag: String, target: String) {
    has_since_released_with_archives(agent, tag).await;
    fs::remove_file(state(agent).join(format!("release/ladderback-{target}.tar.xz")))
        .expect("the archive is taken away");
}

/// The archive served instead is a well-formed one, with another binary in
/// it, so only the checksum tells it apart.
#[given(
    expr = "the release's archive for {string} has been replaced by another with another binary in it"
)]
async fn release_archive_replaced(agent: &mut Agent, target: String) {
    let binary = stand_in(agent, "echo 'ladderback (not the release's)'\n");
    release_archive(agent, "release-other", "release", &target, &binary).await;
}

#[given(expr = "the release's SHA256SUMS lists no checksum for {string}")]
fn sums_without(agent: &mut Agent, target: String) {
    let file = state(agent).join("release/SHA256SUMS");
    let sums = fs::read_to_string(&file).expect("the release's SHA256SUMS");
    let archive = format!("ladderback-{target}.tar.xz");
    let kept: String = sums
        .lines()
        .filter(|line| !line.ends_with(&archive))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_ne!(kept, sums, "SHA256SUMS listed no {archive}");
    fs::write(&file, kept).expect("the release's SHA256SUMS");
}

/// A redirect that downgrades the release's downloads to plain HTTP, where
/// anyone on the way can swap the archive and SHA256SUMS together: the
/// HTTP side serves another binary's archive with a checksum that matches
/// it.
#[given(
    "the release's downloads are redirected to plain HTTP, which serves another archive and its checksum"
)]
async fn release_redirected_to_http(agent: &mut Agent) {
    let target = "x86_64-unknown-linux-gnu";
    let binary = stand_in(agent, "echo 'ladderback (not the release's)'\n");
    let archive = release_archive(agent, "release-other", "release-http", target, &binary).await;
    let digest = Sha256::digest(fs::read(&archive).expect("the other archive"));
    fs::write(
        state(agent).join("release-http/SHA256SUMS"),
        format!("{digest:x}  ladderback-{target}.tar.xz\n"),
    )
    .expect("the other SHA256SUMS");
}

/// Writes the release's archive for `target`, with `binary` in it, packed
/// from `folder` under the state folder into `served` there, where the
/// `curl` stand-in serves the release from, and returns its path.
async fn release_archive(
    agent: &Agent,
    folder: &str,
    served: &str,
    target: &str,
    binary: &str,
) -> PathBuf {
    let name = format!("ladderback-{target}");
    let folder = state(agent).join(folder);
    fs::create_dir_all(folder.join(&name)).expect("the archive's folder");
    write_runnable(&folder.join(&name).join("ladderback"), binary);
    let served = state(agent).join(served);
    fs::create_dir_all(&served).expect("the release's folder");
    let archive = served.join(format!("{name}.tar.xz"));
    pack(&archive, "-cJf", &folder, &name).await;
    archive
}

#[given("none of the build's tools is installed")]
fn no_build_tools(agent: &mut Agent) {
    for tool in [
        "cc", "git", "node", "npm", "rustup", "cargo", "rustc", "just", "trunk",
    ] {
        remove_tool(agent, tool);
    }
}

#[given("Rust isn't installed")]
fn no_rust(agent: &mut Agent) {
    for tool in ["rustup", "cargo", "rustc"] {
        remove_tool(agent, tool);
    }
    fs::remove_dir_all(state(agent).join("toolchains")).expect("rustup's toolchains");
}

#[given("rustup doesn't have the Rust the release pins")]
fn no_pinned_rust(agent: &mut Agent) {
    fs::remove_file(pinned_rust(agent)).expect("rustup had the Rust the release pins");
}

#[given(regex = r"^rustup has the Rust the release pins without (the WebAssembly target|Clippy)$")]
fn pinned_rust_without(agent: &mut Agent, part: String) {
    let line = if part == "Clippy" {
        format!("clippy-{HOST}")
    } else {
        "rust-std-wasm32-unknown-unknown".to_owned()
    };
    fs::write(pinned_rust(agent), pinned_rust_components(Some(&line)))
        .expect("rustup has the Rust the release pins");
}

#[given(expr = "the owner's environment sets {word} to {string}")]
fn environment_sets(agent: &mut Agent, name: String, value: String) {
    agent.install.env.push((name, value));
}

#[given(expr = "Rust {string} is installed without rustup")]
fn rust_without_rustup(agent: &mut Agent, version: String) {
    remove_tool(agent, "rustup");
    set(agent, "rustc-version", &version);
}

#[given(expr = "Rust {string} is installed without rustup or the WebAssembly target")]
fn rust_without_wasm(agent: &mut Agent, version: String) {
    rust_without_rustup(agent, version);
    fs::remove_dir(state(agent).join("sysroot/lib/rustlib/wasm32-unknown-unknown"))
        .expect("the target was there");
}

#[given("rustup's Rust is installed in the home folder, behind the system's own Rust on the PATH")]
fn rustup_behind_system_rust(agent: &mut Agent) {
    let cargo_bin = home(agent).join(".cargo/bin");
    fs::create_dir_all(&cargo_bin).expect("rustup's folder");
    for tool in ["rustup", "cargo", "rustc"] {
        let text = fs::read_to_string(state(agent).join("templates").join(tool))
            .expect("the stand-in's template");
        write_runnable(&cargo_bin.join(tool), &text);
    }
    remove_tool(agent, "rustup");
    for tool in ["cargo", "rustc"] {
        write_runnable(&bin(agent).join(tool), &stand_in(agent, SYSTEM_RUST));
    }
}

#[given("Apple's Command Line Tools aren't installed")]
fn no_command_line_tools(agent: &mut Agent) {
    write_runnable(
        &bin(agent).join("xcode-select"),
        &stand_in(agent, NO_COMMAND_LINE_TOOLS),
    );
}

#[given(expr = "the owner will answer {string} at the terminal")]
fn will_answer(agent: &mut Agent, answer: String) {
    let terminal = state(agent).join("terminal");
    fs::write(&terminal, format!("{answer}\n")).expect("the terminal's answer");
    terminal_is(agent, terminal);
}

#[given("there is no terminal")]
fn no_terminal(agent: &mut Agent) {
    let terminal = state(agent).join("no-terminal");
    terminal_is(agent, terminal);
}

fn terminal_is(agent: &mut Agent, terminal: PathBuf) {
    agent
        .install
        .env
        .retain(|(name, _)| name != "LADDERBACK_INSTALL_TTY");
    agent.install.env.push((
        "LADDERBACK_INSTALL_TTY".to_owned(),
        terminal.display().to_string(),
    ));
}

#[given(expr = "XDG_DATA_HOME is {string}")]
fn xdg_data_home(agent: &mut Agent, folder: String) {
    let folder = expand(agent, &folder);
    agent.install.env.push(("XDG_DATA_HOME".to_owned(), folder));
}

#[given(expr = "the folder {string} holds a file {string}")]
fn folder_holds_file(agent: &mut Agent, folder: String, name: String) {
    let folder = PathBuf::from(expand(agent, &folder));
    fs::create_dir_all(&folder).expect("the folder");
    let file = folder.join(name);
    let text = "Notes of the owner's own.\n".to_owned();
    fs::write(&file, &text).expect("the owner's file");
    agent.install.notes = Some((file, text));
}

#[given(expr = "the folder {string} is a clone of another project")]
async fn clone_of_another_project(agent: &mut Agent, folder: String) {
    let folder = expand(agent, &folder);
    let other = agent.scratch().join("other-project");
    fs::create_dir_all(&other).expect("the other project's folder");
    fs::write(other.join("README.md"), "Another project.\n").expect("its file");
    git(agent, &other, &["init", "--quiet"]).await;
    git(agent, &other, &["add", "README.md"]).await;
    git(agent, &other, &["commit", "--quiet", "-m", "start"]).await;
    let other = other.display().to_string();
    git(agent, &home(agent), &["clone", "--quiet", &other, &folder]).await;
}

#[given("the owner has run the install script, and its build rewrote the web UI's stylesheet")]
async fn has_run(agent: &mut Agent) {
    run_script(agent, &[]).await;
    let ran = ran(agent);
    assert_eq!(ran.code, Some(0), "the first run: {}", ran.stderr);
}

#[given("the owner's install failed after its build rewrote the web UI's stylesheet")]
async fn install_failed(agent: &mut Agent) {
    let fails = state(agent).join("build-fails");
    fs::write(&fails, "").expect("the build is set to fail");
    run_script(agent, &[]).await;
    let ran = ran(agent);
    assert_ne!(ran.code, Some(0), "the first run: {}", ran.stdout);
    assert!(
        ran.stderr.contains("the build failed"),
        "the first run: {}",
        ran.stderr
    );
    fs::remove_file(&fails).expect("the build is set to succeed");
}

#[given("a file in the source folder has been changed")]
fn source_changed(agent: &mut Agent) {
    let file = source(agent).join("Cargo.toml");
    let text = format!(
        "{}# The owner's change.\n",
        fs::read_to_string(&file).expect("the source's Cargo.toml")
    );
    fs::write(&file, &text).expect("the change");
    agent.install.changed = Some((file, text));
}

#[when("the owner runs the install script")]
async fn runs(agent: &mut Agent) {
    run_script(agent, &[]).await;
}

#[when(expr = "the owner runs the install script with {string}")]
async fn runs_with(agent: &mut Agent, args: String) {
    let args = expand(agent, &args);
    let args: Vec<&str> = args.split_whitespace().collect();
    run_script(agent, &args).await;
}

/// Runs the script as `curl … | bash -s -- <args>` runs it: bash reads it
/// from its standard input.
async fn run_script(agent: &mut Agent, args: &[&str]) {
    let work = agent.scratch().join("work");
    fs::create_dir_all(&work).expect("the working folder");
    agent.install.calls_before = calls(agent).len();
    let script = File::open(SCRIPT).expect("the install script");
    let mut command = Command::new(real("bash"));
    command
        .args(["-s", "--"])
        .args(args)
        .current_dir(work)
        .env_clear()
        .env("HOME", home(agent))
        .env("PATH", bin(agent))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LADDERBACK_INSTALL_TTY", state(agent).join("no-terminal"));
    for (name, value) in &agent.install.env {
        command.env(name, value);
    }
    command
        .stdin(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = blocking(move || {
        spawn_gate::hold(|| command.spawn())
            .expect("bash runs")
            .wait_with_output()
            .expect("bash ran")
    })
    .await;
    agent.interop.ran = Some(Ran {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy_owned(output.stdout),
        stderr: String::from_utf8_lossy_owned(output.stderr),
    });
}

async fn holds_release(agent: &Agent, folder: &Path, tag: &str) {
    let head = git(agent, folder, &["rev-parse", "HEAD"]).await;
    let release = git(
        agent,
        &upstream(agent),
        &["rev-parse", &format!("{tag}^{{commit}}")],
    )
    .await;
    assert!(!release.is_empty(), "git named no commit for {tag}");
    assert_eq!(head, release, "the source folder isn't at {tag}");
}

#[then(expr = "the source folder holds the release {string}")]
async fn source_holds(agent: &mut Agent, tag: String) {
    holds_release(agent, &source(agent), &tag).await;
}

#[then(expr = "the folder {string} holds the release {string}")]
async fn folder_holds_release(agent: &mut Agent, folder: String, tag: String) {
    let folder = PathBuf::from(expand(agent, &folder));
    holds_release(agent, &folder, &tag).await;
}

fn installs(agent: &Agent) -> Vec<String> {
    calls_since_run(agent)
        .into_iter()
        .filter(|call| call == "just install" || call.starts_with("just install "))
        .collect()
}

/// The owner's options come after `--force`, which every build from source
/// passes: cargo won't overwrite a binary it didn't install, such as one an
/// earlier install took from the release's archive.
#[then(expr = "the release was installed with {string}")]
fn installed_with(agent: &mut Agent, args: String) {
    let expected = format!("just install --force {}", expand(agent, &args));
    assert_eq!(installs(agent), [expected], "{:?}", calls_since_run(agent));
}

#[then("the release was installed")]
fn installed(agent: &mut Agent) {
    assert_eq!(installs(agent).len(), 1, "{:?}", calls_since_run(agent));
}

#[then("no release was installed")]
fn not_installed(agent: &mut Agent) {
    assert!(installs(agent).is_empty(), "{:?}", calls_since_run(agent));
}

#[then("nothing ran sudo or a package manager")]
fn no_sudo(agent: &mut Agent) {
    let ran: Vec<String> = calls(agent)
        .into_iter()
        .filter(|call| {
            let name = call.split(' ').next().unwrap_or_default();
            name == "sudo" || PACKAGE_MANAGERS.contains(&name)
        })
        .collect();
    assert!(ran.is_empty(), "these ran: {ran:?}");
}

/// Looking for the newest release's archive, and finding none, fetches
/// nothing: the scratch folder it downloads into is deleted.
#[then("nothing was installed, fetched or built")]
fn nothing_done(agent: &mut Agent) {
    nothing_installed_or_built(agent, true);
}

/// Downloading trunk's release installs nothing until it has been checked,
/// nor the release's archive.
#[then("nothing was installed or built")]
fn nothing_installed(agent: &mut Agent) {
    nothing_installed_or_built(agent, false);
}

fn nothing_installed_or_built(agent: &Agent, nor_fetched: bool) {
    let done: Vec<String> = calls_since_run(agent)
        .into_iter()
        .filter(|call| {
            let mut words = call.split(' ');
            let name = words.next().unwrap_or_default();
            let changes_git =
                name == "git" && words.any(|word| ["clone", "fetch", "checkout"].contains(&word));
            changes_git
                || name == "cargo"
                || (nor_fetched && name == "curl" && !call.contains(RELEASE_URL))
                || call.starts_with("rustup toolchain install")
                || name.starts_with("rustup-init")
                || call.starts_with("just install")
        })
        .collect();
    assert!(done.is_empty(), "these ran: {done:?}");
    assert!(!source(agent).exists(), "the source folder was made");
    assert!(
        !home(agent).join(".cargo").exists(),
        "something was installed into ~/.cargo"
    );
}

#[then(
    expr = "trunk's release for {string} was installed, checked against the checksum the script pins"
)]
fn trunk_release_installed(agent: &mut Agent, target: String) {
    let (version, pins) = trunk_pins();
    let version = version.expect("the script pins a trunk version");
    assert!(
        pins.iter().any(|(pinned, _)| *pinned == target),
        "the script pins no SHA-256 for trunk's {target} archive: {pins:?}"
    );
    let url = format!(
        "https://github.com/trunk-rs/trunk/releases/download/v{version}/trunk-{target}.tar.gz"
    );
    let calls = calls_since_run(agent);
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("curl ") && call.split(' ').any(|word| word == url)),
        "{url} wasn't downloaded: {calls:?}"
    );
    let installed = home(agent).join(".cargo/bin/trunk");
    let text = fs::read_to_string(&installed).expect("trunk is in ~/.cargo/bin");
    assert_eq!(
        text,
        stand_in(agent, RELEASED_TRUNK),
        "~/.cargo/bin/trunk isn't the one in trunk's release"
    );
    let mode = fs::metadata(&installed)
        .expect("trunk is in ~/.cargo/bin")
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "trunk isn't runnable: {mode:o}");
}

#[then(regex = r"^(just|trunk) wasn't built$")]
fn not_built(agent: &mut Agent, tool: String) {
    let built: Vec<String> = calls_since_run(agent)
        .into_iter()
        .filter(|call| call.starts_with("cargo ") && call.split(' ').any(|word| word == tool))
        .collect();
    assert!(built.is_empty(), "{tool} was built: {built:?}");
}

#[then(expr = "the release's archive for {string} was installed into {string}")]
fn release_archive_installed(agent: &mut Agent, target: String, folder: String) {
    let calls = calls_since_run(agent);
    for file in [
        "SHA256SUMS".to_owned(),
        format!("ladderback-{target}.tar.xz"),
    ] {
        let url = format!("{RELEASE_URL}latest/download/{file}");
        assert!(
            calls
                .iter()
                .any(|call| call.starts_with("curl ") && call.split(' ').any(|word| word == url)),
            "{url} wasn't downloaded: {calls:?}"
        );
    }
    let installed = PathBuf::from(expand(agent, &folder)).join("ladderback");
    let text = fs::read_to_string(&installed)
        .unwrap_or_else(|error| panic!("{} can't be read: {error}", installed.display()));
    let released = state(agent).join(format!("release-build/ladderback-{target}/ladderback"));
    assert_eq!(
        text,
        fs::read_to_string(released).expect("the release's binary"),
        "{} isn't the release's binary for {target}",
        installed.display()
    );
    let mode = fs::metadata(&installed)
        .expect("the installed binary")
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "the binary isn't runnable: {mode:o}");
}

/// Nothing that builds or fetches the source ran.
#[then("nothing was built")]
fn nothing_built(agent: &mut Agent) {
    let built: Vec<String> = calls_since_run(agent)
        .into_iter()
        .filter(|call| {
            let name = call.split(' ').next().unwrap_or_default();
            ["cargo", "just", "git", "rustup", "trunk"].contains(&name)
                || name.starts_with("rustup-init")
        })
        .collect();
    assert!(built.is_empty(), "these ran: {built:?}");
}

#[then("nothing was downloaded from Ladderback's releases")]
fn release_not_downloaded(agent: &mut Agent) {
    let downloads: Vec<String> = calls_since_run(agent)
        .into_iter()
        .filter(|call| call.starts_with("curl ") && call.contains(RELEASE_URL))
        .collect();
    assert!(downloads.is_empty(), "{downloads:?}");
}

#[then("trunk's release wasn't downloaded")]
fn trunk_not_downloaded(agent: &mut Agent) {
    let downloads: Vec<String> = calls_since_run(agent)
        .into_iter()
        .filter(|call| call.starts_with("curl ") && call.contains("/trunk-rs/"))
        .collect();
    assert!(downloads.is_empty(), "{downloads:?}");
}

/// Every `curl` call gives up on a connection that takes over 30 s and on a
/// transfer under 1 KiB/s for 60 s, and sets no limit on its whole time
/// (`--max-time`), which would cut off a slow link that still moves.
#[then(
    "every download gives up after 30 s without a connection or 60 s under 1 KiB/s, with no limit on its whole time"
)]
fn downloads_bounded(agent: &mut Agent) {
    let downloads: Vec<String> = calls_since_run(agent)
        .into_iter()
        .filter(|call| call.starts_with("curl "))
        .collect();
    for url in [
        "https://github.com/trunk-rs/",
        "https://sh.rustup.rs",
        RELEASE_URL,
    ] {
        assert!(
            downloads.iter().any(|call| call.contains(url)),
            "nothing was downloaded from {url}: {downloads:?}"
        );
    }
    for call in &downloads {
        let words: Vec<&str> = call.split(' ').collect();
        for bound in [
            ["--connect-timeout", "30"],
            ["--speed-limit", "1024"],
            ["--speed-time", "60"],
        ] {
            assert!(
                words.windows(2).any(|pair| pair == bound),
                "{call} lacks {}",
                bound.join(" ")
            );
        }
        assert!(
            !words
                .iter()
                .any(|word| word.starts_with("-m") || word.starts_with("--max-time")),
            "{call} limits the download's whole time"
        );
    }
}

#[then("the source was updated, not fetched again in full")]
fn updated_not_cloned(agent: &mut Agent) {
    let git: Vec<String> = calls_since_run(agent)
        .into_iter()
        .filter(|call| call.starts_with("git "))
        .collect();
    assert!(
        git.iter()
            .any(|call| call.split(' ').any(|word| word == "fetch")),
        "no fetch: {git:?}"
    );
    assert!(
        !git.iter()
            .any(|call| call.split(' ').any(|word| word == "clone")),
        "cloned again: {git:?}"
    );
}

/// `{trunk version}` in the command stands for the trunk version the script
/// pins.
#[then(expr = "the tools were installed with {string}")]
fn tools_installed_with(agent: &mut Agent, command: String) {
    let command = match trunk_pins().0 {
        Some(version) => command.replace("{trunk version}", &version),
        None => command,
    };
    let calls = calls_since_run(agent);
    assert!(calls.contains(&command), "{command}: {calls:?}");
}

/// The calls of rustup's installer, which the script saves and runs with sh.
fn rustup_installs(agent: &Agent) -> Vec<String> {
    calls_since_run(agent)
        .into_iter()
        .filter(|call| call.starts_with("rustup-init"))
        .collect()
}

#[then(expr = "rustup's installer ran with {string}")]
fn rustup_ran_with(agent: &mut Agent, args: String) {
    let ran = rustup_installs(agent);
    assert_eq!(ran.len(), 1, "{:?}", calls_since_run(agent));
    let given: Vec<&str> = ran[0].split(' ').skip(1).collect();
    assert_eq!(given, [args.as_str()]);
}

#[then("rustup's installer ran, asking its own questions")]
fn rustup_asked(agent: &mut Agent) {
    let ran = rustup_installs(agent);
    assert_eq!(ran.len(), 1, "{:?}", calls_since_run(agent));
    assert_eq!(
        ran[0].split(' ').count(),
        1,
        "it was told not to ask: {ran:?}"
    );
}

#[then("wasm-bindgen was built before the release was installed")]
fn wasm_bindgen_built(agent: &mut Agent) {
    let calls = calls_since_run(agent);
    let build = format!("cargo install --locked wasm-bindgen-cli --version {WASM_BINDGEN}");
    let built = calls.iter().position(|call| *call == build);
    let installed = calls
        .iter()
        .position(|call| call.starts_with("just install"));
    assert_matches!((built, installed), (Some(b), Some(i)) if b < i, "{calls:?}");
}

#[then("wasm-bindgen was not built")]
fn wasm_bindgen_not_built(agent: &mut Agent) {
    let calls = calls_since_run(agent);
    assert!(
        !calls.iter().any(|call| call.contains("wasm-bindgen-cli")),
        "{calls:?}"
    );
    assert_eq!(installs(agent).len(), 1, "{calls:?}");
}

#[then("the system's own Rust wasn't used")]
fn system_rust_unused(agent: &mut Agent) {
    let ran = fs::read_to_string(state(agent).join("system-rust")).unwrap_or_default();
    assert!(ran.is_empty(), "these ran: {ran}");
}

#[then(expr = "the folder {string} holds only {string}, unchanged")]
fn folder_unchanged(agent: &mut Agent, folder: String, name: String) {
    let folder = PathBuf::from(expand(agent, &folder));
    let names: Vec<String> = fs::read_dir(&folder)
        .expect("the folder is still there")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, [name]);
    let (file, text) = agent.install.notes.as_ref().expect("the owner's file");
    assert_eq!(&fs::read_to_string(file).expect("the owner's file"), text);
}

#[then("the change in the source folder is still there")]
fn change_kept(agent: &mut Agent) {
    let (file, text) = agent.install.changed.as_ref().expect("a change");
    assert_eq!(&fs::read_to_string(file).expect("the changed file"), text);
}

/// Nothing git sees has changed: no tracked file, and no file it doesn't
/// ignore.
#[then("the source folder has no changes")]
async fn source_clean(agent: &mut Agent) {
    let changes = git(agent, &source(agent), &["status", "--porcelain"]).await;
    assert!(
        changes.is_empty(),
        "the source folder has changes: {changes}"
    );
}

fn stylesheet(agent: &Agent) -> String {
    fs::read_to_string(source(agent).join("crates/web/tw-out.css"))
        .expect("the web UI's stylesheet")
}

#[then("the web UI's stylesheet in the source folder is the release's")]
fn stylesheet_is_release(agent: &mut Agent) {
    assert_eq!(stylesheet(agent), RELEASED_STYLESHEET);
}

/// The build stand-in appends its line to the stylesheet; the release's
/// checkout removed the one the earlier release tracked.
#[then("the web UI's stylesheet in the source folder is the one its build wrote")]
fn stylesheet_is_built(agent: &mut Agent) {
    assert_eq!(stylesheet(agent), "/* rebuilt */\n");
}

/// The Rust the last cargo call whose arguments start with `call` built
/// with, as the cargo stand-in recorded it.
fn built_with(agent: &Agent, call: &str) -> String {
    let builds = fs::read_to_string(state(agent).join("builds")).unwrap_or_default();
    builds
        .lines()
        .rev()
        .find_map(|line| {
            let (rust, args) = line.split_once(' ')?;
            args.starts_with(call).then(|| rust.to_owned())
        })
        .unwrap_or_else(|| panic!("no cargo call {call:?}: {builds}"))
}

#[then(expr = "the release was built with the Rust it pins, {string}")]
fn release_built_with_pin(agent: &mut Agent, rust: String) {
    assert_eq!(built_with(agent, "install --locked --path ."), rust);
}

#[then(expr = "wasm-bindgen was built with the Rust the release pins, {string}")]
fn wasm_bindgen_built_with_pin(agent: &mut Agent, rust: String) {
    assert_eq!(built_with(agent, "install --locked wasm-bindgen-cli"), rust);
}

/// Where in `calls` rustup was told to install the Rust the release pins.
fn pinned_rust_installs(calls: &[String]) -> Vec<usize> {
    calls
        .iter()
        .enumerate()
        .filter(|(_, call)| {
            call.split(' ')
                .take(4)
                .eq(["rustup", "toolchain", "install", PINNED])
        })
        .map(|(index, _)| index)
        .collect()
}

#[then(
    "the Rust the release pins was installed, with the components and targets it lists, before the release"
)]
fn pinned_rust_installed(agent: &mut Agent) {
    let calls = calls_since_run(agent);
    let installed = pinned_rust_installs(&calls).first().copied();
    let built = calls
        .iter()
        .position(|call| call.starts_with("just install"));
    assert_matches!((installed, built), (Some(i), Some(b)) if i < b, "{calls:?}");
    let has = fs::read_to_string(pinned_rust(agent)).expect("rustup has the Rust the release pins");
    let wants = pinned_rust_components(None);
    let missing: Vec<&str> = wants
        .lines()
        .filter(|want| !has.lines().any(|line| line == *want))
        .collect();
    assert!(
        missing.is_empty(),
        "the Rust the release pins lacks {missing:?}"
    );
}

#[then("the Rust the release pins was installed by the first run only")]
fn pinned_rust_installed_once(agent: &mut Agent) {
    let calls = calls(agent);
    let installs = pinned_rust_installs(&calls);
    assert_matches!(
        installs.as_slice(),
        [first] if *first < agent.install.calls_before,
        "{calls:?}"
    );
}

#[then(expr = "the release was built with {string}, the Rust the owner's environment names")]
fn release_built_with_environment(agent: &mut Agent, rust: String) {
    assert_eq!(built_with(agent, "install --locked --path ."), rust);
}

#[then(expr = "standard output doesn't say {string}")]
fn stdout_doesnt_say(agent: &mut Agent, text: String) {
    let stdout = &ran(agent).stdout;
    assert!(
        !stdout.contains(&text),
        "standard output says {text:?}: {stdout}"
    );
}
