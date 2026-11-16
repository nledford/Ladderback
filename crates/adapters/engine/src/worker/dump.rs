//! A download's dump (technical design §10.4): one entry of a snapshot handed
//! to the daemon in pieces on the event stream, a file as itself and a folder
//! or the snapshot's top as one archive. The entry is reached through the
//! snapshot's trees one name at a time, as a restore reaches it, and its
//! contents are read with rustic's own dump. Nothing is written anywhere but
//! the stream, and nothing is held whole: one piece at a time.

mod archive;

use std::io::{self, Write};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domain::restore::download::PIECE_BYTES;
use rustic_core::LsOptions;
use rustic_core::repofile::{NodeType, SnapshotId as RusticSnapshotId};

use super::convert::JobError;
use super::events::Events;
use super::listing::cut;
use super::repository::{open, repository_id};
use super::restore::find_entry;
use super::{Context, Done};
use crate::job::{DumpFound, DumpJobSpec, DumpResult, OpResult, WorkerEvent};

/// Hands the job's entry over. It tells the daemon it has started right
/// after the open, before anything else, with a backup's budget as its
/// deadline (I3: a prune may delete the packs it reads once `keep-delete`
/// has passed): a snapshot or an entry that isn't there is an answer, which
/// only a started job gives. Then the header, the pieces, and the bytes
/// they held in all.
pub(crate) fn dump(ctx: &Context, job: &DumpJobSpec) -> Result<Done, JobError> {
    let id: RusticSnapshotId =
        job.snapshot.as_str().parse().map_err(|_| {
            JobError::failed(format!("not a snapshot ID: {}", job.snapshot.as_str()))
        })?;
    let repository = open(ctx, &job.repo)?;
    ctx.start(
        Some(job.deadline_budget),
        Some(&repository_id(&repository)?),
    )?;
    let unknown = || Ok(Done::Result(OpResult::Dump(DumpResult::SnapshotUnknown)));
    // Listed rather than looked up, so a snapshot that isn't there is an
    // answer, not one of the many errors a lookup gives.
    if !repository
        .list::<RusticSnapshotId>()?
        .any(|listed| listed == id)
    {
        return unknown();
    }
    let Some(snapshot) = repository
        .get_snapshots(&[job.snapshot.as_str()])?
        .into_iter()
        .next()
    else {
        return unknown();
    };
    let repository = repository.to_indexed()?;
    let entry_unknown = Ok(Done::Result(OpResult::Dump(DumpResult::EntryUnknown)));
    let Some((node, _)) = find_entry(&repository, snapshot.tree, &job.entry)? else {
        return entry_unknown;
    };
    let found = match node.node_type {
        NodeType::File => DumpFound::File {
            size: node.meta.size,
        },
        NodeType::Dir => DumpFound::Archive {
            archive: job.archive,
        },
        // A link, a device, a pipe or a socket has no contents to hand over.
        NodeType::Symlink { .. }
        | NodeType::Dev { .. }
        | NodeType::Chardev { .. }
        | NodeType::Fifo
        | NodeType::Socket => return entry_unknown,
    };
    ctx.events.emit(&WorkerEvent::Dumping {
        found,
        // The top has no name of its own: the host names its archive.
        name: (!job.entry.is_empty()).then(|| cut(&node.name().to_string_lossy()).0),
        host: cut(&snapshot.hostname).0,
    })?;
    let mut pieces = Pieces::new(&ctx.events);
    match found {
        DumpFound::File { size } => {
            repository.dump(&node, &mut pieces)?;
            if pieces.bytes() != size {
                return Err(JobError::failed(format!(
                    "the file's contents came to {} bytes, not the {size} the snapshot records",
                    pieces.bytes()
                )));
            }
        }
        DumpFound::Archive { archive } => {
            let listing = repository.ls(&node, &LsOptions::default())?;
            // A folder goes under its own name, the top under its paths.
            let named = (!job.entry.is_empty()).then(|| node.clone());
            archive::write(
                archive,
                &mut pieces,
                named,
                listing.map(|item| item.map_err(JobError::from)),
                |node, mut out| Ok(repository.dump(node, &mut out)?),
            )?;
        }
    }
    let bytes = pieces.finish()?;
    // Everything is handed over: nothing is left that the deadline guards.
    ctx.disarm();
    Ok(Done::Result(OpResult::Dump(DumpResult::Done { bytes })))
}

/// What a dump writes: its bytes, gathered into pieces of at most
/// [`PIECE_BYTES`], each sent as a `piece` event once it is full. Only
/// [`Pieces::finish`] sends the last one: a flush sends nothing, so the
/// writers above it can't break the stream into small pieces.
struct Pieces<'e> {
    events: &'e Events,
    piece: Vec<u8>,
    /// The bytes sent so far.
    sent: u64,
}

impl<'e> Pieces<'e> {
    fn new(events: &'e Events) -> Self {
        Self {
            events,
            piece: Vec::with_capacity(PIECE_BYTES),
            sent: 0,
        }
    }

    /// The bytes written so far, sent or not.
    fn bytes(&self) -> u64 {
        let held = u64::try_from(self.piece.len()).unwrap_or(u64::MAX);
        self.sent.saturating_add(held)
    }

    fn send(&mut self) -> io::Result<()> {
        if self.piece.is_empty() {
            return Ok(());
        }
        self.events.emit(&WorkerEvent::Piece {
            bytes: STANDARD.encode(&self.piece),
        })?;
        self.sent = self.bytes();
        self.piece.clear();
        Ok(())
    }

    /// Sends what is left, and tells how many bytes went in all.
    fn finish(mut self) -> io::Result<u64> {
        self.send()?;
        Ok(self.sent)
    }
}

impl Write for Pieces<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let room = PIECE_BYTES.saturating_sub(self.piece.len());
        let taken = buf.get(..room).unwrap_or(buf);
        self.piece.extend_from_slice(taken);
        if self.piece.len() >= PIECE_BYTES {
            self.send()?;
        }
        Ok(taken.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD;

    use super::*;
    use crate::worker::events::testing::Capture;

    fn pieces(capture: &Capture) -> Vec<Vec<u8>> {
        capture
            .parsed()
            .into_iter()
            .map(|event| match event {
                WorkerEvent::Piece { bytes } => STANDARD.decode(bytes).unwrap(),
                other => panic!("a piece, not {other:?}"),
            })
            .collect()
    }

    #[test]
    fn bytes_go_in_full_pieces_and_the_rest_at_the_finish() {
        let (events, capture) = Capture::default().events();
        let mut out = Pieces::new(&events);
        let data: Vec<u8> = (0..PIECE_BYTES * 2 + 10).map(|at| at as u8).collect();
        // Written in uneven bites, as an archive writer writes.
        for chunk in data.chunks(1000) {
            out.write_all(chunk).unwrap();
        }
        out.flush().unwrap();
        assert_eq!(
            pieces(&capture).len(),
            2,
            "a flush sends nothing; only full pieces are sent before the finish"
        );
        assert_eq!(out.bytes(), u64::try_from(data.len()).unwrap());
        assert_eq!(out.finish().unwrap(), u64::try_from(data.len()).unwrap());
        let sent = pieces(&capture);
        assert_eq!(
            sent.iter().map(Vec::len).collect::<Vec<_>>(),
            [PIECE_BYTES, PIECE_BYTES, 10]
        );
        assert_eq!(sent.concat(), data);
    }

    #[test]
    fn nothing_written_sends_no_piece() {
        let (events, capture) = Capture::default().events();
        assert_eq!(Pieces::new(&events).finish().unwrap(), 0);
        assert!(pieces(&capture).is_empty());
    }

    #[test]
    fn a_piece_fits_one_event_line() {
        let (events, capture) = Capture::default().events();
        let mut out = Pieces::new(&events);
        out.write_all(&vec![0xff; PIECE_BYTES]).unwrap();
        let line = capture.text();
        assert!(
            line.len() < crate::job::MAX_MESSAGE_BYTES,
            "a full piece is {} bytes of event",
            line.len()
        );
    }
}
