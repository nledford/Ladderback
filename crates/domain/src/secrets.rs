//! Secrets context: resolved secret values and the redactor that keeps them
//! out of logs, warnings and errors (invariant I4, technical design §10.2).

use std::collections::BTreeMap;

use zeroize::{Zeroize, ZeroizeOnDrop};

mod command;
mod go_print;
mod head;
mod inline;
mod options;
mod outcome;
#[cfg(test)]
mod properties;
mod proxy;
mod source;
mod template;
mod toml_read;

pub use command::{CommandLine, CommandLineError};
pub(crate) use inline::MAX_RCLONE_DEPTH;
pub(crate) use inline::url_has_userinfo;
use inline::{AuthorityEnd, Scheme, Userinfo, WrittenUrl, rclone_credentials};
pub use inline::{
    InlineSecret, MAX_RCLONE_CREDENTIALS, location_secret_written, rclone_location,
    too_many_rclone_credentials, url_password_written,
};
pub(crate) use options::is_rclone_secret_flag;
use options::is_rclone_secret_option;
pub use options::{is_command_option, is_secret_option};
pub use outcome::{
    KeyringCause, MAX_DETAIL_BYTES, Problem, SecretError, SecretWarning, Stdout, command_failure,
    command_secret, decode, file_warning,
};
pub use proxy::{PROXY_URL_VARIABLES, proxy_credentials, proxy_credentials_by_variable};
pub use source::{FileRead, KeyringEntry, NotAKeyringEntry, SecretRef, SourceName, first_line};
pub use template::{Part, Reference, Template, TemplateError, has_reference, only_references};

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
/// however many places hold it: a secret gives up to about 80 bytes of
/// needles per byte of it, which take about 400 bytes of memory, so this
/// keeps one redactor under about 100 MiB. The secrets of a real repository
/// come to a few kilobytes (technical design §10.2).
pub const MAX_MASKED_BYTES: usize = 256 * 1024;

/// A resolved secret, such as a repository password. Its `Debug` form never
/// shows the value, so it can't leak through a log or a test failure, and it
/// wipes its bytes when it is dropped, each clone its own (§10.2).
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
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

impl RepositorySecrets<'_> {
    /// Every secret of the repository a redactor masks, as
    /// [`Redactor::for_repository`] describes them, so a redactor can be
    /// built from the secrets of several repositories at once.
    pub fn values(self) -> Vec<SecretValue> {
        let mut values = vec![self.password.clone()];
        values.extend(self.derived());
        values.extend_from_slice(self.resolved);
        values
    }

    /// What the locations and the options hold that a redactor masks.
    fn derived(self) -> Vec<SecretValue> {
        std::iter::once(self.repository)
            .chain(self.repo_hot)
            .flat_map(location_secrets)
            .chain(
                [self.options, self.options_hot, self.options_cold]
                    .into_iter()
                    .flatten()
                    .flat_map(|(name, value)| option_secrets(name, value)),
            )
            .collect()
    }
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

    /// Counts the secrets a location holds: what of its URL's userinfo a
    /// redactor masks, and the credentials of an rclone connection string.
    pub fn location(&mut self, location: &str) -> Result<(), TooMuchToMask> {
        location_secrets(location)
            .iter()
            .try_for_each(|secret| self.secret(secret))
    }

    /// Counts the secrets an option holds: what of the userinfo of a URL in
    /// it a redactor masks, and its whole value when its name says it holds
    /// a credential.
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
/// it, or its userinfo where a reference gives it ([`url_secrets`]), and
/// each credential of an rclone connection string.
fn location_secrets(location: &str) -> Vec<SecretValue> {
    let mut secrets = url_secrets(location);
    secrets.extend(rclone_credentials(location));
    secrets
}

/// What an option holds that a redactor masks: what of a URL in its value
/// does ([`url_secrets`]), and the whole value when its name says it holds
/// a credential ([`is_secret_option`]), even one written inline.
fn option_secrets(name: &str, value: &str) -> Vec<SecretValue> {
    let mut secrets = url_secrets(value);
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
/// else of it shows in text on its own (see `partial_needles`); and as a
/// TOML file writes it, found in the text as TOML reads its escapes (see
/// `toml_read`); and the start of a text, after its white space, where it
/// is the last 4 bytes or more of a needle, as written or as TOML reads it:
/// the rest of a secret a line-ending backslash broke (see `head`). Secrets
/// that overlap or touch are replaced as one span. Masking a text compares
/// each of its bytes at most twice per needle in each pass: the text and
/// its start, and the same again as TOML reads it where it holds a TOML
/// escape, is cut with a backslash in it or ends in a line-ending
/// backslash.
///
/// Masking a masked text again is not always idempotent: when a secret holds
/// `*`, the character of [`REDACTED`], a mask and the text beside it can make
/// up the secret, and a second pass masks that too (secret `*?`: `*??` masks
/// to `***?`, and that to `*****`). So can a mask that ends between the two
/// backslashes of `\\`: the one left after it can begin a TOML escape of
/// another secret (secrets `\u0041Z\` and `\u{7}bell`: `x \u0041Z\\x07bell`
/// masks to `x ***\x07bell`, and that to `x ******`). A second pass only
/// ever masks more, never reveals what the first masked (I4). Matches inside
/// a mask are not skipped: a real `*` beside a secret would then leave part
/// of it unmasked (#287).
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
        let mut borders = Vec::new();
        fill_borders(text.as_bytes(), &mut borders)?;
        Some(Self { text, borders })
    }
}

/// Fills `borders` with the borders of `bytes`, as a [`Needle`] keeps them;
/// `None` when it is longer than `u32::MAX` bytes, whose borders a `u32`
/// couldn't hold. Filled in place rather than returned, so no mutant of
/// it can hand a search a border as long as what it is the border of,
/// which the search would never leave.
fn fill_borders(bytes: &[u8], borders: &mut Vec<u32>) -> Option<()> {
    u32::try_from(bytes.len()).ok()?;
    borders.reserve_exact(bytes.len());
    if let Some((_, rest)) = bytes.split_first() {
        // The first byte has no start shorter than itself.
        borders.push(0);
        let mut matched = 0;
        for &byte in rest {
            matched = advance(bytes, borders, matched, byte);
            borders.push(matched);
        }
    }
    Some(())
}

/// Marks bytes `from` to `to` of a text as one span in `spans`, the longest
/// span that ends at each byte of it (see `Redactor::mask_with`).
fn cover(spans: &mut [u32], from: usize, to: usize) {
    if let Some(span) = to.checked_sub(1).and_then(|last| spans.get_mut(last)) {
        *span = (*span).max(place(to.saturating_sub(from)));
    }
}

/// A place in a text of at most `u32::MAX` bytes.
fn place(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
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
        COMPARED.update(|count| count + 1);
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
    /// 80 times the secret's size): the redactor masks every text whole.
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
                FORMED.update(|count| count + 1);
                let raw = partial_needles(value);
                let escaped: Vec<String> = std::iter::once(value)
                    .chain(raw.iter().map(String::as_str))
                    .flat_map(|form| debug_escaped(form).into_iter().chain(go_quoted(form)))
                    .collect();
                url_needles(value).into_iter().chain(raw).chain(escaped)
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
        let derived = repository.derived();
        Self::new(
            std::iter::once(repository.password)
                .chain(&derived)
                .chain(repository.resolved),
        )
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

    /// `text` masked with the secrets of every one of `redactors` at once,
    /// as one redactor of all their secrets would mask it: a secret of one
    /// that holds or overlaps a secret of another is masked whole, where
    /// masking with each in turn leaves what the first one's mask broke up
    /// of the other's (#287). One of them that masks everything masks every
    /// text that isn't empty whole.
    pub fn redact_together<'a>(
        redactors: impl IntoIterator<Item = &'a Redactor>,
        text: &str,
    ) -> String {
        let mut whole = false;
        let mut needles = Vec::new();
        for redactor in redactors {
            whole |= redactor.whole;
            needles.extend(&redactor.needles);
        }
        Self::mask_with(needles.iter().copied(), whole, text, None)
    }

    /// `text` masked with this redactor's needles ([`Redactor::mask_with`]).
    fn mask(&self, text: &str, cut: Option<usize>) -> String {
        Self::mask_with(self.needles.iter(), self.whole, text, cut)
    }

    /// `text` with every one of `needles` in it replaced, or the whole of it
    /// where `everything` says so and it isn't empty. When `cut` is given, the
    /// text was cut after its first `cut` bytes, and the end of those from
    /// where it is the start of a needle on (the earliest start of a secret
    /// the cut could have gone through) is part of the same spans. The start
    /// of the text, after its white space, is part of them too where it is
    /// the end of a needle, [`head::MIN_TAIL_BYTES`] or more of it.
    ///
    /// One pass over the text per needle, and one over the text as TOML reads
    /// it where it holds an escape, is cut with a backslash in it or ends in
    /// a line-ending backslash: each byte of the text is compared at most
    /// twice with a byte of the needle in each pass, however long and
    /// repetitive both are. For each start of the text, and of what TOML
    /// read, one pass over as many of the last bytes of each needle as the
    /// start has, each compared at most twice, after one over the start
    /// itself ([`head::Head`]); a text has a second start only where it
    /// begins with white space TOML keeps. A needle and a valid text are
    /// UTF-8, so what matches starts and ends between characters.
    fn mask_with<'a>(
        needles: impl Iterator<Item = &'a Needle> + Clone,
        everything: bool,
        text: &str,
        cut: Option<usize>,
    ) -> String {
        if everything && !text.is_empty() {
            return REDACTED.to_owned();
        }
        // A place in the text as TOML reads it is kept in a `u32`: a longer
        // text that holds an escape is masked whole, more than a secret,
        // never less (I4).
        if u32::try_from(text.len()).is_err() && text.contains('\\') {
            return REDACTED.to_owned();
        }
        let toml = toml_read::TomlRead::of(text, cut);
        // What is longer than every needle can't be the end of one.
        let longest = needles
            .clone()
            .map(|needle| needle.text.len())
            .max()
            .unwrap_or_default();
        // Where the white space the text begins with ends: as TOML drops it
        // after a line-ending backslash (spaces, tabs and line breaks), and,
        // where that differs, as Unicode has it. Other white space may begin
        // the rest of a secret, or stand before it, and both are looked at.
        // Neither holds a backslash, so it ends at the same place in what
        // TOML read.
        let toml_white = text
            .len()
            .saturating_sub(text.trim_start_matches([' ', '\t', '\r', '\n']).len());
        let unicode_white = text.len().saturating_sub(text.trim_start().len());
        let ats = [
            Some(toml_white),
            (unicode_white != toml_white).then_some(unicode_white),
        ];
        let starts = ats.map(|at| at.and_then(|at| head::Head::of(text, at, longest)));
        let toml_starts = ats.map(|at| at.and_then(|at| toml.as_ref()?.head(at, longest)));
        let bytes = text.as_bytes();
        // The longest span, of a needle or of the cut, that ends at each byte:
        // at most a needle's length, which a `u32` holds, or the bytes it was
        // read from as TOML reads its escapes.
        let mut spans = vec![0_u32; bytes.len()];
        for needle in needles {
            let whole = needle.text.as_bytes();
            let mut matched = 0;
            for (at, (&byte, span)) in bytes.iter().zip(spans.iter_mut()).enumerate() {
                matched = advance(whole, &needle.borders, matched, byte);
                if matched as usize == whole.len() || cut == Some(at + 1) {
                    *span = (*span).max(matched);
                }
            }
            for start in starts.iter().flatten() {
                if let Some(tail) = start.tail(needle) {
                    cover(&mut spans, tail.start, tail.end);
                }
            }
            if let Some(toml) = &toml {
                toml.mask(needle, &mut spans);
                for start in toml_starts.iter().flatten() {
                    toml.mask_tail(start, needle, &mut spans);
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

/// A secret, or a raw form of it, as its `Debug` form prints it between the
/// quotes (`"` and `\` escaped, a control or combining character as
/// `\u{…}`), where that differs from it: a library that logs a value with
/// `{:?}` prints a secret in it so, as rustic logs the command it starts
/// rclone with, the location last (#330). `Debug` escapes each character on
/// its own, so this form is in the printed text wherever the secret is in
/// the text printed. The encoded forms of a secret are not escaped: they
/// would add up to as many needles again, for a secret that a parser would
/// first have to encode and a library then print with `{:?}`.
fn debug_escaped(secret: &str) -> Option<String> {
    let quoted = format!("{secret:?}");
    let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
    (inner != secret).then(|| inner.to_owned())
}

/// A secret, or a raw form of it, as Go's `strconv.Quote` writes it between
/// the quotes, where that differs from it: rclone, written in Go, quotes
/// what it can't serve as `%q` does (`Failed to create file system for
/// "…"`), and the worker and the daemon log what it writes (#331). Go
/// escapes `"` and `\`, writes `\a`, `\b`, `\f`, `\n`, `\r`, `\t` and `\v`
/// for those, and any other character it doesn't print ([`go_prints`]) as
/// `\x01` below the space and for DEL, else as `\u200b` or `\U000e0001`.
/// Like `Debug`, it escapes each character on its own, so this form is in
/// the quoted text wherever the secret is in the text quoted. Where it is
/// the `Debug` form (a secret whose only escaped characters are `"`, `\`,
/// tab or a line break), `Redactor::new` drops the duplicate.
fn go_quoted(secret: &str) -> Option<String> {
    use std::fmt::Write;
    let mut quoted = String::with_capacity(secret.len());
    for c in secret.chars() {
        let code = u32::from(c);
        // Writing to a `String` can't fail.
        let _ = match c {
            '"' | '\\' => write!(quoted, "\\{c}"),
            '\u{7}' => quoted.write_str("\\a"),
            '\u{8}' => quoted.write_str("\\b"),
            '\u{c}' => quoted.write_str("\\f"),
            '\n' => quoted.write_str("\\n"),
            '\r' => quoted.write_str("\\r"),
            '\t' => quoted.write_str("\\t"),
            '\u{b}' => quoted.write_str("\\v"),
            _ if go_prints(c) => quoted.write_char(c),
            '\0'..='\u{1f}' | '\u{7f}' => write!(quoted, "\\x{code:02x}"),
            '\u{80}'..='\u{ffff}' => write!(quoted, "\\u{code:04x}"),
            _ => write!(quoted, "\\U{code:08x}"),
        };
    }
    (quoted != secret).then_some(quoted)
}

/// Whether Go's `strconv.Quote` writes `c` as it is (`"` and `\` aside):
/// whether `unicode.IsPrint` takes it, a letter, mark, number, punctuation
/// or symbol, or the space, by the Unicode version of the Go that rclone
/// is built with. [`go_print::GO_PRINT`] holds those characters, generated
/// from that version's tables, so the answer is Go's whatever Rust
/// toolchain builds Ladderback (technical design §10.2).
fn go_prints(c: char) -> bool {
    let code = u32::from(c);
    // The first range that doesn't end before `c` holds it, if any does.
    let at = go_print::GO_PRINT.partition_point(|&(_, last)| last < code);
    go_print::GO_PRINT
        .get(at)
        .is_some_and(|&(first, _)| first <= code)
}

/// Every password the userinfo of the URL inside a repository string may
/// hold, as written and decoded; empty when the string has no URL or the
/// URL has no password. Each userinfo the URL may hold is tried
/// (`userinfos`), with a `${…}` reference read as one piece and as text.
pub fn url_passwords(repository: &str) -> Vec<SecretValue> {
    url_userinfo_parts(repository, |userinfo| userinfo.password)
}

/// What of the userinfo of the URL in `text` a redactor masks: its password
/// ([`url_passwords`]), or, where it has no `:` outside a `${…}` reference
/// and a reference is in it, the whole of it, as the reference may stand for
/// a user and a password both (`${file:/path}@host`).
fn url_secrets(text: &str) -> Vec<SecretValue> {
    url_userinfo_parts(text, |userinfo| {
        userinfo
            .password
            .or_else(|| userinfo.user_reference.then_some(userinfo.written))
    })
}

/// What `part` takes of each userinfo ([`userinfos`]) of the URL in `text`,
/// as written and decoded; empty when `text` has no URL.
fn url_userinfo_parts<'a>(
    text: &'a str,
    part: impl Fn(&Userinfo<'a>) -> Option<&'a str>,
) -> Vec<SecretValue> {
    forms(userinfos(text, Scheme::Required).iter().filter_map(part))
}

/// Whether a repository string is an rclone connection string that carries a
/// secret: `rclone::s3,provider=AWS,secret_access_key=…:bucket` holds the
/// settings of its backend as `key=value` pieces between commas and colons,
/// and a piece whose key names a credential in a connection string
/// (`is_rclone_secret_option`) is one. Any other string is not, whatever it
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
    let userinfos = userinfos(url, Scheme::Optional);
    forms(
        userinfos
            .iter()
            .filter_map(|userinfo| userinfo.password)
            .chain(
                userinfos
                    .iter()
                    .filter(|userinfo| userinfo.password == Some(""))
                    .map(|userinfo| userinfo.user),
            )
            .chain(userinfos.iter().map(|userinfo| userinfo.written)),
    )
}

/// The userinfos the URL in `text` may hold, in each of its readings
/// ([`WrittenUrl::readings`]): the one before the last `@` of the authority
/// as a URL parser ends it, at its first `/`, `\`, `?` or `#`; the one
/// before the last `@` up to the first `/`, for a raw `?` or `#` in a
/// password ends the authority early; and the one before the last `@` of
/// the whole of it, for a raw `/` does too. Masking part of a path by
/// mistake is the safe direction.
fn userinfos(text: &str, scheme: Scheme) -> Vec<Userinfo<'_>> {
    WrittenUrl::readings(text, scheme)
        .iter()
        .flat_map(|url| {
            [
                AuthorityEnd::Standard,
                AuthorityEnd::Slash,
                AuthorityEnd::Text,
            ]
            .into_iter()
            .filter_map(|end| url.userinfo(end))
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
mod tests;
