//! Downloads (technical design §10.4): one entry of a snapshot handed to the
//! browser that asked, a file as itself and a folder as one archive, under a
//! name made safe for one file. A confirmed download waits for its browser
//! behind a ticket that is taken once, within [`TICKET_LIFETIME`].

use jiff::{SignedDuration, Timestamp};

use crate::SnapshotId;
use crate::access::step_up::short_id;

/// How many downloads run at once on an agent: each is a worker holding
/// the repository's whole index, for as long as the browser reads.
pub const DOWNLOADS_AT_ONCE: usize = 2;

/// How long a confirmed download waits for its browser to fetch it.
pub const TICKET_LIFETIME: SignedDuration = SignedDuration::from_secs(60);

/// How many confirmed downloads may wait for their browsers at once, so
/// requests can't fill the agent's memory with tickets.
pub const MAX_PENDING: usize = 16;

/// The most bytes one piece of a download holds: the agent holds one piece
/// of each download at a time.
pub const PIECE_BYTES: usize = 64 * 1024;

/// How many random bytes a ticket holds.
pub const TICKET_BYTES: usize = 32;

/// The archive a folder is downloaded as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveFormat {
    Tar,
    Zip,
}

impl ArchiveFormat {
    /// The file name's extension, without its dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Tar => "tar",
            Self::Zip => "zip",
        }
    }
}

/// What a download hands over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadShape {
    /// One file, as it is, of this many bytes.
    File { size: u64 },
    /// A folder, or the snapshot's top, as one archive.
    Archive(ArchiveFormat),
}

/// The name a download is saved under: a file's shown name, a folder's with
/// the archive's extension, and the snapshot's top as `<host>-<short ID>`
/// with it. Every name is made one safe file name: never empty, `.` or
/// `..`, no separator, no control, no leading dot.
pub fn download_file_name(
    shown: Option<&str>,
    host: &str,
    snapshot: &SnapshotId,
    shape: DownloadShape,
) -> String {
    let stem = match shown {
        Some(shown) => safe_file_name(shown),
        None => safe_file_name(&format!("{host}-{}", short_id(snapshot))),
    };
    match shape {
        DownloadShape::File { .. } => stem,
        DownloadShape::Archive(format) => format!("{stem}.{}", format.extension()),
    }
}

/// The name a download is saved under when nothing of `name` is left.
const FALLBACK_NAME: &str = "download";

/// `name` as one file name every common system takes: each separator,
/// control and character Windows refuses becomes `_`, leading dots go (no
/// hidden file, never `.` or `..`), and nothing left is
/// [`FALLBACK_NAME`].
fn safe_file_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | '"' | '<' | '>' | ':' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let kept = replaced.trim_start_matches('.');
    if kept.is_empty() {
        FALLBACK_NAME.to_owned()
    } else {
        kept.to_owned()
    }
}

/// The capability a confirmed download is fetched by: random bytes no one
/// can guess, written as lower-case hex. Never logged or shown: its `Debug`
/// hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct DownloadTicket([u8; TICKET_BYTES]);

impl std::fmt::Debug for DownloadTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DownloadTicket([hidden])")
    }
}

impl DownloadTicket {
    pub fn from_bytes(bytes: [u8; TICKET_BYTES]) -> Self {
        Self(bytes)
    }

    /// The ticket as it travels: lower-case hex.
    pub fn encode(&self) -> String {
        self.0
            .iter()
            .fold(String::with_capacity(2 * TICKET_BYTES), |mut text, byte| {
                text.push(hex_digit(byte >> 4));
                text.push(hex_digit(byte & 0x0f));
                text
            })
    }

    /// The ticket `text` writes, if it writes one.
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.as_bytes();
        if digits.len() != 2 * TICKET_BYTES {
            return None;
        }
        let mut bytes = [0; TICKET_BYTES];
        let (pairs, _) = digits.as_chunks::<2>();
        for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
            *byte = (hex_value(*high)? << 4) | hex_value(*low)?;
        }
        Some(Self(bytes))
    }

    /// Whether `other` is this ticket, in a time that doesn't tell how
    /// much of it matched, so a guess learns nothing from the answer's
    /// speed.
    fn same_as(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0, |differ, (a, b)| differ | (a ^ b))
            == 0
    }
}

/// The lower-case hex digit of a nibble.
fn hex_digit(nibble: u8) -> char {
    char::from_digit(u32::from(nibble), 16).unwrap_or('0')
}

/// The nibble a lower-case hex digit writes.
fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}

/// The confirmed downloads waiting for their browsers, each with what the
/// agent holds for it until it is fetched or given up.
pub struct PendingDownloads<T> {
    waiting: Vec<(DownloadTicket, Timestamp, T)>,
}

impl<T> Default for PendingDownloads<T> {
    fn default() -> Self {
        Self {
            waiting: Vec::new(),
        }
    }
}

impl<T> PendingDownloads<T> {
    /// Keeps `held` under `ticket` from `now` on. Gives it back when
    /// [`MAX_PENDING`] downloads still wait.
    pub fn hold(&mut self, ticket: DownloadTicket, now: Timestamp, held: T) -> Result<(), T> {
        let waiting = self
            .waiting
            .iter()
            .filter(|(_, since, _)| !expired(*since, now))
            .count();
        if waiting >= MAX_PENDING {
            return Err(held);
        }
        self.waiting.push((ticket, now, held));
        Ok(())
    }

    /// What `ticket` holds, taken out so no one fetches it again; none when
    /// no download waits under it or its time has run out.
    pub fn take(&mut self, ticket: &DownloadTicket, now: Timestamp) -> Option<T> {
        let index = self
            .waiting
            .iter()
            .position(|(held, since, _)| held.same_as(ticket) && !expired(*since, now))?;
        Some(self.waiting.swap_remove(index).2)
    }

    /// Gives up every download whose time ran out by `now`, handing back
    /// what each held so it can be let go.
    pub fn expire(&mut self, now: Timestamp) -> Vec<T> {
        let (gone, kept) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|(_, since, _)| expired(*since, now));
        self.waiting = kept;
        gone.into_iter().map(|(_, _, held)| held).collect()
    }
}

/// Whether a download held `since` has waited past [`TICKET_LIFETIME`] by
/// `now`.
fn expired(since: Timestamp, now: Timestamp) -> bool {
    now.duration_since(since) > TICKET_LIFETIME
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> SnapshotId {
        SnapshotId::parse(&format!("3f2a1b4c{}", "d".repeat(56))).expect("a snapshot ID")
    }

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + seconds).expect("an instant")
    }

    fn ticket(byte: u8) -> DownloadTicket {
        DownloadTicket::from_bytes([byte; TICKET_BYTES])
    }

    #[test]
    fn a_file_is_saved_under_its_shown_name() {
        let name = download_file_name(
            Some("notes.md"),
            "laptop",
            &snapshot(),
            DownloadShape::File { size: 11 },
        );
        assert_eq!(name, "notes.md");
    }

    #[test]
    fn a_folder_is_saved_under_its_name_with_the_archive_s_extension() {
        for (format, expected) in [
            (ArchiveFormat::Tar, "docs.tar"),
            (ArchiveFormat::Zip, "docs.zip"),
        ] {
            let name = download_file_name(
                Some("docs"),
                "laptop",
                &snapshot(),
                DownloadShape::Archive(format),
            );
            assert_eq!(name, expected);
        }
    }

    #[test]
    fn the_top_is_saved_under_the_host_and_the_short_id() {
        let name = download_file_name(
            None,
            "laptop",
            &snapshot(),
            DownloadShape::Archive(ArchiveFormat::Tar),
        );
        assert_eq!(name, "laptop-3f2a1b4c.tar");
    }

    #[test]
    fn a_name_that_isnt_one_safe_file_name_is_made_one() {
        let file = DownloadShape::File { size: 1 };
        for (shown, expected) in [
            ("../etc", "_etc"),
            ("a/b", "a_b"),
            ("a\\b", "a_b"),
            ("new\u{240a}line", "new\u{240a}line"),
            ("bell\u{7}", "bell_"),
            ("abc[U+202E]txt.exe", "abc[U+202E]txt.exe"),
            (".hidden", "hidden"),
            ("..", "download"),
            ("", "download"),
            ("say \"hi\"", "say _hi_"),
            ("café.txt", "café.txt"),
        ] {
            assert_eq!(
                download_file_name(Some(shown), "laptop", &snapshot(), file),
                expected,
                "{shown:?}"
            );
        }
        assert_eq!(
            download_file_name(
                None,
                "../evil/host",
                &snapshot(),
                DownloadShape::Archive(ArchiveFormat::Zip)
            ),
            "_evil_host-3f2a1b4c.zip"
        );
    }

    #[test]
    fn a_ticket_travels_as_hex_and_reads_back() {
        let ticket = DownloadTicket::from_bytes(std::array::from_fn(|i| {
            u8::try_from(i).expect("fewer than 256 bytes")
        }));
        let text = ticket.encode();
        assert_eq!(text.len(), 2 * TICKET_BYTES);
        assert!(text.starts_with("000102"), "{text}");
        assert_eq!(DownloadTicket::parse(&text), Some(ticket));
    }

    #[test]
    fn text_that_is_no_ticket_reads_as_none() {
        let good = ticket(0xab).encode();
        let short = good.get(1..).expect("a long ticket");
        let cut = good.get(..62).expect("a long ticket");
        for text in [
            String::new(),
            short.to_owned(),
            format!("{good}0"),
            good.to_ascii_uppercase(),
            good.replacen('a', "g", 1),
            format!("{cut}é"),
        ] {
            assert_eq!(DownloadTicket::parse(&text), None, "{text:?}");
        }
    }

    #[test]
    fn a_ticket_names_itself_in_debug_output_and_never_shows_its_bytes() {
        let shown = format!("{:?}", ticket(0xab));
        assert_eq!(shown, "DownloadTicket([hidden])");
        assert!(!shown.contains("ab"), "{shown}");
    }

    #[test]
    fn a_waiting_download_is_taken_once() {
        let mut pending = PendingDownloads::default();
        assert!(pending.hold(ticket(1), at(0), "one").is_ok());
        assert_eq!(pending.take(&ticket(2), at(1)), None, "another ticket");
        assert_eq!(pending.take(&ticket(1), at(1)), Some("one"));
        assert_eq!(pending.take(&ticket(1), at(2)), None, "taken already");
    }

    #[test]
    fn a_download_waits_its_lifetime_and_no_longer() {
        let mut pending = PendingDownloads::default();
        assert!(pending.hold(ticket(1), at(0), "one").is_ok());
        assert!(pending.hold(ticket(2), at(0), "two").is_ok());
        assert_eq!(pending.take(&ticket(1), at(60)), Some("one"), "at its end");
        assert_eq!(pending.take(&ticket(2), at(61)), None, "past its end");
    }

    #[test]
    fn the_downloads_whose_time_ran_out_are_handed_back() {
        let mut pending = PendingDownloads::default();
        assert!(pending.hold(ticket(1), at(0), "old").is_ok());
        assert!(pending.hold(ticket(2), at(30), "new").is_ok());
        assert_eq!(pending.expire(at(61)), vec!["old"]);
        assert_eq!(pending.take(&ticket(2), at(61)), Some("new"));
    }

    #[test]
    fn no_more_than_the_most_wait_but_the_expired_make_room() {
        let mut pending = PendingDownloads::default();
        for byte in 0..u8::try_from(MAX_PENDING).expect("a small number") {
            assert!(pending.hold(ticket(byte), at(0), byte).is_ok());
        }
        assert_eq!(pending.hold(ticket(0xff), at(1), 0xff), Err(0xff));
        assert!(
            pending.hold(ticket(0xff), at(61), 0xff).is_ok(),
            "the expired ones made room"
        );
    }

    /// A piece is 64 KiB (§10.4), so its base64 text, about 87 KiB, stays far
    /// under the 1 MiB a worker's message may hold (decision 32).
    #[test]
    fn a_piece_is_64_kib_and_its_text_fits_one_message() {
        assert_eq!(PIECE_BYTES, 64 * 1024);
        let text_bytes = PIECE_BYTES.div_ceil(3) * 4;
        assert_eq!(text_bytes, 87_384);
        assert!(text_bytes < 1024 * 1024);
    }
}
