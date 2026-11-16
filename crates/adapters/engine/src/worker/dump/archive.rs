//! A folder of a snapshot, or its top, written as one archive as it is read:
//! each entry's header, then its contents straight from the repository,
//! never a whole file in memory. A tar archive as `tar` writes one; a zip
//! archive streamed, its entries stored rather than deflated (decision 113).
//! Folders, files and links go in, each with its permissions and its time;
//! devices, pipes and sockets have no contents and are left out.

use std::io::{self, Read as _, Write};
use std::path::{Component, Path, PathBuf};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use rustic_core::repofile::{Node, NodeType};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::job::ArchiveSpec;
use crate::worker::convert::JobError;

/// Writes the archive to `out` and hands `out` back. `named` is the folder
/// itself, whose entries then go under its name; without it (the top),
/// each entry goes under its path. `entries` are the folder's, each with
/// its path from the folder, parents before children, as rustic's `ls`
/// lists them; `contents` writes a file's.
pub(super) fn write<W: Write>(
    archive: ArchiveSpec,
    out: W,
    named: Option<Node>,
    entries: impl Iterator<Item = Result<(PathBuf, Node), JobError>>,
    contents: impl FnMut(&Node, &mut dyn Write) -> Result<(), JobError>,
) -> Result<W, JobError> {
    let prefix = named
        .as_ref()
        .map_or_else(PathBuf::new, |node| PathBuf::from(node.name().into_owned()));
    let entries = named
        .map(|node| Ok((PathBuf::new(), node)))
        .into_iter()
        .chain(entries)
        .map(move |entry| {
            // The folder itself is its name, without the separator a join
            // with nothing would end it in.
            entry.map(|(path, node)| {
                if path.as_os_str().is_empty() {
                    (prefix.clone(), node)
                } else {
                    (prefix.join(path), node)
                }
            })
        });
    match archive {
        ArchiveSpec::Tar => tar(out, entries, contents),
        ArchiveSpec::Zip => zip(out, entries, contents),
    }
}

fn tar<W: Write>(
    out: W,
    entries: impl Iterator<Item = Result<(PathBuf, Node), JobError>>,
    mut contents: impl FnMut(&Node, &mut dyn Write) -> Result<(), JobError>,
) -> Result<W, JobError> {
    let mut builder = tar::Builder::new(out);
    for entry in entries {
        let (path, node) = entry?;
        archived(&path)?;
        let mut header = tar::Header::new_gnu();
        header.set_mode(mode(&node));
        header.set_mtime(seconds(node.meta.mtime));
        header.set_uid(node.meta.uid.map_or(0, u64::from));
        header.set_gid(node.meta.gid.map_or(0, u64::from));
        header.set_size(0);
        let written = match &node.node_type {
            NodeType::Dir => {
                header.set_entry_type(tar::EntryType::Directory);
                builder.append_data(&mut header, &path, io::empty())
            }
            NodeType::Symlink { .. } => {
                header.set_entry_type(tar::EntryType::Symlink);
                builder.append_link(&mut header, &path, node.node_type.to_link())
            }
            NodeType::File => {
                let size = node.meta.size;
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(size);
                // The header with the size the snapshot records, then the
                // contents as they are read, then the padding to a block.
                builder
                    .append_data(&mut header, &path, io::empty())
                    .map_err(|error| failed(&path, &error))?;
                let mut counted = Counted::new(builder.get_mut());
                contents(&node, &mut counted)?;
                whole(&path, counted.bytes, size)?;
                pad(builder.get_mut(), size)
            }
            NodeType::Dev { .. } | NodeType::Chardev { .. } | NodeType::Fifo | NodeType::Socket => {
                continue;
            }
        };
        written.map_err(|error| failed(&path, &error))?;
    }
    builder
        .into_inner()
        .map_err(|error| JobError::failed(format!("ending the tar archive: {error}")))
}

/// The zeros that take a tar entry of `size` bytes to a whole block.
fn pad(out: &mut impl Write, size: u64) -> io::Result<()> {
    const BLOCK: u64 = 512;
    let short = (BLOCK - size % BLOCK) % BLOCK;
    io::copy(&mut io::repeat(0).take(short), out).map(drop)
}

fn zip<W: Write>(
    out: W,
    entries: impl Iterator<Item = Result<(PathBuf, Node), JobError>>,
    mut contents: impl FnMut(&Node, &mut dyn Write) -> Result<(), JobError>,
) -> Result<W, JobError> {
    let mut zip = ZipWriter::new_stream(out);
    for entry in entries {
        let (path, node) = entry?;
        archived(&path)?;
        let name = zip_name(&path);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(mode(&node))
            .last_modified_time(zip_time(node.meta.mtime));
        let written = match &node.node_type {
            NodeType::Dir => zip.add_directory(name, options),
            NodeType::Symlink { .. } => {
                zip.add_symlink(name, node.node_type.to_link().to_string_lossy(), options)
            }
            NodeType::File => {
                let size = node.meta.size;
                zip.start_file(name, options.large_file(size >= u64::from(u32::MAX)))
                    .map_err(|error| failed(&path, &error))?;
                let mut counted = Counted::new(&mut zip);
                contents(&node, &mut counted)?;
                whole(&path, counted.bytes, size)?;
                Ok(())
            }
            NodeType::Dev { .. } | NodeType::Chardev { .. } | NodeType::Fifo | NodeType::Socket => {
                continue;
            }
        };
        written.map_err(|error| failed(&path, &error))?;
    }
    zip.finish()
        .map(zip::write::StreamWriter::into_inner)
        .map_err(|error| JobError::failed(format!("ending the zip archive: {error}")))
}

/// Refuses a path that could reach outside the folder it is unpacked in: an
/// absolute one, or one with `..`. A repository is outside input, and a
/// name in its trees can hold anything.
fn archived(path: &Path) -> Result<(), JobError> {
    let mut components = path.components().peekable();
    if components.peek().is_none()
        || !components.all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(JobError::failed(format!(
            "the snapshot holds an entry no archive can name: {}",
            path.display()
        )));
    }
    Ok(())
}

/// A zip entry's name: the path's names joined by `/`, as zip names them,
/// each one made text where it isn't.
fn zip_name(path: &Path) -> String {
    path.iter()
        .map(|name| name.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The entry's permissions, or the usual ones when the snapshot has none.
/// rustic records the mode as Go writes it; its permission bits are the
/// same as Unix's.
fn mode(node: &Node) -> u32 {
    let usual = match node.node_type {
        NodeType::Dir => 0o755,
        NodeType::Symlink { .. } => 0o777,
        _ => 0o644,
    };
    node.meta.mode.map_or(usual, |mode| mode & 0o777)
}

/// Seconds since the epoch, none before it: a tar header holds no earlier
/// time.
fn seconds(time: Option<Timestamp>) -> u64 {
    time.map_or(0, |time| u64::try_from(time.as_second()).unwrap_or(0))
}

/// The time a zip entry holds: in UTC, from 1980 to 2107, which is all zip
/// can say; out of that range, zip's own first day.
fn zip_time(time: Option<Timestamp>) -> zip::DateTime {
    let Some(time) = time else {
        return zip::DateTime::default();
    };
    let at = time.to_zoned(TimeZone::UTC).datetime();
    let parts = (
        u16::try_from(at.year()),
        u8::try_from(at.month()),
        u8::try_from(at.day()),
        u8::try_from(at.hour()),
        u8::try_from(at.minute()),
        u8::try_from(at.second()),
    );
    match parts {
        (Ok(year), Ok(month), Ok(day), Ok(hour), Ok(minute), Ok(second)) => {
            zip::DateTime::from_date_and_time(year, month, day, hour, minute, second)
                .unwrap_or_default()
        }
        _ => zip::DateTime::default(),
    }
}

/// A file whose contents didn't come to the size its header gave fails the
/// download: the archive would be broken past it.
fn whole(path: &Path, bytes: u64, size: u64) -> Result<(), JobError> {
    if bytes == size {
        return Ok(());
    }
    Err(JobError::failed(format!(
        "{}: the contents came to {bytes} bytes, not the {size} the snapshot records",
        path.display()
    )))
}

fn failed(path: &Path, error: &dyn std::fmt::Display) -> JobError {
    JobError::failed(format!("archiving {}: {error}", path.display()))
}

/// A writer that counts what goes through it.
struct Counted<'w, W: Write + ?Sized> {
    out: &'w mut W,
    bytes: u64,
}

impl<'w, W: Write + ?Sized> Counted<'w, W> {
    fn new(out: &'w mut W) -> Self {
        Self { out, bytes: 0 }
    }
}

impl<W: Write + ?Sized> Write for Counted<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.out.write(buf)?;
        self.bytes = self
            .bytes
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::io::{Cursor, Read};

    use rustic_core::repofile::Metadata;

    use super::*;

    fn node(name: &str, node_type: NodeType, size: u64, mode: u32) -> Node {
        Node::new_node(
            OsStr::new(name),
            node_type,
            Metadata {
                size,
                mode: Some(mode),
                mtime: Some("2024-03-01T12:30:15Z".parse().unwrap()),
                ..Metadata::default()
            },
        )
    }

    fn file(name: &str, size: u64) -> Node {
        node(name, NodeType::File, size, 0o640)
    }

    fn folder(name: &str) -> Node {
        node(name, NodeType::Dir, 0, 0o750)
    }

    fn link(name: &str, target: &str) -> Node {
        node(name, NodeType::from_link(Path::new(target)), 0, 0o777)
    }

    /// `docs` with a file, a folder holding a file, a link and a pipe, as
    /// `ls` lists them from `docs`.
    fn listed() -> Vec<(PathBuf, Node)> {
        vec![
            ("notes.txt".into(), file("notes.txt", 5)),
            ("deep".into(), folder("deep")),
            ("deep/big.bin".into(), file("big.bin", 1300)),
            ("latest".into(), link("latest", "notes.txt")),
            ("pipe".into(), node("pipe", NodeType::Fifo, 0, 0o600)),
        ]
    }

    fn contents_of(name: &str) -> Vec<u8> {
        match name {
            "notes.txt" => b"hello".to_vec(),
            "big.bin" => (0..1300).map(|at| (at % 251) as u8).collect(),
            other => panic!("no contents for {other}"),
        }
    }

    fn archive(
        format: ArchiveSpec,
        named: Option<Node>,
        listed: Vec<(PathBuf, Node)>,
    ) -> Result<Vec<u8>, JobError> {
        write(
            format,
            Vec::new(),
            named,
            listed.into_iter().map(Ok),
            |node, out| {
                out.write_all(&contents_of(&node.name().to_string_lossy()))
                    .map_err(JobError::from)
            },
        )
    }

    /// Each entry of a tar archive: its path, its type, its mode and its
    /// contents or link target.
    fn untar(bytes: &[u8]) -> BTreeMap<String, (tar::EntryType, u32, Vec<u8>)> {
        let mut archive = tar::Archive::new(bytes);
        archive
            .entries()
            .unwrap()
            .map(|entry| {
                let mut entry = entry.unwrap();
                let path = entry.path().unwrap().to_string_lossy().into_owned();
                let kind = entry.header().entry_type();
                let mode = entry.header().mode().unwrap();
                let mut data = Vec::new();
                if kind == tar::EntryType::Symlink {
                    data = entry.link_name_bytes().unwrap().into_owned();
                } else {
                    entry.read_to_end(&mut data).unwrap();
                }
                (path, (kind, mode, data))
            })
            .collect()
    }

    #[test]
    fn a_folder_as_tar_holds_its_entries_under_its_name() {
        let bytes = archive(ArchiveSpec::Tar, Some(folder("docs")), listed()).unwrap();
        assert_eq!(bytes.len() % 512, 0, "a tar archive is whole blocks");
        let entries = untar(&bytes);
        assert_eq!(
            entries.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "docs",
                "docs/deep",
                "docs/deep/big.bin",
                "docs/latest",
                "docs/notes.txt"
            ],
            "the pipe is left out"
        );
        assert_eq!(
            entries["docs/notes.txt"],
            (tar::EntryType::Regular, 0o640, b"hello".to_vec())
        );
        assert_eq!(entries["docs/deep/big.bin"].2, contents_of("big.bin"));
        assert_eq!(
            entries["docs/deep"].0,
            tar::EntryType::Directory,
            "a folder is a folder"
        );
        assert_eq!(entries["docs"].1, 0o750);
        assert_eq!(
            entries["docs/latest"],
            (tar::EntryType::Symlink, 0o777, b"notes.txt".to_vec())
        );
    }

    #[test]
    fn a_tar_entry_keeps_its_time() {
        let bytes = archive(
            ArchiveSpec::Tar,
            None,
            vec![("notes.txt".into(), file("notes.txt", 5))],
        )
        .unwrap();
        let mut archive = tar::Archive::new(bytes.as_slice());
        let entry = archive.entries().unwrap().next().unwrap().unwrap();
        let expected: Timestamp = "2024-03-01T12:30:15Z".parse().unwrap();
        assert_eq!(
            entry.header().mtime().unwrap(),
            u64::try_from(expected.as_second()).unwrap()
        );
    }

    #[test]
    fn the_top_as_tar_holds_its_entries_under_their_paths() {
        let entries = untar(&archive(ArchiveSpec::Tar, None, listed()).unwrap());
        assert_eq!(
            entries.keys().map(String::as_str).collect::<Vec<_>>(),
            ["deep", "deep/big.bin", "latest", "notes.txt"]
        );
    }

    #[test]
    fn a_folder_as_zip_holds_its_entries_stored_under_its_name() {
        let bytes = archive(ArchiveSpec::Zip, Some(folder("docs")), listed()).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut names: Vec<String> = zip
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "docs/",
                "docs/deep/",
                "docs/deep/big.bin",
                "docs/latest",
                "docs/notes.txt"
            ],
            "the pipe is left out"
        );
        let mut read = |name: &str| {
            let mut entry = zip.by_name(name).unwrap();
            assert_eq!(
                entry.compression(),
                CompressionMethod::Stored,
                "{name} is stored, not deflated"
            );
            let mode = entry.unix_mode().unwrap() & 0o777;
            let modified = entry.last_modified().unwrap_or_default();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            (mode, modified, data)
        };
        let (mode, modified, data) = read("docs/notes.txt");
        assert_eq!((mode, data), (0o640, b"hello".to_vec()));
        assert_eq!(
            (
                modified.year(),
                modified.month(),
                modified.day(),
                modified.hour(),
                modified.minute(),
                modified.second()
            ),
            (2024, 3, 1, 12, 30, 14),
            "zip keeps even seconds"
        );
        assert_eq!(read("docs/deep/big.bin").2, contents_of("big.bin"));
        assert_eq!(
            read("docs/latest").2,
            b"notes.txt",
            "a link holds its target"
        );
        assert!(zip.by_name("docs/latest").unwrap().is_symlink());
        assert!(zip.by_name("docs/deep/").unwrap().is_dir());
    }

    #[test]
    fn a_name_that_climbs_out_of_the_folder_fails_the_archive() {
        for format in [ArchiveSpec::Tar, ArchiveSpec::Zip] {
            for path in ["../escape.txt", "/etc/escape.txt", "deep/../../escape.txt"] {
                let listed = vec![(PathBuf::from(path), file("notes.txt", 5))];
                assert!(
                    archive(format, None, listed).is_err(),
                    "{path} as {format:?} must be refused"
                );
            }
        }
    }

    #[test]
    fn a_file_shorter_than_its_size_fails_the_archive() {
        for format in [ArchiveSpec::Tar, ArchiveSpec::Zip] {
            // The snapshot says 6 bytes; its contents are 5.
            let listed = vec![("notes.txt".into(), file("notes.txt", 6))];
            let Err(JobError::Failed { detail, .. }) = archive(format, None, listed) else {
                panic!("{format:?}: a short file must fail the archive");
            };
            assert!(detail.contains("5 bytes, not the 6"), "{detail}");
        }
    }

    #[test]
    fn a_failed_read_fails_the_archive() {
        let failed = write(
            ArchiveSpec::Tar,
            Vec::new(),
            None,
            std::iter::once(Err(JobError::failed("pack lost"))),
            |_, _| Ok(()),
        );
        assert!(matches!(
            failed,
            Err(JobError::Failed { detail, .. }) if detail == "pack lost"
        ));
    }

    #[test]
    fn padding_takes_an_entry_to_a_whole_block() {
        for (size, zeros) in [(0, 0), (1, 511), (511, 1), (512, 0), (1300, 236)] {
            let mut out = Vec::new();
            pad(&mut out, size).unwrap();
            assert_eq!(out.len(), zeros, "after {size} bytes");
            assert!(out.iter().all(|&byte| byte == 0));
        }
    }

    #[test]
    fn a_time_zip_can_not_hold_is_its_first_day() {
        let early: Timestamp = "1970-01-01T00:00:00Z".parse().unwrap();
        assert_eq!(zip_time(Some(early)), zip::DateTime::default());
        assert_eq!(zip_time(None), zip::DateTime::default());
        assert_eq!(seconds(Some(early - jiff::SignedDuration::from_secs(1))), 0);
    }

    #[test]
    fn a_mode_keeps_only_its_permission_bits() {
        let mut regular = file("notes.txt", 1);
        // Go's mode of a setuid file: the bit sits high, not at 0o4000.
        regular.meta.mode = Some((1 << 23) | 0o755);
        assert_eq!(mode(&regular), 0o755);
        regular.meta.mode = None;
        assert_eq!(mode(&regular), 0o644);
        assert_eq!(
            mode(&Node::new_node(
                OsStr::new("d"),
                NodeType::Dir,
                Metadata::default()
            )),
            0o755
        );
    }
}
