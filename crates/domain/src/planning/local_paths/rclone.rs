//! What rclone reads from this machine's disk: the path an `rclone:`
//! location hands it, the settings of a connection string, and the flags of
//! an `rclone-command`. rclone runs in the daemon's working directory too, so
//! each of these named by a relative path is found as one.
//!
//! The rules follow rclone 1.76: its reading of a path (`fs/fspath`
//! `Parse`), the connection strings of <https://rclone.org/docs/>, the
//! global flags of <https://rclone.org/flags/>, the flags of
//! <https://rclone.org/commands/rclone_serve_restic/>, and the options of
//! each backend's page that name a file, a program or a location.

use std::borrow::Cow;

use super::{is_relative_program, starts_relative};
use crate::secrets::MAX_RCLONE_DEPTH;

/// What the value of a flag or a setting names on this machine.
#[derive(Clone, Copy)]
enum Names {
    /// A file or a folder.
    File,
    /// A file, with a leading `~` and environment variables expanded by
    /// rclone itself.
    ExpandedFile,
    /// A file to read a list from, or `-` for standard input.
    ListFile,
    /// A program: the first of the words the value splits into.
    Program,
    /// An rclone location: a remote, or a folder here.
    Location,
    /// The upstreams of a union: locations, each with an optional `:ro`,
    /// `:nc` or `:writeback` after it.
    UnionUpstreams,
    /// The upstreams of a combine: `dir=location` each.
    CombineUpstreams,
    /// A Kerberos credential cache: `FILE:path`, `DIR:path` or a path.
    CredentialCache,
    /// An address a server listens on: a socket when it is `unix://path`.
    Address,
}

/// rclone's global flags, without the `--`, that name something on this
/// machine. A connection string sets one as `override.NAME` or
/// `global.NAME`, with each `-` written `_`.
const GLOBAL_FLAGS: [(&str, Names); 36] = [
    ("config", Names::File),
    ("log-file", Names::File),
    ("ca-cert", Names::File),
    ("client-cert", Names::File),
    ("client-key", Names::File),
    ("cache-dir", Names::File),
    ("temp-dir", Names::File),
    ("cpuprofile", Names::File),
    ("memprofile", Names::File),
    ("exclude-from", Names::ListFile),
    ("include-from", Names::ListFile),
    ("filter-from", Names::ListFile),
    ("files-from", Names::ListFile),
    ("files-from-raw", Names::ListFile),
    ("files-from0", Names::ListFile),
    ("metadata-exclude-from", Names::ListFile),
    ("metadata-include-from", Names::ListFile),
    ("metadata-filter-from", Names::ListFile),
    ("password-command", Names::Program),
    ("metadata-mapper", Names::Program),
    ("backup-dir", Names::Location),
    ("compare-dest", Names::Location),
    ("copy-dest", Names::Location),
    ("rc-addr", Names::Address),
    ("rc-files", Names::File),
    ("rc-cert", Names::File),
    ("rc-key", Names::File),
    ("rc-client-ca", Names::File),
    ("rc-htpasswd", Names::File),
    ("rc-template", Names::File),
    ("metrics-addr", Names::Address),
    ("metrics-cert", Names::File),
    ("metrics-key", Names::File),
    ("metrics-client-ca", Names::File),
    ("metrics-htpasswd", Names::File),
    ("metrics-template", Names::File),
];

/// The flags of `rclone serve restic` that name something on this machine.
const SERVE_FLAGS: [(&str, Names); 5] = [
    ("addr", Names::Address),
    ("cert", Names::File),
    ("key", Names::File),
    ("client-ca", Names::File),
    ("htpasswd", Names::File),
];

/// The options of rclone's backends that name something on this machine:
/// the backend's flag prefix, and the option's name as a connection string
/// or the configuration file writes it. Its flag is `--PREFIX-NAME`, with
/// each `_` written `-`.
const BACKEND_OPTIONS: [(&str, &str, Names); 29] = [
    ("alias", "remote", Names::Location),
    ("archive", "remote", Names::Location),
    ("cache", "remote", Names::Location),
    ("chunker", "remote", Names::Location),
    ("compress", "remote", Names::Location),
    ("crypt", "remote", Names::Location),
    ("hasher", "remote", Names::Location),
    ("union", "upstreams", Names::UnionUpstreams),
    ("combine", "upstreams", Names::CombineUpstreams),
    ("azureblob", "client_certificate_path", Names::File),
    ("azureblob", "service_principal_file", Names::File),
    ("azurefiles", "client_certificate_path", Names::File),
    ("azurefiles", "service_principal_file", Names::File),
    ("box", "box_config_file", Names::ExpandedFile),
    ("cache", "chunk_path", Names::File),
    ("cache", "db_path", Names::File),
    ("cache", "tmp_upload_path", Names::File),
    ("drive", "service_account_file", Names::ExpandedFile),
    ("gcs", "service_account_file", Names::ExpandedFile),
    ("oos", "config_file", Names::ExpandedFile),
    ("oos", "sse_customer_key_file", Names::File),
    ("s3", "shared_credentials_file", Names::File),
    ("sftp", "key_file", Names::ExpandedFile),
    ("sftp", "pubkey_file", Names::ExpandedFile),
    ("sftp", "known_hosts_file", Names::ExpandedFile),
    ("sftp", "ssh", Names::Program),
    ("smb", "kerberos_ccache", Names::CredentialCache),
    ("webdav", "unix_socket", Names::File),
    ("webdav", "bearer_token_command", Names::Program),
];

/// Whether rclone, handed `path` as the location of a repository, names a
/// file or folder on this machine by a relative path: the path itself, a
/// setting of its connection string, or a location written in one. `whole`
/// says whether `path` is all of it, or only the text before a reference,
/// which may still decide what it is. A location written in a setting is
/// read as deep as a redactor reads one for credentials
/// ([`MAX_RCLONE_DEPTH`] levels, the location itself the first); one deeper
/// is found as relative, as what it names is not read.
pub(super) fn location_relative(path: &str, whole: bool) -> bool {
    reads_relative(path, whole, MAX_RCLONE_DEPTH)
}

/// Whether one of `args`, the arguments of an `rclone-command`, is a flag
/// that names a file or a folder on this machine, or a program, by a
/// relative path. A flag is `--NAME VALUE` or `--NAME=VALUE`. A `--` ends
/// nothing: pflag gives it to a flag before it that takes a value, and
/// which of rclone's flags take one isn't known here. rclone has no
/// one-letter form of any of these.
pub(super) fn flags_relative(args: &[String]) -> bool {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let Some(flag) = arg.strip_prefix("--") else {
            continue;
        };
        let (name, value) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (flag, None),
        };
        let Some(names) = named_by_flag(name) else {
            continue;
        };
        let Some(value) = value.or_else(|| args.next().map(String::as_str)) else {
            return false;
        };
        if names_relative(names, value, MAX_RCLONE_DEPTH) {
            return true;
        }
    }
    false
}

/// What the flag `--name` names, if it names something on this machine.
fn named_by_flag(name: &str) -> Option<Names> {
    GLOBAL_FLAGS
        .iter()
        .chain(&SERVE_FLAGS)
        .find_map(|(flag, names)| (*flag == name).then_some(*names))
        .or_else(|| {
            BACKEND_OPTIONS.iter().find_map(|(prefix, option, names)| {
                name.strip_prefix(prefix)
                    .and_then(|rest| rest.strip_prefix('-'))
                    .is_some_and(|rest| written_as(rest, option, '-'))
                    .then_some(*names)
            })
        })
}

/// Whether `written` is `name` with each `_` or `-` in it written as
/// `separator`.
fn written_as(written: &str, name: &str, separator: char) -> bool {
    written.len() == name.len()
        && written.chars().zip(name.chars()).all(|(w, n)| match n {
            '_' | '-' => w == separator,
            n => w == n,
        })
}

/// Whether the setting `name` of a connection string names something on
/// this machine by a relative path when it is `value`. `override.NAME` and
/// `global.NAME` set a global flag. Any other is an option of `backend`,
/// the backend an on-the-fly location names or a remote's `type` setting
/// says; a remote of rclone's configuration file without one may be of
/// any, so its setting is read as each backend with an option of that name
/// reads it.
fn setting_relative(backend: Option<&str>, name: &str, value: &str, depth: usize) -> bool {
    if let Some(global) = name
        .strip_prefix("override.")
        .or_else(|| name.strip_prefix("global."))
    {
        return GLOBAL_FLAGS.iter().any(|(flag, names)| {
            written_as(global, flag, '_') && names_relative(*names, value, depth)
        });
    }
    let options = || {
        BACKEND_OPTIONS
            .iter()
            .filter(|(_, option, _)| *option == name)
    };
    let own = options().any(|(prefix, _, _)| Some(*prefix) == backend);
    options()
        .filter(|(prefix, _, _)| !own || Some(*prefix) == backend)
        .any(|(_, _, names)| names_relative(*names, value, depth))
}

/// Whether `value`, what a flag or a setting is set to, names something on
/// this machine by a relative path, reading a location in it `depth` levels
/// deep at most.
fn names_relative(names: Names, value: &str, depth: usize) -> bool {
    match names {
        Names::File => file_relative(value),
        Names::ExpandedFile => !value.starts_with(['~', '$']) && file_relative(value),
        Names::ListFile => value != "-" && file_relative(value),
        Names::Program => words(value)
            .and_then(|words| words.into_iter().next())
            .is_some_and(|program| is_relative_program(&program)),
        Names::Location => reads_relative(value, true, depth),
        Names::UnionUpstreams => words(value).unwrap_or_default().iter().any(|upstream| {
            let location = [":ro", ":nc", ":writeback"]
                .iter()
                .find_map(|mode| upstream.strip_suffix(mode))
                .unwrap_or(upstream);
            reads_relative(location, true, depth)
        }),
        Names::CombineUpstreams => words(value).unwrap_or_default().iter().any(|upstream| {
            upstream
                .split_once('=')
                .is_some_and(|(_, location)| reads_relative(location, true, depth))
        }),
        Names::CredentialCache => file_relative(
            value
                .strip_prefix("FILE:")
                .or_else(|| value.strip_prefix("DIR:"))
                .unwrap_or(value),
        ),
        Names::Address => value.strip_prefix("unix://").is_some_and(file_relative),
    }
}

/// Whether `value`, a file's path, is set and relative.
fn file_relative(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('/')
}

/// The words of a space-separated list as rclone reads one, a record of
/// comma-separated values with a space for the comma: split at each space,
/// a word that starts with `"` quoted to the `"` that a space or the end
/// follows, where a `"` written twice inside stands for one. No words in an
/// empty list. `None` when rclone refuses the list, and so reads nothing of
/// it: a `"` in a word that doesn't start with one, or a quote that isn't
/// closed or that something other than a space follows.
fn words(value: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    if value.is_empty() {
        return Some(words);
    }
    let mut chars = value.chars();
    // Whether a space ended the last word, so another follows.
    let mut more = true;
    while more {
        let mut word = String::new();
        let mut first = chars.next();
        more = if first == Some('"') {
            loop {
                match chars.next()? {
                    '"' => match chars.next() {
                        Some('"') => word.push('"'),
                        Some(' ') => break true,
                        None => break false,
                        Some(_) => return None,
                    },
                    c => word.push(c),
                }
            }
        } else {
            loop {
                match first.take().or_else(|| chars.next()) {
                    Some(' ') => break true,
                    None => break false,
                    Some('"') => return None,
                    Some(c) => word.push(c),
                }
            }
        };
        words.push(word);
    }
    Some(words)
}

/// [`location_relative`], `depth` levels deep at most.
fn reads_relative(path: &str, whole: bool, depth: usize) -> bool {
    let Some(depth) = depth.checked_sub(1) else {
        return true;
    };
    // rclone reads a path with no `:` as a folder here.
    if whole && !path.contains(':') {
        return starts_relative(path, whole);
    }
    let Some((at, found)) = path
        .char_indices()
        .find(|(_, c)| matches!(c, '/' | '\\' | ':' | ','))
    else {
        // A reference may still add a `:`.
        return false;
    };
    match (at, found) {
        // A `/` or `\` before the remote's name ends is a folder here, which
        // rclone refuses when the text starts as an on-the-fly backend.
        (_, '/' | '\\') => !path.starts_with([':', '/']),
        (0, ':') => path
            .get(1..)
            .is_some_and(|backend| on_the_fly_relative(backend, whole, depth)),
        // An empty remote name, which rclone refuses.
        (0, _) => false,
        // A remote of rclone's configuration file without settings, of a
        // type only that file says.
        (_, ':') => false,
        // rclone reads a remote's type from its settings before the
        // configuration file, the last when there are two.
        _ => path
            .get(at.saturating_add(1)..)
            .and_then(settings)
            .is_some_and(|(settings, path)| {
                let backend = settings
                    .iter()
                    .rev()
                    .find(|(name, _)| *name == "type")
                    .map(|(_, backend)| backend.as_ref());
                backend_relative(backend, &settings, path, whole, depth)
            }),
    }
}

/// [`reads_relative`] for an on-the-fly backend: `text` is what follows its
/// first `:`, the backend's name, its settings, a `:` and its path. Only the
/// `local` backend serves its path from this machine.
fn on_the_fly_relative(text: &str, whole: bool, depth: usize) -> bool {
    let Some((at, found)) = text
        .char_indices()
        .find(|(_, c)| matches!(c, '/' | '\\' | ':' | ','))
    else {
        return false;
    };
    let (Some(backend), Some(rest)) = (text.get(..at), text.get(at.saturating_add(1)..)) else {
        return false;
    };
    let (settings, path) = match found {
        ':' => (Vec::new(), rest),
        ',' => match settings(rest) {
            Some(read) => read,
            None => return false,
        },
        // rclone refuses a backend's name with a `/` or `\` in it.
        _ => return false,
    };
    !backend.is_empty() && backend_relative(Some(backend), &settings, path, whole, depth)
}

/// A setting of a connection string: its name and its value.
type Setting<'a> = (&'a str, Cow<'a, str>);

/// Whether a location of `backend` (unknown for a remote of the
/// configuration file that doesn't say), with `settings` and `path`, names
/// something on this machine by a relative path: one of its settings, read
/// as `backend` reads it, or its path, which only the `local` backend serves
/// from this machine.
fn backend_relative(
    backend: Option<&str>,
    settings: &[Setting<'_>],
    path: &str,
    whole: bool,
    depth: usize,
) -> bool {
    settings
        .iter()
        .any(|(name, value)| setting_relative(backend, name, value, depth))
        || (backend == Some("local") && starts_relative(path, whole))
}

/// The settings of a connection string as rclone reads them, `text` what
/// follows the `,` after the remote's or backend's name: each name with its
/// value, `true` for one written without, a value that starts with `'` or
/// `"` quoted to the same quote, where the quote written twice stands for
/// one. Then the path, after the `:` that ends them. `None` when rclone
/// refuses them, or when they don't end in `text`.
fn settings(mut text: &str) -> Option<(Vec<Setting<'_>>, &str)> {
    let mut settings = Vec::new();
    loop {
        let end = text.find([':', ',', '='])?;
        let name = text.get(..end)?;
        let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.');
        if name.is_empty() || !name.chars().all(valid) {
            return None;
        }
        let separator = text.get(end..)?.chars().next()?;
        text = text.get(end.saturating_add(1)..)?;
        let (value, ends) = match separator {
            '=' => {
                let (value, ends, rest) = value(text)?;
                text = rest;
                (value, ends)
            }
            _ => (Cow::Borrowed("true"), separator),
        };
        settings.push((name, value));
        if ends == ':' {
            return Some((settings, text));
        }
    }
}

/// The value at the start of `text`, the character that ends it (`:` or
/// `,`), and what follows that.
fn value(text: &str) -> Option<(Cow<'_, str>, char, &str)> {
    let Some(quote) = text.chars().next().filter(|c| matches!(c, '\'' | '"')) else {
        let end = text.find([':', ','])?;
        let ends = text.get(end..)?.chars().next()?;
        return Some((
            Cow::Borrowed(text.get(..end)?),
            ends,
            text.get(end.saturating_add(1)..)?,
        ));
    };
    let mut value = String::new();
    let mut chars = text.char_indices().skip(1).peekable();
    while let Some((_, c)) = chars.next() {
        if c != quote {
            value.push(c);
            continue;
        }
        match chars.next() {
            Some((_, next)) if next == quote => value.push(quote),
            Some((at, ends @ (':' | ','))) => {
                return Some((Cow::Owned(value), ends, text.get(at.saturating_add(1)..)?));
            }
            _ => return None,
        }
    }
    None
}
