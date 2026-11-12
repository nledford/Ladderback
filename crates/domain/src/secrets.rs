//! Secrets context: resolved secret values and the redactor that keeps them
//! out of logs, warnings and errors (invariant I4, technical design §10.2).

use std::collections::BTreeMap;

mod command;
mod inline;
mod options;
mod outcome;
#[cfg(test)]
mod properties;
mod source;
mod template;

pub use command::{CommandLine, CommandLineError};
use inline::rclone_credentials;
pub use inline::{
    InlineSecret, MAX_RCLONE_CREDENTIALS, location_secret_written, rclone_location,
    too_many_rclone_credentials, url_password_written,
};
use options::is_rclone_secret_option;
pub use options::is_secret_option;
pub use outcome::{
    MAX_DETAIL_BYTES, Problem, SecretError, SecretWarning, Stdout, command_failure, command_secret,
    decode, file_warning,
};
pub use source::{FileRead, KeyringEntry, NotAKeyringEntry, SecretRef, SourceName, first_line};
pub use template::{Part, Reference, Template, TemplateError, has_reference};

/// The most bytes a secret may have, and a field that may hold references:
/// what is read from a file or from a command's output, what a reference
/// expands to, and what a template is made of. A file or an output of any
/// size could otherwise size an allocation (technical design §13.5).
pub const MAX_SECRET_BYTES: usize = 64 * 1024;

/// The most `${…}` references one field may hold.
pub const MAX_REFERENCES: usize = 16;

/// The most bytes a location (`repository`, `repo-hot`) may hold as it is
/// written: what a job reads of it as a template. A longer one could never
/// run a job, and every page and log that shows it masks it first, so the
/// configuration refuses it when it is read, and an import and a request
/// when they write it (technical design §10.2).
pub const MAX_LOCATION_BYTES: usize = MAX_SECRET_BYTES;

/// The most bytes the value of an option (in `options`, `options-hot` or
/// `options-cold`) may hold as it is written, for the reasons a location
/// has ([`MAX_LOCATION_BYTES`]).
pub const MAX_OPTION_VALUE_BYTES: usize = MAX_SECRET_BYTES;

/// The most options one of `options`, `options-hot` and `options-cold` may
/// hold. A value may be a secret, or hold one, that a redactor looks for in
/// every text it masks, so the work grows with their number; a backend takes
/// a handful.
pub const MAX_OPTIONS: usize = 64;

/// The most bytes of secrets one redactor looks for, each counted once
/// however many places hold it: a secret gives up to about 50 bytes of
/// needles per byte of it, which take about 250 bytes of memory, so this
/// keeps one redactor under about 64 MiB. The secrets of a real repository
/// come to a few kilobytes (technical design §10.2).
pub const MAX_MASKED_BYTES: usize = 256 * 1024;

/// A resolved secret, such as a repository password. Its `Debug` form never
/// shows the value, so it can't leak through a log or a test failure.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself, for the one place that must use it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretValue(***)")
    }
}

/// What a secret is replaced with.
pub const REDACTED: &str = "***";

/// A line of a secret that spans lines is a needle of its own when it has at
/// least this many bytes: a shorter one would mask half of all the text there
/// is.
const MIN_LINE_BYTES: usize = 8;

/// How many lines of one secret are needles of their own, so a large secret
/// can't make every redaction slow.
const MAX_LINES_PER_SECRET: usize = 64;

/// What is known of the secrets of one repository: every text of its
/// settings that can hold one, and what the references in them resolved to.
/// A [`Redactor`] for the repository is built from all of it at once, so no
/// half of a hot/cold repository can be left out. It has no `Debug` form: it
/// borrows texts that are secrets.
#[derive(Clone, Copy)]
pub struct RepositorySecrets<'a> {
    /// rustic's `repository` string: the cold half of a hot/cold repository.
    pub repository: &'a str,
    /// rustic's `repo-hot` string: the hot half, if the repository has one.
    pub repo_hot: Option<&'a str>,
    pub password: &'a SecretValue,
    /// The options both halves get.
    pub options: &'a BTreeMap<String, String>,
    /// The options of the hot half only.
    pub options_hot: &'a BTreeMap<String, String>,
    /// The options of the cold half only.
    pub options_cold: &'a BTreeMap<String, String>,
    /// What the `${…}` references in all of the above resolved to (the
    /// password is not among them).
    pub resolved: &'a [SecretValue],
}

/// The secrets of a repository, counted as a job finds them, each once, in
/// the bytes [`MAX_MASKED_BYTES`] caps: what a [`Redactor`] for it would be
/// built from, before any of it is built. It has no `Debug` form: it holds
/// secrets.
#[derive(Default)]
pub struct MaskedSecrets {
    distinct: std::collections::BTreeSet<String>,
    bytes: usize,
}

/// The secrets counted come to more than [`MAX_MASKED_BYTES`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooMuchToMask;

impl MaskedSecrets {
    /// Counts a secret: the password, or what a reference resolved to.
    pub fn secret(&mut self, secret: &SecretValue) -> Result<(), TooMuchToMask> {
        self.count(secret.expose())
    }

    /// Counts the secrets a location holds: the password of its URL, and
    /// the credentials of an rclone connection string.
    pub fn location(&mut self, location: &str) -> Result<(), TooMuchToMask> {
        location_secrets(location)
            .iter()
            .try_for_each(|secret| self.secret(secret))
    }

    /// Counts the secrets an option holds: the password of a URL in it, and
    /// its whole value when its name says it holds a credential.
    pub fn option(&mut self, name: &str, value: &str) -> Result<(), TooMuchToMask> {
        option_secrets(name, value)
            .iter()
            .try_for_each(|secret| self.secret(secret))
    }

    /// Counts every secret of a repository that a redactor for it masks
    /// ([`Redactor::for_repository`]): where a job is run, its secrets are
    /// counted again, with whatever else is masked beside them.
    pub fn repository(&mut self, secrets: RepositorySecrets<'_>) -> Result<(), TooMuchToMask> {
        let RepositorySecrets {
            repository,
            repo_hot,
            password,
            options,
            options_hot,
            options_cold,
            resolved,
        } = secrets;
        self.secret(password)?;
        std::iter::once(repository)
            .chain(repo_hot)
            .try_for_each(|location| self.location(location))?;
        [options, options_hot, options_cold]
            .into_iter()
            .flatten()
            .try_for_each(|(name, value)| self.option(name, value))?;
        resolved.iter().try_for_each(|secret| self.secret(secret))
    }

    /// Counts `value` once, as [`Redactor::new`] does: nothing for an empty
    /// one or one counted already. Nothing more is kept once the count is
    /// past the cap, which it then stays.
    fn count(&mut self, value: &str) -> Result<(), TooMuchToMask> {
        if self.bytes <= MAX_MASKED_BYTES && !value.is_empty() && !self.distinct.contains(value) {
            self.bytes = self.bytes.saturating_add(value.len());
            if self.bytes <= MAX_MASKED_BYTES {
                self.distinct.insert(value.to_owned());
            }
        }
        if self.bytes > MAX_MASKED_BYTES {
            Err(TooMuchToMask)
        } else {
            Ok(())
        }
    }
}

/// What a location holds that a redactor masks: the password of a URL in
/// it, and each credential of an rclone connection string.
fn location_secrets(location: &str) -> Vec<SecretValue> {
    let mut secrets = url_passwords(location);
    secrets.extend(rclone_credentials(location));
    secrets
}

/// What an option holds that a redactor masks: the password of a URL in its
/// value, and the whole value when its name says it holds a credential
/// ([`is_secret_option`]), even one written inline.
fn option_secrets(name: &str, value: &str) -> Vec<SecretValue> {
    let mut secrets = url_passwords(value);
    if is_secret_option(name) {
        secrets.push(SecretValue::new(value));
    }
    secrets
}

/// Replaces every known secret value, raw or as a URL parser writes it
/// (percent-encoded in full with either hex case, or as the url crate
/// encodes a userinfo, a path, a query, a special URL's query or a fragment;
/// without the tab and newlines it takes out; with `\` read as `/`; and
/// lower-cased, as a host is; and in pieces, where a parser splits it at
/// `@`, `#`, `?`, `/` or `:`; see `url_needles`), with [`REDACTED`]; and what
/// else of it shows in text on its own (see `partial_needles`). Secrets that
/// overlap or touch are replaced as one span. Masking a text compares each of
/// its bytes at most twice per needle, whatever the needles and the text are.
///
/// Masking a masked text again is not always idempotent: when a secret holds
/// `*`, the character of [`REDACTED`], a mask and the text beside it can make
/// up the secret, and a second pass masks that too (secret `*?`: `*??` masks
/// to `***?`, and that to `*****`). A second pass only ever masks more, never
/// reveals what the first masked (I4). Matches inside a mask are not skipped:
/// a real `*` beside a secret would then leave part of it unmasked (#287).
#[derive(Clone, Default)]
pub struct Redactor {
    needles: Vec<Needle>,
    /// Every text is masked whole, which masks more than a secret, never
    /// less (I4): the secrets came to more than [`MAX_MASKED_BYTES`], or a
    /// needle was longer than one can be (`Needle::new`).
    whole: bool,
}

/// A `u32` widens to a `usize` without loss, so a length or a place in a
/// needle kept as a `u32` reads back as it was.
const _: () = assert!(usize::BITS >= u32::BITS);

/// A text a redactor looks for, and where a search for it goes on from when
/// a byte of the text doesn't match the next byte of it, so that no byte of
/// the text is looked at again (Knuth, Morris and Pratt): `borders[i]` is the
/// length of the longest start of `text[..=i]` that is also its end and is
/// shorter than it. A needle is at most `u32::MAX` bytes long, so each
/// border, and each length of it matched, is a `u32`: four bytes per byte
/// of the needle, not eight.
#[derive(Clone)]
struct Needle {
    text: String,
    borders: Vec<u32>,
}

impl Needle {
    /// `None` for a text longer than `u32::MAX` bytes, whose borders a `u32`
    /// couldn't hold.
    fn new(text: String) -> Option<Self> {
        let bytes = text.as_bytes();
        u32::try_from(bytes.len()).ok()?;
        let mut borders = Vec::with_capacity(bytes.len());
        if let Some((_, rest)) = bytes.split_first() {
            // The first byte has no start shorter than itself.
            borders.push(0);
            let mut matched = 0;
            for &byte in rest {
                matched = advance(bytes, &borders, matched, byte);
                borders.push(matched);
            }
        }
        Some(Self { text, borders })
    }
}

#[cfg(test)]
thread_local! {
    /// How many bytes of a needle masking compared with a byte of a text,
    /// on this thread: the tests bound the work with it.
    static COMPARED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many secrets a redactor built the forms of, on this thread: the
    /// tests prove with it that a redactor past the cap builds none.
    static FORMED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How much of the start of `needle` ends at `byte`, when the `matched`
/// bytes before it were the longest start of it that ended there. `borders`
/// holds at least the first `matched` borders of `needle`, which is at most
/// `u32::MAX` bytes long. A whole needle matched goes on as the longest
/// shorter start, so occurrences that overlap are all found (`abab` twice in
/// `ababab`).
fn advance(needle: &[u8], borders: &[u32], mut matched: u32, byte: u8) -> u32 {
    loop {
        #[cfg(test)]
        COMPARED.with(|count| count.set(count.get() + 1));
        if needle.get(matched as usize) == Some(&byte) {
            // `matched` is the place of a byte of the needle, so one more is
            // at most its length, which a `u32` holds.
            return matched.saturating_add(1);
        }
        // A border is shorter than what it is the border of, so this ends.
        match matched
            .checked_sub(1)
            .and_then(|last| borders.get(last as usize))
        {
            Some(&shorter) => matched = shorter,
            None => return 0,
        }
    }
}

/// What a redactor hides is what it holds, so its `Debug` form counts the
/// needles and shows none.
impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Redactor")
            .field("needles", &self.needles.len())
            .finish()
    }
}

impl Redactor {
    /// A redactor for `secrets`, each counted once. When they come to more
    /// than [`MAX_MASKED_BYTES`], none of their forms is built (each about
    /// 50 times the secret's size): the redactor masks every text whole.
    pub fn new<'a>(secrets: impl IntoIterator<Item = &'a SecretValue>) -> Self {
        let mut values: Vec<&str> = secrets
            .into_iter()
            .map(SecretValue::expose)
            .filter(|value| !value.is_empty())
            .collect();
        values.sort_unstable();
        values.dedup();
        let bytes = values
            .iter()
            .try_fold(0_usize, |sum, value| sum.checked_add(value.len()));
        if bytes.is_none_or(|bytes| bytes > MAX_MASKED_BYTES) {
            return Self::masking_everything();
        }
        let mut needles: Vec<String> = values
            .into_iter()
            .flat_map(|value| {
                #[cfg(test)]
                FORMED.with(|count| count.set(count.get() + 1));
                url_needles(value).into_iter().chain(partial_needles(value))
            })
            .collect();
        needles.sort_unstable();
        needles.dedup();
        let mut whole = false;
        let needles = needles
            .into_iter()
            .filter_map(|text| {
                let needle = Needle::new(text);
                whole |= needle.is_none();
                needle
            })
            .collect();
        Self { needles, whole }
    }

    /// A redactor that masks every text that isn't empty whole: for secrets
    /// that come to more than [`MAX_MASKED_BYTES`], or that were not all
    /// read. It masks more than a secret, never less (I4).
    pub fn masking_everything() -> Self {
        Self {
            needles: Vec::new(),
            whole: true,
        }
    }

    /// Whether this redactor masks every text that isn't empty whole.
    pub fn masks_everything(&self) -> bool {
        self.whole
    }

    /// A redactor for everything that gives access to a repository: its
    /// password; the password in the userinfo of every URL in its repository
    /// string and its hot repository string (`rest:https://…`) and in the
    /// value of any option, shared, hot or cold (an `endpoint`), which
    /// rustic's and OpenDAL's errors quote; the value of every credential
    /// setting of either string when it is an rclone connection string
    /// (`rclone::sftp,pass=…:path`), as written and as rclone reads it; the
    /// value of every option, in any of the three sets, whose name says it
    /// holds a credential ([`is_secret_option`]), even one written inline;
    /// and `resolved`, what the references in its settings were resolved to.
    pub fn for_repository(repository: RepositorySecrets<'_>) -> Self {
        let RepositorySecrets {
            repository,
            repo_hot,
            password,
            options,
            options_hot,
            options_cold,
            resolved,
        } = repository;
        let derived: Vec<SecretValue> = std::iter::once(repository)
            .chain(repo_hot)
            .flat_map(location_secrets)
            .chain(
                [options, options_hot, options_cold]
                    .iter()
                    .flat_map(|set| set.iter())
                    .flat_map(|(name, value)| option_secrets(name, value)),
            )
            .collect();
        Self::new(std::iter::once(password).chain(&derived).chain(resolved))
    }

    /// `text` with every needle in it replaced by [`REDACTED`]. Needles that
    /// overlap or touch become one span, so nothing of one is left behind
    /// by the replacement of another.
    pub fn redact(&self, text: &str) -> String {
        self.mask(text, None)
    }

    /// `kept`, a text that may have been cut short (at a cap, by a read that
    /// stopped, or by its writer), with every needle replaced as
    /// [`Redactor::redact`] replaces it, and its end too from where it is
    /// the beginning of a needle on: a secret the cut went
    /// through. A cut inside a character leaves bytes of it that are no
    /// character, and they are left out. This masks more than a secret
    /// where the end of `kept` only looks like the start of one, never
    /// less (I4).
    pub fn redact_cut(&self, kept: &[u8]) -> String {
        let mut text = String::with_capacity(kept.len());
        let mut chunks = kept.utf8_chunks().peekable();
        while let Some(chunk) = chunks.next() {
            text.push_str(chunk.valid());
            // What is no character before the end is shown as one that
            // can't be read, as `from_utf8_lossy` shows it.
            if !chunk.invalid().is_empty() && chunks.peek().is_some() {
                text.push(char::REPLACEMENT_CHARACTER);
            }
        }
        self.mask(&text, Some(text.len()))
    }

    /// `text` masked as [`Redactor::redact`] masks it, unless it ends in
    /// `mark`, which says that whoever cut it short knew fewer secrets than
    /// this redactor: then what was kept is masked as
    /// [`Redactor::redact_cut`] masks it, and the mark stays, unless it is
    /// the end of a whole secret, which is masked with it.
    pub fn redact_clipped(&self, text: &str, mark: &str) -> String {
        self.mask(text, text.strip_suffix(mark).map(str::len))
    }

    /// `text` with every needle in it replaced. When `cut` is given, the
    /// text was cut after its first `cut` bytes, and the end of those from
    /// where it is the start of a needle on (the earliest start of a secret
    /// the cut could have gone through) is part of the same spans.
    ///
    /// One pass over the text per needle: each byte of the text is compared
    /// at most twice with a byte of the needle, however long and repetitive
    /// both are. A needle and a valid text are UTF-8, so what matches starts
    /// and ends between characters.
    fn mask(&self, text: &str, cut: Option<usize>) -> String {
        if self.whole && !text.is_empty() {
            return REDACTED.to_owned();
        }
        let bytes = text.as_bytes();
        // The longest span, of a needle or of the cut, that ends at each byte:
        // at most a needle's length, which a `u32` holds.
        let mut spans = vec![0_u32; bytes.len()];
        for needle in &self.needles {
            let whole = needle.text.as_bytes();
            let mut matched = 0;
            for (at, (&byte, span)) in bytes.iter().zip(spans.iter_mut()).enumerate() {
                matched = advance(whole, &needle.borders, matched, byte);
                if matched as usize == whole.len() || cut == Some(at + 1) {
                    *span = (*span).max(matched);
                }
            }
        }
        // From the end: a byte is covered when a span that ends at it or
        // after it starts at it or before it. A byte no span ends at gives
        // the byte after it, which covers nothing.
        let mut first = usize::MAX;
        let mut any = false;
        for (at, span) in spans.iter_mut().enumerate().rev() {
            first = first.min((at + 1).saturating_sub(*span as usize));
            let covered = first <= at;
            any |= covered;
            *span = u32::from(covered);
        }
        if !any {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut in_span = false;
        for (at, c) in text.char_indices() {
            if spans.get(at) == Some(&1) {
                if !in_span {
                    out.push_str(REDACTED);
                    in_span = true;
                }
            } else {
                in_span = false;
                out.push(c);
            }
        }
        out
    }
}

/// What else of a secret shows in text without being the whole of it, apart
/// from its encodings: the secret with the white space around it taken off
/// (a variable that ends in a newline is logged without it), and each line
/// that is long enough of one that spans lines (a key, a certificate), for
/// text that reaches a reader a line at a time.
fn partial_needles(secret: &str) -> Vec<String> {
    let trimmed = secret.trim();
    let mut needles = Vec::new();
    // A secret with nothing around it is its own trimmed form: `Redactor::new`
    // drops the duplicate.
    if !trimmed.is_empty() {
        needles.push(trimmed.to_owned());
    }
    if trimmed.contains('\n') {
        needles.extend(
            trimmed
                .lines()
                .map(str::trim)
                .filter(|line| line.len() >= MIN_LINE_BYTES)
                .take(MAX_LINES_PER_SECRET)
                .map(str::to_owned),
        );
    }
    needles
}

/// Every password the userinfo of the URL inside a repository string may
/// hold, as written and decoded; empty when the string has no URL or the
/// URL has no password.
///
/// The userinfo usually ends at the last `@` before the first `/`, but a raw
/// `/` in the password ends the authority early, so the last `@` of the
/// whole string is tried too. Masking part of a path by mistake is the safe
/// direction.
pub fn url_passwords(repository: &str) -> Vec<SecretValue> {
    let Some((_, rest)) = repository.split_once("://") else {
        return Vec::new();
    };
    forms(
        userinfos(rest)
            .into_iter()
            .filter_map(|userinfo| userinfo.split_once(':'))
            .map(|(_, password)| password),
    )
}

/// Whether a repository string is an rclone connection string that carries a
/// secret: `rclone::s3,provider=AWS,secret_access_key=…:bucket` holds the
/// settings of its backend as `key=value` pieces between commas and colons,
/// and a piece whose key names a credential in a connection string
/// ([`is_rclone_secret_option`]) is one. Any other string is not, whatever it
/// holds. Every piece is read, the path's too, so this finds every setting
/// the redactor masks and more.
pub fn rclone_secret(location: &str) -> bool {
    location.strip_prefix("rclone:").is_some_and(|rest| {
        rest.split([',', ':'])
            .filter_map(|piece| piece.split_once('='))
            .any(|(key, _)| is_rclone_secret_option(key))
    })
}

/// What the userinfo of a URL holds that must not show, wherever the URL is
/// echoed: its password (as [`url_passwords`] finds it) and the whole
/// userinfo, each as written and decoded. The user name is the credential
/// when there is no password (a token as the user name, `token@proxy` or
/// `token:@proxy`), so it is one then. `url` may lack its scheme
/// (`demo:s3cr3t@proxy:3128`), as proxy settings often do. Empty when there
/// is no userinfo.
pub fn url_credentials(url: &str) -> Vec<SecretValue> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let userinfos = userinfos(rest);
    let split: Vec<(&str, &str)> = userinfos
        .iter()
        .filter_map(|userinfo| userinfo.split_once(':'))
        .collect();
    forms(
        split
            .iter()
            .map(|&(_, password)| password)
            .chain(
                split
                    .iter()
                    .filter(|(_, password)| password.is_empty())
                    .map(|&(user, _)| user),
            )
            .chain(userinfos.iter().copied()),
    )
}

/// The userinfos a URL's authority and path (what follows its scheme) may
/// hold: the one before the last `@` of the authority, and the one before
/// the last `@` of the whole of it, for a raw `/` in a password ends the
/// authority early.
fn userinfos(rest: &str) -> Vec<&str> {
    let authority = rest.split('/').next().unwrap_or(rest);
    [authority, rest]
        .into_iter()
        .filter_map(|candidate| candidate.rsplit_once('@'))
        .map(|(userinfo, _)| userinfo)
        // A `/` before the `:` puts the `@` in the path, not in a userinfo.
        .filter(|userinfo| {
            let user = userinfo.split_once(':').map_or(*userinfo, |(user, _)| user);
            !user.contains('/')
        })
        .collect()
}

/// Each of `texts` that isn't empty, as written and percent-decoded, once.
fn forms<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<SecretValue> {
    let mut found: Vec<String> = Vec::new();
    for text in texts.into_iter().filter(|text| !text.is_empty()) {
        for form in [Some(text.to_owned()), percent_decode(text)]
            .into_iter()
            .flatten()
        {
            if !found.contains(&form) {
                found.push(form);
            }
        }
    }
    found.into_iter().map(SecretValue::new).collect()
}

/// `None` when `text` isn't valid percent-encoding.
fn percent_decode(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let (hex, after) = tail.split_at_checked(2)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            bytes.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            rest = after;
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

#[derive(Clone, Copy)]
enum Hex {
    Upper,
    Lower,
}

/// RFC 3986 unreserved characters.
fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~".contains(&b)
}

/// What the url crate (the WHATWG URL standard) encodes in each part of a
/// URL besides controls, space and non-ASCII, which every part encodes. A
/// byte outside the set is written as it is. The sets are those of url
/// 2.5.8; the path set of a newer standard also takes `^`, so both are
/// needles (see `PATH_WITH_CARET`).
const FRAGMENT_SET: &[u8] = b"\"<>`";
const QUERY_SET: &[u8] = b"\"#<>";
const SPECIAL_QUERY_SET: &[u8] = b"\"#<>'";
const PATH_SET: &[u8] = b"\"<>`#?{}";
const PATH_WITH_CARET_SET: &[u8] = b"\"<>`#?{}^";
const USERINFO_SET: &[u8] = b"\"#<>?`{}/:;=@[\\]^|";

/// The bytes `set` doesn't name, apart from controls, space and non-ASCII,
/// stay as they are.
fn all_but(set: &'static [u8]) -> impl Fn(u8) -> bool {
    move |b| b.is_ascii_graphic() && !set.contains(&b)
}

/// Percent-encodes every byte that `keep` rejects, with `hex` digits.
fn percent_encode(value: &str, keep: impl Fn(u8) -> bool, hex: Hex) -> String {
    use std::fmt::Write;
    value.bytes().fold(String::new(), |mut out, b| {
        if keep(b) {
            out.push(char::from(b));
        } else {
            let _ = match hex {
                Hex::Upper => write!(out, "%{b:02X}"),
                Hex::Lower => write!(out, "%{b:02x}"),
            };
        }
        out
    })
}

/// A piece of a secret that a URL parser takes apart is a needle of its own
/// when it has at least this many bytes: a shorter one would mask half of
/// all the text there is, as with [`MIN_LINE_BYTES`].
const MIN_PIECE_BYTES: usize = 8;

/// How many pieces of one secret are needles of their own, so a large secret
/// can't make every redaction slow.
const MAX_PIECES_PER_SECRET: usize = 64;

/// The characters at which a URL parser takes a secret apart: it ends the
/// userinfo (`@`), the host and the path (`?`, `/`) and the query (`#`), and
/// splits a user name from a password (`:`).
const SPLITTING: [char; 5] = ['@', '#', '?', '/', ':'];

/// Every way a URL parser can write `value`. A parser takes tab and
/// newlines out of a URL and reads `\` as `/` (in a special URL's path), so
/// those two rewrites of the value are encoded too; then each is written as
/// fully encoded (either hex case) and as each part of a URL encodes it,
/// with upper-case hex like the url crate. A host is lower-cased, so every
/// needle is lower-cased too. A secret of any size has at most about 130
/// needles of its own size or up to three times it, and 64 pieces of up to
/// 18 each; a secret of ordinary characters has about a dozen.
///
/// A secret that holds a character a parser splits a URL at is printed in
/// pieces, each as its part of the URL writes it, and a library that takes
/// a URL apart may print them one by one. So each piece of at least
/// [`MIN_PIECE_BYTES`] is a needle in all those forms, and the secret is
/// also written as the parser prints it when it starts in a host, a path or
/// a query and moves on at each delimiter (a mixed form: `canary/Alpha`).
/// Not covered: a host that isn't ASCII (punycode needs an IDNA library),
/// percent-decoding in a host, and dot segments in a path.
fn url_needles(value: &str) -> Vec<String> {
    let stripped: String = value
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let slashed = stripped.replace('\\', "/");
    let mut bases: Vec<&str> = [value, stripped.as_str(), slashed.as_str()]
        .into_iter()
        .filter(|base| !base.is_empty())
        .collect();
    // A rewrite that changed nothing is the same text (and each rewrite is
    // the same as the last, or different from all before it).
    bases.dedup();
    let mut needles = Vec::new();
    for base in &bases {
        written_each_way(base, &mut needles);
        for start in [Stage::Host, Stage::Path, Stage::Query] {
            for special in [false, true] {
                for caret in [false, true] {
                    needles.push(written_across_parts(base, start, special, caret));
                }
            }
        }
    }
    let mut pieces: Vec<&str> = Vec::new();
    for piece in bases
        .iter()
        .flat_map(|base| base.split(SPLITTING))
        .filter(|piece| piece.len() >= MIN_PIECE_BYTES)
    {
        if pieces.len() == MAX_PIECES_PER_SECRET {
            break;
        }
        if !pieces.contains(&piece) {
            pieces.push(piece);
        }
    }
    for piece in pieces {
        written_each_way(piece, &mut needles);
    }
    needles.sort_unstable();
    needles.dedup();
    let lower: Vec<String> = needles.iter().map(|n| n.to_ascii_lowercase()).collect();
    needles.extend(lower);
    needles.sort_unstable();
    needles.dedup();
    needles
}

/// `text` as it is, fully encoded with either hex case, and as each part of
/// a URL encodes it on its own.
fn written_each_way(text: &str, needles: &mut Vec<String>) {
    needles.push(text.to_owned());
    needles.push(percent_encode(text, is_unreserved, Hex::Upper));
    needles.push(percent_encode(text, is_unreserved, Hex::Lower));
    for set in [
        USERINFO_SET,
        FRAGMENT_SET,
        QUERY_SET,
        SPECIAL_QUERY_SET,
        PATH_SET,
        PATH_WITH_CARET_SET,
    ] {
        needles.push(percent_encode(text, all_but(set), Hex::Upper));
    }
}

/// The part of a URL a parser is in while it writes a secret.
#[derive(Clone, Copy)]
enum Stage {
    Host,
    Path,
    Query,
    Fragment,
}

/// `value` as a parser prints it when it starts in `start` and moves on at
/// each delimiter: a `/` ends a host, a `?` a host or a path, a `#` all but
/// a fragment. Each stretch is written the way its part of the URL writes
/// it; a delimiter stays as it is. A `special` URL (`http`, `https`, …) has
/// the path `/` when it has none, and a query that encodes `'` too.
fn written_across_parts(value: &str, start: Stage, special: bool, caret: bool) -> String {
    let mut written = String::with_capacity(value.len());
    let mut stage = start;
    let mut stretch = String::new();
    for c in value.chars() {
        let next = match (stage, c) {
            (Stage::Host, '/') => Some(Stage::Path),
            (Stage::Host | Stage::Path, '?') => Some(Stage::Query),
            (Stage::Host | Stage::Path | Stage::Query, '#') => Some(Stage::Fragment),
            _ => None,
        };
        if let Some(next) = next {
            write_stretch(&mut written, stage, &stretch, special, caret);
            if special && matches!(stage, Stage::Host) && c != '/' {
                written.push('/');
            }
            written.push(c);
            stretch.clear();
            stage = next;
        } else {
            stretch.push(c);
        }
    }
    write_stretch(&mut written, stage, &stretch, special, caret);
    written
}

/// Appends `stretch` as the `stage` of a URL writes it.
fn write_stretch(out: &mut String, stage: Stage, stretch: &str, special: bool, caret: bool) {
    let encoded = |set| percent_encode(stretch, all_but(set), Hex::Upper);
    match stage {
        // The last `@` of an authority ends the userinfo, whose first `:`
        // ends the user name; a host is lower-cased.
        Stage::Host => match stretch.rsplit_once('@') {
            Some((userinfo, host)) => {
                let (user, password) = userinfo.split_once(':').unzip();
                let encode = |text: &str| percent_encode(text, all_but(USERINFO_SET), Hex::Upper);
                out.push_str(&encode(user.unwrap_or(userinfo)));
                if let Some(password) = password {
                    out.push(':');
                    out.push_str(&encode(password));
                }
                out.push('@');
                out.push_str(&host.to_ascii_lowercase());
            }
            None => out.push_str(&stretch.to_ascii_lowercase()),
        },
        Stage::Path => out.push_str(&encoded(if caret { PATH_WITH_CARET_SET } else { PATH_SET })),
        Stage::Query => out.push_str(&encoded(if special {
            SPECIAL_QUERY_SET
        } else {
            QUERY_SET
        })),
        Stage::Fragment => out.push_str(&encoded(FRAGMENT_SET)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor(secrets: &[&str]) -> Redactor {
        let secrets: Vec<SecretValue> = secrets.iter().map(|&s| SecretValue::new(s)).collect();
        Redactor::new(&secrets)
    }

    #[test]
    fn a_secret_never_shows_in_its_debug_form() {
        let secret = SecretValue::new("hunter2");
        let debug = format!("{secret:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(
            debug.contains(REDACTED),
            "a masked value still shows: {debug}"
        );
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn a_redactor_never_shows_what_it_hides_in_its_debug_form() {
        let debug = format!("{:?}", redactor(&["hunter2", "tok-ABC", "a b"]));
        for secret in ["hunter2", "tok-ABC", "tok-abc", "a%20b"] {
            assert!(!debug.contains(secret), "{secret} leaked: {debug}");
        }
        assert!(debug.contains("Redactor"), "{debug}");
    }

    #[test]
    fn a_secret_that_overlaps_itself_is_replaced_whole() {
        for (secret, text) in [
            ("abab", "ababab"),
            ("abcabc", "abcabcabc"),
            ("aaaa", "aaaaa"),
        ] {
            assert_eq!(
                redactor(&[secret]).redact(text),
                "***",
                "{secret} in {text}"
            );
        }
        // At character boundaries, with text around it.
        assert_eq!(redactor(&["éé"]).redact("xéééy"), "x***y");
    }

    #[test]
    fn every_occurrence_of_every_secret_is_replaced() {
        let redactor = redactor(&["hunter2", "s3cr3t"]);
        assert_eq!(
            redactor.redact("hunter2 then s3cr3t then hunter2"),
            "*** then *** then ***"
        );
    }

    #[test]
    fn a_secret_inside_a_url_is_replaced_in_its_encoded_form() {
        let redactor = redactor(&["p@ss word/1"]);
        assert_eq!(
            redactor.redact("rest:https://demo:p%40ss%20word%2F1@nas/ and p%40ss%20word%2f1"),
            "rest:https://demo:***@nas/ and ***"
        );
    }

    /// The url crate leaves `!`, `$`, `&` and the like unencoded in a
    /// userinfo, and rustic quotes URLs as the url crate writes them.
    #[test]
    fn a_secret_is_replaced_as_the_url_crate_encodes_a_userinfo() {
        let redactor = redactor(&["P@ssw0rd!"]);
        assert_eq!(
            redactor.redact("URL `rest:https://demo:P%40ssw0rd!@nas:80x/b` parsing failed"),
            "URL `rest:https://demo:***@nas:80x/b` parsing failed"
        );
    }

    #[test]
    fn lowercase_percent_encoding_keeps_the_letters_case() {
        let redactor = redactor(&["Ab/C"]);
        assert_eq!(redactor.redact("x Ab%2fC x"), "x *** x");
    }

    #[test]
    fn a_secret_containing_another_is_replaced_whole() {
        let redactor = redactor(&["abc", "abcdef"]);
        assert_eq!(redactor.redact("xabcdefx abc"), "x***x ***");
    }

    #[test]
    fn secrets_that_overlap_are_replaced_as_one_span() {
        let redactor = redactor(&["abcdefghij", "defghijklm"]);
        assert_eq!(redactor.redact("abcdefghijklm"), "***");
        assert_eq!(redactor.redact("x abcdefghijklm y"), "x *** y");
    }

    #[test]
    fn a_second_pass_over_a_secret_with_the_mask_character_never_reveals() {
        // #287: it may mask more, but what is left besides masks is what the
        // first pass left, in order, and never more of it.
        let redactor = redactor(&["*?"]);
        let once = redactor.redact("*??");
        let twice = redactor.redact(&once);
        assert!(!once.contains("??"), "{once:?}");
        let mut first = once.chars().filter(|&c| c != '*');
        assert!(
            twice
                .chars()
                .filter(|&c| c != '*')
                .all(|c| first.any(|f| f == c)),
            "{once:?} then {twice:?}"
        );
    }

    #[test]
    fn secrets_that_touch_are_replaced_as_one_span() {
        let redactor = redactor(&["abc", "def"]);
        assert_eq!(redactor.redact("abcdef ghi abc"), "*** ghi ***");
    }

    #[test]
    fn text_that_is_not_ascii_is_cut_only_at_character_boundaries() {
        let redactor = redactor(&["päss", "ssé"]);
        assert_eq!(redactor.redact("é pässéé ü"), "é ***é ü");
    }

    /// Where a secret may land in a URL, and what a URL parser makes of it:
    /// the parser, not the redactor, decides how it is printed, so the
    /// `url` crate is the oracle.
    #[derive(Clone, Copy, Debug)]
    enum Place {
        Userinfo,
        Host,
        Path,
        Query,
        Fragment,
    }

    fn url_with(place: Place, secret: &str) -> String {
        match place {
            Place::Userinfo => format!("http://demo:{secret}@nas.example/b"),
            Place::Host => format!("http://{secret}.example/b"),
            Place::Path => format!("http://nas.example/{secret}/b"),
            Place::Query => format!("http://nas.example/b?k={secret}&z=1"),
            Place::Fragment => format!("http://nas.example/b#{secret}"),
        }
    }

    #[test]
    fn a_secret_shows_nowhere_as_the_url_parser_prints_it() {
        const PIECES: [&str; 14] = [
            "a b:c",
            "q=1&x y",
            "\"q\\b",
            "tab\there",
            "nl\nine",
            "it's",
            "Host",
            "ab+/=",
            "100%",
            "päss",
            "a;b=c|d^e",
            "x[y]z",
            "{x} <y>",
            "back`tick",
        ];
        let mut checked = 0;
        for piece in PIECES {
            let secret = format!("CANARY{piece}ZZEND");
            for place in [
                Place::Userinfo,
                Place::Host,
                Place::Path,
                Place::Query,
                Place::Fragment,
            ] {
                // What the parser refuses, or what changes the URL's shape
                // (a `/` ends a host), is not a case of this kind.
                // A host that isn't ASCII is written in punycode, which needs
                // an IDNA library that the domain doesn't link: not covered.
                if matches!(place, Place::Host) && !secret.is_ascii() {
                    continue;
                }
                let Ok(url) = url::Url::parse(&url_with(place, &secret)) else {
                    continue;
                };
                let kept_shape = match place {
                    Place::Host => url.host_str().is_some_and(|h| h.ends_with(".example")),
                    Place::Userinfo => {
                        url.password().is_some() && url.host_str() == Some("nas.example")
                    }
                    _ => url.host_str() == Some("nas.example"),
                };
                if !kept_shape {
                    continue;
                }
                checked += 1;
                let printed = url.as_str();
                let redacted = redactor(&[&secret]).redact(printed).to_lowercase();
                for marker in ["canary", "zzend"] {
                    assert!(
                        !redacted.contains(marker),
                        "{piece:?} in {place:?} shows as {printed:?}, redacted to {redacted:?}"
                    );
                }
            }
        }
        assert!(checked >= 50, "only {checked} cases ran");
    }

    /// The pieces of a secret that a URL parser takes apart: each of at
    /// least 8 bytes, in mixed case, so that a parser that lower-cases a host
    /// shows a piece in a case the secret doesn't have.
    const MARKERS: [&str; 3] = ["Canary-Alpha", "Bravo-Delta", "Echo-ZzEnd"];

    /// Secrets made of the markers and the characters a parser splits a URL
    /// at (`@`, `#`, `?`, `/`, `:`), in every order of two.
    fn splitting_secrets() -> Vec<String> {
        let delimiters = ["@", "#", "?", "/", ":"];
        let [first, second, third] = MARKERS;
        let mut secrets = Vec::new();
        for one in delimiters {
            secrets.push(format!("{first}{one}{second}"));
            for two in delimiters {
                secrets.push(format!("{first}{one}{second}{two}{third}"));
            }
        }
        secrets
    }

    const PLACES: [Place; 5] = [
        Place::Userinfo,
        Place::Host,
        Place::Path,
        Place::Query,
        Place::Fragment,
    ];

    /// A secret that holds a character a parser splits a URL at is shown in
    /// pieces, each as its part of the URL is printed (a host in lower case,
    /// a path as it is). The whole URL, as it is printed, shows none.
    #[test]
    fn a_secret_a_url_parser_splits_shows_nowhere_in_the_url_it_prints() {
        let mut checked = 0;
        for secret in splitting_secrets() {
            let redactor = redactor(&[&secret]);
            for place in PLACES {
                // What the parser refuses is not a case of this kind.
                let Ok(url) = url::Url::parse(&url_with(place, &secret)) else {
                    continue;
                };
                checked += 1;
                let printed = url.as_str();
                let redacted = redactor.redact(printed).to_lowercase();
                for marker in MARKERS {
                    assert!(
                        !redacted.contains(&marker.to_lowercase()),
                        "{secret:?} in {place:?} shows as {printed:?}, redacted to {redacted:?}"
                    );
                }
            }
        }
        assert!(checked >= 70, "only {checked} cases ran");
    }

    /// A library that takes a URL apart may log its parts one by one. No part
    /// of a secret that is 8 bytes or more shows in any of them.
    #[test]
    fn the_parts_of_a_split_secret_show_nowhere_when_printed_one_by_one() {
        let mut checked = 0;
        for secret in splitting_secrets() {
            let redactor = redactor(&[&secret]);
            for place in PLACES {
                let Ok(url) = url::Url::parse(&url_with(place, &secret)) else {
                    continue;
                };
                checked += 1;
                let parts = [
                    url.username(),
                    url.password().unwrap_or_default(),
                    url.host_str().unwrap_or_default(),
                    url.path(),
                    url.query().unwrap_or_default(),
                    url.fragment().unwrap_or_default(),
                ];
                for part in parts {
                    let redacted = redactor.redact(part).to_lowercase();
                    for marker in MARKERS {
                        assert!(
                            !redacted.contains(&marker.to_lowercase()),
                            "{secret:?} in {place:?}: the part {part:?} shows {marker}"
                        );
                    }
                }
            }
        }
        assert!(checked >= 70, "only {checked} cases ran");
    }

    /// Pieces too short to be needles of their own (a secret may be made of
    /// them) still show nowhere in the URL that is printed whole, where the
    /// parser writes each in its part's way.
    #[test]
    fn a_secret_of_short_pieces_shows_nowhere_in_the_url_it_prints() {
        let markers = ["Qx1", "Zy2", "Wv3"];
        let delimiters = ["@", "#", "?", "/", ":"];
        let mut checked = 0;
        for one in delimiters {
            for two in delimiters {
                let [first, second, third] = markers;
                for secret in [
                    format!("{first}{one}{second}"),
                    format!("{first}{one}{second}{two}{third}"),
                ] {
                    let redactor = redactor(&[&secret]);
                    for place in PLACES {
                        let Ok(url) = url::Url::parse(&url_with(place, &secret)) else {
                            continue;
                        };
                        checked += 1;
                        let printed = url.as_str();
                        let redacted = redactor.redact(printed).to_lowercase();
                        assert!(
                            !redacted.contains("qx1") && !redacted.contains("zy2"),
                            "{secret:?} in {place:?} shows as {printed:?}, redacted to {redacted:?}"
                        );
                    }
                }
            }
        }
        assert!(checked >= 100, "only {checked} cases ran");
    }

    /// A piece of 8 bytes is a needle of its own; one of 7 is not, or it
    /// would mask ordinary text.
    #[test]
    fn a_piece_of_a_split_secret_is_masked_alone_from_8_bytes_on() {
        let redactor = redactor(&["Abcdefgh/ijklmno@Pqrstuvw"]);
        assert_eq!(redactor.redact("x Abcdefgh y"), "x *** y");
        assert_eq!(redactor.redact("x Pqrstuvw y"), "x *** y");
        assert_eq!(redactor.redact("x ijklmno y"), "x ijklmno y");
        // ... and as a host writes it.
        assert_eq!(redactor.redact("x abcdefgh y"), "x *** y");
    }

    /// The pieces of a secret that are needles of their own are limited, the
    /// first ones kept.
    #[test]
    fn only_the_first_64_pieces_of_a_secret_are_needles_of_their_own() {
        let piece = |n: usize| format!("piece{n:03}x");
        let secret: Vec<String> = (0..65).map(piece).collect();
        let redactor = redactor(&[&secret.join("/")]);
        assert_eq!(redactor.redact(&piece(0)), "***");
        assert_eq!(redactor.redact(&piece(63)), "***");
        assert_eq!(redactor.redact(&piece(64)), piece(64));
    }

    /// A piece that is the same in every rewrite of a secret takes one place
    /// of the 64, not one for each rewrite, so the pieces that only a rewrite
    /// has (a `\` is read as `/`) still find a place.
    #[test]
    fn a_piece_that_every_rewrite_has_takes_one_place_of_the_64() {
        let secret: Vec<String> = (0..40)
            .map(|n| format!("a{n:02}-head\\z{n:02}-tail"))
            .collect();
        let redactor = redactor(&[&secret.join("/")]);
        assert_eq!(redactor.redact("z00-tail"), "***");
        assert_eq!(redactor.redact("z11-tail"), "***");
    }

    /// The secret as it is written when it starts in a part of the URL and
    /// is carried over into the next at each delimiter.
    #[test]
    fn a_secret_is_written_part_by_part_as_the_parts_of_a_url_write_it() {
        use Stage::{Host, Path, Query};
        let write =
            |value, start, special, caret| written_across_parts(value, start, special, caret);
        // A host is lower-cased, the rest of it is not.
        assert_eq!(write("Ab/Cd?Ef#Gh", Host, false, false), "ab/Cd?Ef#Gh");
        // A special URL has the path `/` when it has none.
        assert_eq!(write("Ab?Cd", Host, true, false), "ab/?Cd");
        assert_eq!(write("Ab#Cd", Host, true, false), "ab/#Cd");
        assert_eq!(write("Ab/Cd", Host, true, false), "ab/Cd");
        assert_eq!(write("Ab?Cd", Host, false, false), "ab?Cd");
        assert_eq!(write("Ab#Cd", Host, false, false), "ab#Cd");
        // A path escapes its own set, and a `^` as the newer standard does.
        assert_eq!(
            write("a b{c}`d^e", Path, false, false),
            "a%20b%7Bc%7D%60d^e"
        );
        assert_eq!(
            write("a b{c}`d^e", Path, false, true),
            "a%20b%7Bc%7D%60d%5Ee"
        );
        // The query escapes its own, and `'` in a special URL's.
        assert_eq!(
            write("a'b\"c<d>e f", Query, false, false),
            "a'b%22c%3Cd%3Ee%20f"
        );
        assert_eq!(
            write("a'b\"c<d>e f", Query, true, false),
            "a%27b%22c%3Cd%3Ee%20f"
        );
        // `?` ends a path but not a query; `#` ends them both, and not a
        // fragment, which escapes only its own.
        assert_eq!(
            write("a?b/c d#e?f#g`h'i", Path, true, false),
            "a?b/c%20d#e?f#g%60h'i"
        );
        assert_eq!(write("a?b#c", Query, true, false), "a?b#c");
        assert_eq!(write("a b#c d", Query, false, false), "a%20b#c%20d");
        // The userinfo of a host: split at its last `@`, its user name from
        // its password at the first `:`, each escaped as a userinfo is.
        assert_eq!(
            write("Us:Pa:ss@Ho/St", Host, false, false),
            "Us:Pa%3Ass@ho/St"
        );
        assert_eq!(write("A@b/c@D", Host, false, false), "A@b/c@D");
        assert_eq!(write("A/b@C@D", Host, false, false), "a/b@C@D");
        assert_eq!(write("Us@Ho", Host, false, false), "Us@ho");
        assert_eq!(write("U@s@Ho", Host, false, false), "U%40s@ho");
        assert_eq!(write("U s:P w@Ho", Host, false, false), "U%20s:P%20w@ho");
    }

    #[test]
    fn an_empty_secret_redacts_nothing() {
        assert_eq!(redactor(&[""]).redact("text"), "text");
        assert_eq!(redactor(&[]).redact("text"), "text");
    }

    fn passwords(repository: &str) -> Vec<String> {
        url_passwords(repository)
            .iter()
            .map(|secret| secret.expose().to_owned())
            .collect()
    }

    /// A repository's settings as a redactor is built from them: only what a
    /// test sets is there.
    struct Settings {
        repository: String,
        repo_hot: Option<String>,
        password: SecretValue,
        options: BTreeMap<String, String>,
        options_hot: BTreeMap<String, String>,
        options_cold: BTreeMap<String, String>,
        resolved: Vec<SecretValue>,
    }

    impl Settings {
        fn new(repository: &str) -> Self {
            Self {
                repository: repository.to_owned(),
                repo_hot: None,
                password: SecretValue::new("hunter2"),
                options: BTreeMap::new(),
                options_hot: BTreeMap::new(),
                options_cold: BTreeMap::new(),
                resolved: Vec::new(),
            }
        }

        fn secrets(&self) -> RepositorySecrets<'_> {
            RepositorySecrets {
                repository: &self.repository,
                repo_hot: self.repo_hot.as_deref(),
                password: &self.password,
                options: &self.options,
                options_hot: &self.options_hot,
                options_cold: &self.options_cold,
                resolved: &self.resolved,
            }
        }

        fn redactor(&self) -> Redactor {
            Redactor::for_repository(self.secrets())
        }
    }

    fn for_repository(repository: &str, password: &str) -> Redactor {
        let mut settings = Settings::new(repository);
        settings.password = SecretValue::new(password);
        settings.redactor()
    }

    #[test]
    fn the_password_of_a_url_in_a_repository_string_is_found() {
        for repository in [
            "rest:https://demo:s3cr3t@nas:8000/backups",
            "https://demo:s3cr3t@nas/backups",
            "rest:https://demo:s3cr3t@nas",
        ] {
            assert_eq!(passwords(repository), ["s3cr3t"], "{repository}");
        }
    }

    #[test]
    fn a_percent_encoded_url_password_is_found_decoded_and_as_written() {
        assert_eq!(
            passwords("rest:https://demo:p%40ss%2Fw%20rd@nas/b"),
            ["p%40ss%2Fw%20rd", "p@ss/w rd"]
        );
    }

    #[test]
    fn an_unencoded_at_sign_in_the_password_stays_in_the_password() {
        assert_eq!(passwords("rest:https://demo:p@ss@nas/b"), ["p@ss"]);
    }

    #[test]
    fn a_password_that_is_not_valid_percent_encoding_is_taken_as_it_is() {
        assert_eq!(passwords("rest:https://demo:100%@nas/b"), ["100%"]);
        assert_eq!(passwords("rest:https://demo:a%zzb@nas/b"), ["a%zzb"]);
    }

    /// A raw `/` ends the authority, so the URL doesn't parse, and rustic's
    /// error quotes the whole string.
    #[test]
    fn a_raw_slash_in_the_password_does_not_hide_it() {
        let redactor = for_repository("rest:https://demo:ab/cd@nas/b", "hunter2");
        assert_eq!(
            redactor.redact("URL `rest:https://demo:ab/cd@nas/b` parsing failed"),
            "URL `rest:https://demo:***@nas/b` parsing failed"
        );
    }

    #[test]
    fn a_repository_string_without_a_url_password_has_none() {
        for repository in [
            "/backups/home",
            "local:/backups/home",
            "opendal:s3",
            "rclone:remote:path",
            "rest:https://demo@nas/b",
            "rest:https://nas:8000/b",
            // The @ is in the path, not in the authority.
            "rest:https://nas/b/x:y@z",
        ] {
            assert_eq!(passwords(repository), Vec::<String>::new(), "{repository}");
        }
    }

    #[test]
    fn an_rclone_connection_string_holds_a_secret_when_a_piece_names_one() {
        for location in [
            "rclone::s3,provider=AWS,secret_access_key=abc:bucket/repo",
            "rclone::drive,token=abc:repo",
            "rclone::sftp,host=nas.demo,CLIENT_SECRET=abc",
            "rclone:remote,password=abc:path",
            "rclone:::s3,x=1:sas-token=abc",
        ] {
            assert!(rclone_secret(location), "{location}");
        }
    }

    #[test]
    fn an_address_that_is_not_an_rclone_connection_string_or_names_no_secret_holds_none() {
        for location in [
            "rclone:remote:path",
            "rclone::s3,provider=AWS,access_key_id=demo:bucket",
            "rclone::s3,use_password=true:bucket",
            "rclone:",
            "rclone",
            "/srv/repo,token=demo",
            "local:rclone::s3,token=demo",
            "sftp:demo@nas:/srv/repo,secret=demo",
            "Rclone::s3,token=demo",
        ] {
            assert!(!rclone_secret(location), "{location}");
        }
    }

    fn credentials(url: &str) -> Vec<String> {
        let mut found: Vec<String> = url_credentials(url)
            .iter()
            .map(|secret| secret.expose().to_owned())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn the_password_and_the_whole_userinfo_of_a_url_are_credentials() {
        for url in [
            "http://demo:s3cr3t@proxy.demo:3128",
            "https://demo:s3cr3t@proxy.demo",
            "socks5h://demo:s3cr3t@proxy.demo:1080/",
        ] {
            assert_eq!(credentials(url), ["demo:s3cr3t", "s3cr3t"], "{url}");
        }
    }

    /// Proxy settings often leave the scheme out.
    #[test]
    fn a_url_without_its_scheme_has_its_credentials_too() {
        assert_eq!(
            credentials("demo:s3cr3t@proxy.demo:3128"),
            ["demo:s3cr3t", "s3cr3t"]
        );
        assert_eq!(credentials("tok3n-demo@proxy.demo:3128"), ["tok3n-demo"]);
    }

    #[test]
    fn a_user_name_without_a_password_is_a_credential_too() {
        assert_eq!(credentials("https://tok3n-demo@proxy.demo"), ["tok3n-demo"]);
    }

    /// `http://token:@proxy` is how a token as the user name is often
    /// written: the empty password is no credential, the token is.
    #[test]
    fn a_user_name_with_an_empty_password_is_a_credential_too() {
        assert_eq!(
            credentials("https://tok3n-demo:@proxy.demo"),
            ["tok3n-demo", "tok3n-demo:"]
        );
    }

    #[test]
    fn percent_encoded_credentials_are_found_decoded_and_as_written() {
        assert_eq!(
            credentials("http://demo:p%40ss@proxy.demo"),
            ["demo:p%40ss", "demo:p@ss", "p%40ss", "p@ss"]
        );
        assert_eq!(credentials("http://t%40k@proxy.demo"), ["t%40k", "t@k"]);
    }

    #[test]
    fn credentials_that_are_not_valid_percent_encoding_are_taken_as_they_are() {
        assert_eq!(
            credentials("http://demo:100%@proxy.demo"),
            ["100%", "demo:100%"]
        );
    }

    #[test]
    fn an_at_sign_or_a_slash_in_the_password_stays_in_the_credentials() {
        assert_eq!(
            credentials("http://demo:p@ss@proxy.demo"),
            ["demo:p@ss", "p@ss"]
        );
        assert_eq!(
            credentials("http://demo:ab/cd@proxy.demo"),
            ["ab/cd", "demo:ab/cd"]
        );
    }

    #[test]
    fn a_url_without_a_userinfo_has_no_credentials() {
        for url in [
            "",
            "http://proxy.demo:3128",
            "proxy.demo:3128",
            "http://@proxy.demo",
            "@proxy.demo",
            // The @ is in the path, not in the authority.
            "http://proxy.demo/a@b",
            "http://proxy.demo/p/x:y@z",
            "proxy.demo/a@b",
        ] {
            assert_eq!(credentials(url), Vec::<String>::new(), "{url}");
        }
    }

    #[test]
    fn what_a_proxy_echoes_of_its_credentials_is_masked() {
        let redactor = Redactor::new(&url_credentials("http://demo:p%40ss@proxy.demo:3128"));
        assert_eq!(
            redactor.redact("proxy http://demo:p%40ss@proxy.demo:3128 said 407 for p@ss"),
            "proxy http://***@proxy.demo:3128 said 407 for ***"
        );
    }

    #[test]
    fn a_repository_redactor_masks_the_password_and_the_url_password_in_any_form() {
        let redactor = for_repository("rest:https://demo:p%40ss@nas/b", "hunter2");
        assert_eq!(
            redactor.redact("open rest:https://demo:p%40ss@nas/b with hunter2: p@ss"),
            "open rest:https://demo:***@nas/b with ***: ***"
        );
    }

    /// rustic quotes a URL as the url crate writes it: a raw `@` in the
    /// password comes back as `%40`.
    #[test]
    fn a_raw_password_is_masked_as_the_url_crate_rewrites_it() {
        let redactor = for_repository("rest:https://demo:P@ssw0rd!@nas/b", "hunter2");
        assert_eq!(
            redactor.redact("join https://demo:P%40ssw0rd!@nas/b/config"),
            "join https://demo:***@nas/b/config"
        );
    }

    #[test]
    fn a_repository_with_an_empty_password_redacts_nothing() {
        let redactor = for_repository("/backups/home", "");
        assert_eq!(redactor.redact("nothing to hide"), "nothing to hide");
    }

    /// rclone reads a quoted value without its quotes and with a quote
    /// written twice as one, and may quote it so; the location shows it as
    /// written.
    #[test]
    fn a_credential_written_in_an_rclone_connection_string_is_masked_as_written_and_as_rclone_reads_it()
     {
        let location = "rclone::sftp,host=nas.example.com,user=demo,pass='CANARY,it''s:x':/backups";
        let redactor = for_repository(location, "hunter2");
        assert_eq!(
            redactor.redact(location),
            "rclone::sftp,host=nas.example.com,user=demo,pass=***:/backups"
        );
        assert_eq!(redactor.redact("denied CANARY,it's:x"), "denied ***");
        assert_eq!(redactor.redact("denied CANARY,it''s:x"), "denied ***");
        let location = "rclone::s3,provider=AWS,access_key_id=AKIADEMO,secret_access_key=\"CANARY-s3\":bucket/repo";
        assert_eq!(
            for_repository(location, "hunter2").redact(location),
            "rclone::s3,provider=AWS,access_key_id=AKIADEMO,secret_access_key=***:bucket/repo"
        );
        // A remote's name alone, and a value of characters wider than a byte.
        assert_eq!(
            for_repository("rclone::pass=é€", "hunter2").redact("é€"),
            REDACTED
        );
    }

    #[test]
    fn a_credential_written_in_the_hot_rclone_location_is_masked_too() {
        let mut settings = Settings::new("/srv/demo/repo");
        let hot = "rclone:demo-remote,key=CANARY-b2-key:demo-bucket";
        settings.repo_hot = Some(hot.to_owned());
        assert_eq!(
            settings.redactor().redact(hot),
            "rclone:demo-remote,key=***:demo-bucket"
        );
    }

    /// A reference in an rclone location is refused before it is resolved,
    /// but the location still shows as written; a doubled dollar is text,
    /// which rclone reads as `${`.
    #[test]
    fn an_rclone_credential_with_a_reference_or_an_escape_is_masked_as_written() {
        let location = "rclone::sftp,pass=pre${DEMO_PASS}post:/backups";
        assert_eq!(
            for_repository(location, "hunter2").redact(location),
            "rclone::sftp,pass=***:/backups"
        );
        let location = "rclone::sftp,pass=CANARY$${b}c:/backups";
        let redactor = for_repository(location, "hunter2");
        assert_eq!(redactor.redact(location), "rclone::sftp,pass=***:/backups");
        assert_eq!(redactor.redact("denied CANARY${b}c"), "denied ***");
    }

    /// rclone opens a quote only where a value starts: a quote further on is
    /// text, and the `,` after it ends the value all the same.
    #[test]
    fn a_quote_inside_an_rclone_value_is_text_as_rclone_reads_it() {
        let location = "rclone::s3,pass=a='b,secret_access_key=XX':bucket";
        let redactor = for_repository(location, "hunter2");
        assert_eq!(
            redactor.redact(location),
            "rclone::s3,pass=***,secret_access_key=***:bucket"
        );
        assert_eq!(redactor.redact("denied a='b and XX'"), "denied *** and ***");
    }

    /// A location `depth` deep: each level but the last is a crypt remote
    /// whose `remote` is the next level, quoted; the last is `innermost`.
    fn nested(depth: usize, innermost: &str) -> String {
        (1..depth).fold(innermost.to_owned(), |inner, _| {
            format!(":crypt,remote='{}':p", inner.replace('\'', "''"))
        })
    }

    /// A setting's value may be a location of its own (crypt's `remote`),
    /// which rclone reads as it reads any: its credentials are masked too,
    /// as written, as each level reads it and as rclone reads it last.
    #[test]
    fn a_credential_of_an_rclone_location_written_in_a_setting_is_masked() {
        let location = "rclone::crypt,remote=':s3,secret_access_key=XX:bucket',password=YY:path";
        let redactor = for_repository(location, "hunter2");
        assert_eq!(
            redactor.redact(location),
            "rclone::crypt,remote=':s3,secret_access_key=***:bucket',password=***:path"
        );
        assert_eq!(redactor.redact("denied XX"), "denied ***");
        // A named remote with settings of its own, in which a quoted value
        // holds a quote: each is written twice once more in the location.
        let location = "rclone::crypt,remote='demo,pass=''CANARY,it''''s'':bucket':path";
        let redactor = for_repository(location, "hunter2");
        assert_eq!(
            redactor.redact(location),
            "rclone::crypt,remote='demo,pass=***:bucket':path"
        );
        assert_eq!(
            redactor.redact("open demo,pass='CANARY,it''s':bucket"),
            "open demo,pass=***:bucket"
        );
        assert_eq!(redactor.redact("denied CANARY,it's"), "denied ***");
        // A backend's settings with no path after them are a location too.
        let location = "rclone::crypt,remote=':s3,secret_access_key=CANARY-s3':path";
        assert_eq!(
            for_repository(location, "hunter2").redact("denied CANARY-s3"),
            "denied ***"
        );
    }

    /// Four levels are read; a location deeper than that is masked whole.
    #[test]
    fn an_rclone_location_nested_deeper_than_four_is_masked_whole() {
        let innermost = ":s3,secret_access_key=CANARY-a,session_token=CANARY-b:b";
        let four = format!("rclone:{}", nested(4, innermost));
        let redactor = for_repository(&four, "hunter2");
        assert_eq!(
            redactor.redact(&four),
            format!(
                "rclone:{}",
                nested(4, ":s3,secret_access_key=***,session_token=***:b")
            )
        );
        assert_eq!(redactor.redact("denied CANARY-a"), "denied ***");
        let five = format!("rclone:{}", nested(5, innermost));
        assert_eq!(
            for_repository(&five, "hunter2").redact(&five),
            format!("rclone:{}", nested(4, ":crypt,remote=***:p"))
        );
    }

    /// Only what rclone reads as a location is read as one: a quoted value
    /// with no `:` outside quotes in it is a local path to rclone.
    #[test]
    fn a_value_that_is_no_location_hides_no_credential() {
        let location = "rclone::crypt,remote='demo,pass=visible':path";
        assert_eq!(
            for_repository(location, "hunter2").redact(location),
            location
        );
    }

    #[test]
    fn rclones_own_credential_names_are_masked() {
        for name in [
            "access_grant",
            "master_keys",
            "authorization",
            "headers",
            "2fa",
        ] {
            let location = format!("rclone::demo,{name}='CANARY,{name}':bucket");
            assert_eq!(
                for_repository(&location, "hunter2").redact(&location),
                format!("rclone::demo,{name}=***:bucket")
            );
        }
    }

    /// rustic's OpenDAL SFTP backend reads its option `key` as the path of
    /// a key file: it stays readable, where an rclone setting `key` doesn't.
    #[test]
    fn a_rustic_sftp_key_option_is_not_masked() {
        let mut settings = Settings::new("opendal:sftp");
        settings.options = options(&[("key", "/home/demo/.ssh/id_demo")]);
        assert_eq!(
            settings
                .redactor()
                .redact("no key at /home/demo/.ssh/id_demo"),
            "no key at /home/demo/.ssh/id_demo"
        );
    }

    #[test]
    fn what_is_no_rclone_credential_is_not_masked() {
        for location in [
            "rclone::sftp,host=nas.example.com,key_file=/home/demo/.ssh/id,bypass_cache=true:/backups",
            // What follows the first `:` outside quotes is the path.
            "rclone:demo-remote:backups/pass=visible",
            "rclone::sftp,pass='':/backups",
            "rclone::sftp,pass=:/backups",
            "/srv/pass=visible",
            "local:rclone::sftp,pass=visible",
        ] {
            assert_eq!(
                for_repository(location, "hunter2").redact(location),
                location
            );
        }
    }

    /// Each credential is a secret of its own to look for in every text, so
    /// a location with more than 16 of them is masked whole instead.
    #[test]
    fn an_rclone_location_with_more_credentials_than_are_read_is_masked_whole() {
        let location = |count: usize| {
            let settings: String = (0..count)
                .map(|n| format!(",secret{n}=CANARY{n}"))
                .collect();
            format!("rclone::demo{settings}:/backups")
        };
        let sixteen = location(16);
        let shown = for_repository(&sixteen, "hunter2").redact(&sixteen);
        assert!(
            shown.starts_with("rclone::demo,secret0=***,secret1=***") && !shown.contains("CANARY"),
            "{shown}"
        );
        let seventeen = location(17);
        assert_eq!(
            for_repository(&seventeen, "hunter2").redact(&seventeen),
            REDACTED
        );
    }

    #[test]
    fn hostile_rclone_locations_end_without_a_panic() {
        // One pass over each is milliseconds; a reading quadratic in the
        // length, or a needle for every one of thousands of settings, would
        // run past the test runner's time limit.
        let many: String = (0..20_000).map(|n| format!(",pass=v{n}")).collect();
        let long = [
            format!("rclone::demo{many}:/backups"),
            format!("rclone::demo{}", ",secret=x".repeat(100_000)),
            format!("rclone::sftp,pass='{}", "'',".repeat(100_000)),
            format!("rclone::sftp,pass={}", "${A}".repeat(50_000)),
            format!("rclone::sftp,pass=\"{}\":/b", "é".repeat(100_000)),
            format!("rclone:{}", "$${".repeat(100_000)),
            format!("rclone::crypt{}:/b", ",remote=':a,pass=v:b'".repeat(20_000)),
            format!("rclone::crypt,remote='{}':/b", "''".repeat(100_000)),
            format!("rclone::crypt,remote=':a,x={}':/b", "''''".repeat(50_000)),
        ];
        let short = [
            "rclone:",
            "rclone::",
            "rclone:,",
            "rclone:=",
            "rclone:,=,",
            "rclone::pass='",
            "rclone::pass=''",
            "rclone::pass='''",
            "rclone::pass=\"'\"",
            "rclone::pass=${A}",
            "rclone::pass=$${",
            "rclone::${A}=x",
            "rclone::pass=é€",
        ];
        for location in long.iter().map(String::as_str) {
            let _ = for_repository(location, "hunter2").redact("a short text");
        }
        for location in short {
            let _ = for_repository(location, "hunter2").redact(location);
        }
    }

    fn options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|&(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    #[test]
    fn a_repository_redactor_masks_the_value_of_every_option_that_holds_a_credential() {
        let mut settings = Settings::new("opendal:azblob");
        settings.options = options(&[
            ("sas_token", "sv=2024&sig=abc123"),
            ("secret_access_key", "AKIA-demo-secret"),
            ("application_key_id", "0021a7c1"),
            ("bucket", "demo-bucket"),
        ]);
        let redactor = settings.redactor();
        assert_eq!(
            redactor.redact(
                "url: http://nas/demo-bucket/config?sv=2024&sig=abc123 key AKIA-demo-secret id 0021a7c1"
            ),
            "url: http://nas/demo-bucket/config?*** key *** id 0021a7c1"
        );
    }

    #[test]
    fn a_repository_redactor_masks_what_the_references_in_its_settings_resolved_to() {
        let mut settings = Settings::new("opendal:s3");
        settings.resolved = vec![
            SecretValue::new("canary-from-a-file"),
            SecretValue::new("canary-from-env"),
        ];
        let redactor = settings.redactor();
        assert_eq!(
            redactor.redact("GET https://canary-from-env/canary-from-a-file/config failed"),
            "GET https://***/***/config failed"
        );
    }

    /// An endpoint isn't a secret, but a URL may carry credentials in its
    /// userinfo, and OpenDAL's errors quote the URL it built from it.
    #[test]
    fn a_repository_redactor_masks_the_password_of_a_url_an_option_holds() {
        let mut settings = Settings::new("opendal:s3");
        settings.options = options(&[
            ("endpoint", "https://demo:s3cr3t@minio.example:9000"),
            ("root", "/backups/home"),
        ]);
        let redactor = settings.redactor();
        assert_eq!(
            redactor.redact("url: https://demo:s3cr3t@minio.example:9000/demo/config failed"),
            "url: https://demo:***@minio.example:9000/demo/config failed"
        );
    }

    /// The control is the third URL: without it, a redactor that looks at no
    /// option would pass this test too.
    #[test]
    fn an_option_url_without_a_password_hides_nothing() {
        let mut settings = Settings::new("opendal:s3");
        settings.options = options(&[
            ("endpoint", "https://demo@minio.example:9000/data"),
            ("rest-url", "https://minio.example:9000"),
            ("proxy", "https://demo:s3cr3t@proxy.example:3128"),
        ]);
        let redactor = settings.redactor();
        assert_eq!(
            redactor.redact(
                "https://demo@minio.example:9000/data/demo/config via https://demo:s3cr3t@proxy.example:3128"
            ),
            "https://demo@minio.example:9000/data/demo/config via https://demo:***@proxy.example:3128"
        );
    }

    /// A hot/cold repository is opened through both halves, and the errors
    /// of rustic and of OpenDAL quote whichever half failed: what only the
    /// hot half or only the cold half holds is hidden like the rest.
    #[test]
    fn a_repository_redactor_masks_the_password_in_the_url_of_the_hot_repository() {
        let mut settings = Settings::new("/backups/cold");
        settings.repo_hot = Some("rest:https://demo:hot-s3cr3t@nas:8000/hot".to_owned());
        assert_eq!(
            settings
                .redactor()
                .redact("URL `rest:https://demo:hot-s3cr3t@nas:8000/hot/` parsing failed"),
            "URL `rest:https://demo:***@nas:8000/hot/` parsing failed"
        );
    }

    #[test]
    fn a_repository_redactor_masks_the_value_of_every_hot_and_cold_option_that_holds_a_credential()
    {
        let mut settings = Settings::new("opendal:s3");
        settings.repo_hot = Some("opendal:s3".to_owned());
        settings.options_hot = options(&[
            ("secret_access_key", "AKIA-hot-secret"),
            ("region", "eu-hot-1"),
        ]);
        settings.options_cold =
            options(&[("sas_token", "sv=cold&sig=xyz789"), ("region", "eu-cold-1")]);
        assert_eq!(
            settings
                .redactor()
                .redact("hot AKIA-hot-secret in eu-hot-1, cold sv=cold&sig=xyz789 in eu-cold-1"),
            "hot *** in eu-hot-1, cold *** in eu-cold-1"
        );
    }

    #[test]
    fn a_repository_redactor_masks_the_password_of_a_url_a_hot_or_cold_option_holds() {
        let mut settings = Settings::new("opendal:s3");
        settings.repo_hot = Some("opendal:s3".to_owned());
        settings.options_hot = options(&[("endpoint", "https://demo:hot-s3cr3t@hot.example:9000")]);
        settings.options_cold =
            options(&[("endpoint", "https://demo:cold-s3cr3t@cold.example:9000")]);
        assert_eq!(
            settings.redactor().redact(
                "https://demo:hot-s3cr3t@hot.example:9000/config, https://demo:cold-s3cr3t@cold.example:9000/keys"
            ),
            "https://demo:***@hot.example:9000/config, https://demo:***@cold.example:9000/keys"
        );
    }

    /// A text cut at a cap can end in the beginning of a secret, which no
    /// needle matches whole: that end is masked as the secret would be, and
    /// a whole secret just before the cut is masked whole (I4).
    #[test]
    fn a_secret_a_cut_went_through_is_masked_and_so_is_one_just_before_it() {
        let masked = redactor(&["hunter2-canary"]);
        // The cut went through it.
        assert_eq!(masked.redact_cut(b"rejected hunter2-ca"), "rejected ***");
        assert_eq!(masked.redact_cut(b"rejected h"), "rejected ***");
        // Longer than what was kept.
        assert_eq!(masked.redact_cut(b"hunter2"), "***");
        // Whole, and ending just before the cut, and then cut again.
        assert_eq!(masked.redact_cut(b"x hunter2-canary!"), "x ***!");
        assert_eq!(masked.redact_cut(b"x hunter2-canary hunter"), "x *** ***");
        // Nothing of a secret at the end: the text as it is.
        assert_eq!(masked.redact_cut(b"nothing here"), "nothing here");
        assert_eq!(redactor(&[]).redact_cut(b"nothing here"), "nothing here");
    }

    /// Bytes cut inside a character: the part of it that was kept is not
    /// shown, and the secret it began is still masked.
    #[test]
    fn a_cut_inside_a_character_of_a_secret_still_masks_it() {
        let masked = redactor(&["pässwort-canary"]);
        assert_eq!(masked.redact_cut(b"key p\xC3"), "key ***");
        assert_eq!(masked.redact_cut(b"key \xC3"), "key ");
        assert_eq!(masked.redact_cut("key pä".as_bytes()), "key ***");
        // A byte that is no character, before the end, is shown as one that
        // can't be read, and the text goes on after it.
        assert_eq!(
            masked.redact_cut(b"bad \xFF here p\xC3"),
            "bad \u{FFFD} here ***"
        );
    }

    /// Someone who knew fewer secrets cut a text and marked the cut: what it
    /// kept is masked as any cut text is, and the mark stays.
    #[test]
    fn a_text_marked_as_cut_is_masked_as_a_cut_text_and_keeps_its_mark() {
        let masked = redactor(&["hunter2-canary"]);
        assert_eq!(masked.redact_clipped("said hunter2-ca…", "…"), "said ***…");
        // Not marked: only what is whole is a secret.
        assert_eq!(
            masked.redact_clipped("said hunter2-canary h", "…"),
            "said *** h"
        );
    }

    /// A secret that ends in what looks like the mark is a whole secret
    /// there: it is masked with its last character, as `redact` masks it,
    /// and no mark is put back after it (I4).
    #[test]
    fn a_secret_that_ends_in_the_mark_is_masked_whole_with_it() {
        let masked = redactor(&["hunter2-canary…"]);
        assert_eq!(
            masked.redact_clipped("said hunter2-canary…", "…"),
            "said ***"
        );
        // Cut in it and marked: what was kept of it is masked, and the mark
        // stays.
        assert_eq!(masked.redact_clipped("said hunter2-ca…", "…"), "said ***…");
    }

    /// A worker's stderr reaches the daemon a line at a time, so no needle
    /// of a whole key could match any of it.
    #[test]
    fn a_secret_that_spans_lines_is_masked_line_by_line_too() {
        let key = "-----BEGIN KEY-----\r\n  MIIEvQIBADANBgkqhkiG9w0B  \nshort\nAQEFAASCBKcwggSj\n-----END KEY-----\n";
        let redactor = redactor(&[key]);
        for line in [
            "-----BEGIN KEY-----",
            "MIIEvQIBADANBgkqhkiG9w0B",
            "AQEFAASCBKcwggSj",
            "-----END KEY-----",
        ] {
            assert_eq!(
                redactor.redact(&format!("rclone: {line} failed")),
                "rclone: *** failed"
            );
        }
        // The whole text is still replaced as one.
        assert_eq!(redactor.redact(&format!("[{key}]")), "[***]");
        // A line too short to stand for the secret doesn't mask other text.
        assert_eq!(redactor.redact("a short line"), "a short line");
    }

    #[test]
    fn a_line_of_a_secret_is_a_needle_of_its_own_from_eight_bytes_on() {
        let redactor = redactor(&["1234567\n12345678\nabcdefgh"]);
        assert_eq!(redactor.redact("7: 1234567"), "7: 1234567");
        assert_eq!(redactor.redact("8: 12345678"), "8: ***");
        assert_eq!(redactor.redact("9: abcdefgh"), "9: ***");
    }

    #[test]
    fn only_the_first_lines_of_a_very_long_secret_are_needles_of_their_own() {
        let lines: Vec<String> = (0..70).map(|n| format!("line-number-{n:03}")).collect();
        let redactor = redactor(&[lines.join("\n").as_str()]);
        assert_eq!(redactor.redact("line-number-063"), "***");
        assert_eq!(redactor.redact("line-number-064"), "line-number-064");
    }

    /// `${VAR}` gives a value as it is, and a log line doesn't have its
    /// newline.
    #[test]
    fn the_white_space_around_a_secret_is_masked_without_it() {
        let redactor = redactor(&["hunter2\n", "  pad  "]);
        assert_eq!(
            redactor.redact("login hunter2 failed, pad."),
            "login *** failed, ***."
        );
        // The secret with its white space is replaced as a whole.
        assert_eq!(redactor.redact("[hunter2\n]"), "[***]");
        // A secret of nothing but white space has no trimmed form.
        let blank = super::Redactor::new(&[SecretValue::new(" \n ")]);
        assert_eq!(blank.redact("a b"), "a b");
        assert_eq!(blank.redact("x \n y"), "x***y");
    }

    /// What `work` gives, and how many bytes of a needle and the text a
    /// redactor compared while doing it, on this thread.
    fn compared<T>(work: impl FnOnce() -> T) -> (T, usize) {
        let before = COMPARED.with(std::cell::Cell::get);
        let done = work();
        (done, COMPARED.with(std::cell::Cell::get) - before)
    }

    /// Masking compares each byte of the text at least once and at most
    /// twice per needle, however long and repetitive a secret and the text
    /// are: the work grows with the length of the text, never with the
    /// length of a needle (#236). A count, not a time, so it can't flake.
    #[test]
    fn masking_a_text_compares_each_byte_at_most_twice_per_needle() {
        let cap = MAX_SECRET_BYTES;
        // A password half the cap long, one byte over and over, then the
        // characters a URL parser writes in different ways: dozens of
        // needles that share their first 32 KiB, all of it in the text at
        // every byte. And a location at the cap whose password is the same.
        let mut settings =
            Settings::new(&format!("rest:https://demo:{}@nas/", "a".repeat(cap - 23)));
        assert_eq!(settings.repository.len(), cap);
        settings.password =
            SecretValue::new(format!("{}\"`#'^{{}}?/:@\\ <>|", "a".repeat(cap / 2)));
        let redactor = settings.redactor();
        let needles = redactor.needles.len();
        assert!(needles > 40, "{needles}");
        let text = "a".repeat(cap);
        let clipped = format!("{text}…");
        for (what, length, (masked, work), expected) in [
            ("whole", cap, compared(|| redactor.redact(&text)), "***"),
            (
                "cut",
                cap,
                compared(|| redactor.redact_cut(text.as_bytes())),
                "***",
            ),
            (
                "clipped",
                clipped.len(),
                compared(|| redactor.redact_clipped(&clipped, "…")),
                "***…",
            ),
        ] {
            assert_eq!(masked, expected, "{what}");
            assert!(
                (length * needles..=2 * length * needles).contains(&work),
                "{what}: {work} comparisons for {length} bytes and {needles} needles"
            );
        }
    }

    /// A needle longer than a `u32` holds can't be built in a test (4 GiB),
    /// so this builds the redactor such a needle leaves: it masks every text
    /// that isn't empty whole, cut or not.
    #[test]
    fn a_redactor_with_a_needle_too_long_to_search_masks_every_text_whole() {
        let redactor = Redactor {
            needles: Vec::new(),
            whole: true,
        };
        assert_eq!(redactor.redact(""), "");
        assert_eq!(redactor.redact("x"), REDACTED);
        assert_eq!(redactor.redact_cut(b"x"), REDACTED);
        assert_eq!(redactor.redact_clipped("x…", "…"), REDACTED);
    }

    /// Distinct secrets of `total` bytes, each of its own letter.
    fn secrets_of(total: usize) -> Vec<SecretValue> {
        let count = total.div_ceil(MAX_SECRET_BYTES);
        (0..count)
            .map(|at| {
                let share = total / count + usize::from(at < total % count);
                let letter = char::from(b'a' + u8::try_from(at).unwrap());
                SecretValue::new(letter.to_string().repeat(share))
            })
            .collect()
    }

    /// How many secrets a redactor built the forms of while doing `work`,
    /// on this thread.
    fn formed<T>(work: impl FnOnce() -> T) -> (T, usize) {
        let before = FORMED.with(std::cell::Cell::get);
        let done = work();
        (done, FORMED.with(std::cell::Cell::get) - before)
    }

    /// Up to the cap, a redactor masks each secret; past it, it builds no
    /// form of any (each about 50 times its size) and masks every text
    /// whole: more than a secret, never less (I4).
    #[test]
    fn a_redactor_builds_no_forms_of_secrets_past_the_cap() {
        let at_the_cap = secrets_of(MAX_MASKED_BYTES);
        assert_eq!(
            at_the_cap.iter().map(|s| s.expose().len()).sum::<usize>(),
            MAX_MASKED_BYTES
        );
        let (redactor, built) = formed(|| Redactor::new(&at_the_cap));
        assert_eq!(built, at_the_cap.len());
        assert!(!redactor.masks_everything());
        assert_eq!(redactor.redact("a b"), "a b");
        assert_eq!(
            redactor.redact(&format!("[{}]", at_the_cap[0].expose())),
            "[***]"
        );

        let over = secrets_of(MAX_MASKED_BYTES + 1);
        let (redactor, built) = formed(|| Redactor::new(&over));
        assert_eq!(built, 0);
        assert!(redactor.masks_everything());
        assert_eq!(redactor.redact("a b"), REDACTED);
        assert_eq!(redactor.redact(""), "");
    }

    /// The cap counts each secret once, however often it is given, and an
    /// empty one not at all.
    #[test]
    fn a_secret_given_twice_counts_once_toward_the_cap() {
        let mut secrets = secrets_of(MAX_MASKED_BYTES);
        secrets.push(secrets[0].clone());
        secrets.push(SecretValue::new(""));
        assert!(!Redactor::new(&secrets).masks_everything());
    }

    #[test]
    fn a_redactor_masking_everything_masks_every_text_that_is_not_empty_whole() {
        let redactor = Redactor::masking_everything();
        assert!(redactor.masks_everything());
        assert_eq!(redactor.redact(""), "");
        assert_eq!(redactor.redact("x"), REDACTED);
        assert_eq!(redactor.redact_cut(b"x"), REDACTED);
        assert!(!Redactor::default().masks_everything());
    }

    /// What the count takes from a location and an option is what a
    /// redactor for the repository is built from, so a repository the count
    /// lets through gets a redactor that masks each secret.
    #[test]
    fn the_count_takes_from_each_setting_what_a_redactor_masks() {
        let mut count = MaskedSecrets::default();
        let mut secrets = secrets_of(MAX_MASKED_BYTES - 19);
        for secret in &secrets {
            assert_eq!(count.secret(secret), Ok(()));
        }
        // Counted already, and not a credential by its name: nothing more.
        assert_eq!(count.secret(&secrets[0]), Ok(()));
        assert_eq!(count.option("region", "eu-demo-1-not-a-secret"), Ok(()));
        assert_eq!(count.location("/srv/demo/repo"), Ok(()));
        // 8 bytes of a URL's password, 6 of an rclone credential, 5 of a
        // credential option: the cap.
        assert_eq!(count.location("rest:https://demo:pw-12345@nas/"), Ok(()));
        assert_eq!(count.location("rclone::sftp,pass=rc-123:/b"), Ok(()));
        assert_eq!(count.option("secret_access_key", "sk-12"), Ok(()));
        let mut settings = Settings::new("rest:https://demo:pw-12345@nas/");
        settings.repo_hot = Some("rclone::sftp,pass=rc-123:/b".to_owned());
        settings.password = secrets.remove(0);
        settings.resolved = secrets;
        settings
            .options
            .insert("secret_access_key".to_owned(), "sk-12".to_owned());
        assert!(!settings.redactor().masks_everything());
        // One byte more, in a URL in an option: past the cap, for both.
        assert_eq!(
            count.option("endpoint", "https://u:x@h"),
            Err(TooMuchToMask)
        );
        assert_eq!(count.secret(&SecretValue::new("")), Err(TooMuchToMask));
        settings
            .options
            .insert("endpoint".to_owned(), "https://u:x@h".to_owned());
        assert!(settings.redactor().masks_everything());
    }

    /// A repository is counted with every secret a redactor for it masks:
    /// any one of them, of any setting, takes it past the cap.
    #[test]
    fn a_repository_is_counted_with_every_secret_its_redactor_masks() {
        let at_the_cap = || {
            let mut settings = Settings::new("/srv/demo/repo");
            settings.password = SecretValue::new("");
            settings.resolved = secrets_of(MAX_MASKED_BYTES - 1);
            settings
        };
        let counted = |settings: &Settings| MaskedSecrets::default().repository(settings.secrets());
        assert_eq!(counted(&at_the_cap()), Ok(()));
        type Add = fn(&mut Settings);
        let parts: [(&str, Add); 7] = [
            ("password", |s| s.password = SecretValue::new("p1")),
            ("repository", |s| {
                s.repository = "rest:https://demo:p2@nas/".to_owned();
            }),
            ("repo-hot", |s| {
                s.repo_hot = Some("rclone::sftp,pass=p3:/b".to_owned());
            }),
            ("options", |s| {
                s.options
                    .insert("secret_access_key".to_owned(), "p4".to_owned());
            }),
            ("options-hot", |s| {
                s.options_hot.insert("password".to_owned(), "p5".to_owned());
            }),
            ("options-cold", |s| {
                s.options_cold
                    .insert("endpoint".to_owned(), "https://u:p6@h".to_owned());
            }),
            ("resolved", |s| s.resolved.push(SecretValue::new("p7"))),
        ];
        for (part, add) in parts {
            let mut settings = at_the_cap();
            add(&mut settings);
            assert_eq!(counted(&settings), Err(TooMuchToMask), "{part}");
            assert!(settings.redactor().masks_everything(), "{part}");
        }
    }
}
