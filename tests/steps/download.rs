//! Steps for the `@e2e` scenarios of `features/restore/download.feature`:
//! the download use case runs against the real worker and a scratch
//! repository made with `rustic_core` (see `snapshots.rs`); what it hands
//! over is compared with the original files, an archive once the system's
//! own `tar` or `unzip` has unpacked it.

use std::fs;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use app::download::{DownloadAsk, Downloads};
use app::fakes::InMemoryConfigStore;
use app::ports::ConfigStore;
use app::secrets::ResolveRepository;
use cucumber::{given, then, when};
use domain::RepositoryAlias;
use domain::access::step_up::{StepUpConfirmation, short_id};
use domain::planning::{AgentConfig, AgentSettings, Hooks};
use domain::restore::download::ArchiveFormat;
use futures::StreamExt;
use platform::SystemClock;
use platform::random::OsRandom;
use secrets::SystemSecrets;

use super::{Agent, blocking};

/// What a download scenario was handed.
#[derive(Default)]
pub struct DownloadE2e {
    /// Every byte handed over, joined.
    bytes: Vec<u8>,
    /// The largest piece handed over.
    largest_piece: usize,
    /// The archive format the download asked for.
    format: Option<ArchiveFormat>,
}

/// The large file's size: far more than any piece, so it can only arrive
/// in many.
const BIG_BYTES: usize = 64 * 1024 * 1024;

fn alias() -> RepositoryAlias {
    RepositoryAlias::parse("demo").expect("an alias")
}

fn format(text: &str) -> ArchiveFormat {
    match text {
        "tar" => ArchiveFormat::Tar,
        "zip" => ArchiveFormat::Zip,
        other => panic!("no archive format {other:?}"),
    }
}

impl Agent {
    /// The download use case on the real worker, configured with the
    /// scenario's repository and sign-in off.
    async fn downloads(&mut self) -> Downloads {
        let config = InMemoryConfigStore::new();
        let repositories = [(alias(), self.snapshot_entry())].into_iter().collect();
        let agent_config = AgentConfig::new(
            AgentSettings::default(),
            Hooks::default(),
            repositories,
            Vec::new(),
        )
        .expect("the configuration is valid");
        config
            .save(&agent_config)
            .await
            .expect("the configuration saves");
        let store = self.store().await;
        Downloads::new(
            self.engine.clone(),
            Arc::new(SystemClock),
            Arc::new(config),
            ResolveRepository::new(Arc::new(SystemSecrets::new())),
            Arc::new(OsRandom),
            store,
        )
    }

    /// Confirms a download of `entry` by the snapshot's short ID, fetches
    /// it and reads it to its end, keeping its bytes.
    async fn download_entry(&mut self, entry: &str, format: ArchiveFormat) {
        let downloads = self.downloads().await;
        let snapshot = self.snapshot_made();
        let ask = DownloadAsk {
            alias: alias(),
            snapshot: snapshot.clone(),
            entry: self.snapshot_path(entry),
            format,
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            confirmation: StepUpConfirmation::parse(short_id(&snapshot).to_owned())
                .expect("a confirmation"),
        };
        let op = self.next_snapshot_op();
        let ticket = match downloads.start(op, ask).await {
            Ok(ticket) => ticket,
            Err(error) => panic!("the download wasn't confirmed: {error:?}"),
        };
        let download = match downloads.fetch(&ticket) {
            Ok(download) => download,
            Err(error) => panic!("the download wasn't handed over: {error:?}"),
        };
        let recorded = tokio::spawn(download.recorded);
        let mut pieces = download.pieces;
        let mut bytes = Vec::new();
        let mut largest = 0;
        while let Some(piece) = pieces.next().await {
            let piece = piece.expect("every piece is handed over");
            largest = largest.max(piece.len());
            bytes.extend(piece);
        }
        drop(pieces);
        recorded.await.expect("the download's end is written");
        self.download.bytes = bytes;
        self.download.largest_piece = largest;
        self.download.format = Some(format);
    }
}

/// Every file and folder under `root`, by its path from it, with each
/// file's bytes.
fn tree(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).expect("a folder to read") {
            let path = entry.expect("a folder entry").path();
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .to_path_buf();
            if path.is_dir() {
                folders.push(path);
                found.push((relative, None));
            } else {
                found.push((relative, Some(fs::read(&path).expect("a file to read"))));
            }
        }
    }
    found.sort();
    found
}

/// Unpacks `archive` into `into` with the system's own tool; `None` when
/// the tool isn't installed here.
fn unpack(archive: &Path, as_format: ArchiveFormat, into: &Path) -> Option<()> {
    fs::create_dir_all(into).expect("a folder to unpack into");
    let mut command = match as_format {
        ArchiveFormat::Tar => {
            let mut tar = Command::new("tar");
            tar.arg("-xf").arg(archive).arg("-C").arg(into);
            tar
        }
        ArchiveFormat::Zip => {
            let mut unzip = Command::new("unzip");
            unzip.arg("-q").arg(archive).arg("-d").arg(into);
            unzip
        }
    };
    match command.output() {
        Ok(output) => {
            assert!(
                output.status.success(),
                "the archive unpacks: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            Some(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => panic!("the unpacking tool ran: {error}"),
    }
}

#[given("a repository with a snapshot of a source folder holding a file of 64 MiB")]
async fn a_snapshot_with_a_big_file(agent: &mut Agent) {
    let big = agent.original("source/big.bin");
    blocking(move || {
        fs::create_dir_all(big.parent().expect("a folder")).expect("the source folder");
        // Bytes that don't repeat in any short run, so neither the
        // repository nor an archive could make them small.
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let bytes: Vec<u8> = (0..BIG_BYTES)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state.to_le_bytes()[0]
            })
            .collect();
        fs::write(&big, bytes).expect("the large file");
    })
    .await;
    super::snapshots_steps::snapshot_of_source(agent, false).await;
}

#[when(expr = "the agent downloads {string} of the snapshot")]
async fn downloads(agent: &mut Agent, entry: String) {
    agent.download_entry(&entry, ArchiveFormat::Tar).await;
}

#[when(expr = "the agent downloads {string} of the snapshot as {word}")]
async fn downloads_as(agent: &mut Agent, entry: String, as_format: String) {
    agent.download_entry(&entry, format(&as_format)).await;
}

#[then(expr = "the download is the original {string} byte for byte")]
async fn is_the_original(agent: &mut Agent, original: String) {
    let original = agent.original(&original);
    let expected = blocking(move || fs::read(original).expect("the original file")).await;
    assert!(
        agent.download.bytes == expected,
        "the download's {} bytes are the original's {}",
        agent.download.bytes.len(),
        expected.len()
    );
}

#[then("no piece the agent handed over was larger than 64 KiB")]
fn no_piece_larger(agent: &mut Agent) {
    assert!(
        agent.download.largest_piece > 0 && agent.download.largest_piece <= 64 * 1024,
        "the largest piece held {} bytes",
        agent.download.largest_piece
    );
}

#[then(
    regex = r#"^the archive unpacks to "([^"]+)" holding the original "([^"]+)" byte for byte$"#
)]
async fn archive_unpacks(agent: &mut Agent, top: String, original: String) {
    let original = agent.original(&original);
    let scratch = agent.scratch().to_path_buf();
    let bytes = std::mem::take(&mut agent.download.bytes);
    let as_format = agent.download.format.expect("a download was asked for");
    let unpacked = blocking(move || {
        let archive = scratch.join("download.archive");
        fs::write(&archive, bytes).expect("the archive saved");
        let into = scratch.join("unpacked");
        unpack(&archive, as_format, &into)?;
        let names: Vec<_> = fs::read_dir(&into)
            .expect("the unpacked folder")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(
            names,
            [top.as_str()],
            "the archive holds one folder at its top"
        );
        Some((tree(&into.join(&top)), tree(&original)))
    })
    .await;
    let Some((unpacked, original)) = unpacked else {
        testkit::skip!("the system has no tool here to unpack the archive with");
    };
    assert_eq!(unpacked, original);
}

#[given(expr = "the plan's source holds the file {string} reading {string}")]
#[when(expr = "the plan's source holds the file {string} reading {string}")]
fn source_holds_file(agent: &mut Agent, name: String, content: String) {
    let source = agent.backup.source.clone().expect("a plan source");
    fs::write(source.join(name), content).expect("the file is written");
}

/// Confirms a download of `file` of the plan's source in the repository's
/// newest snapshot by the snapshot's short ID, as the browser does with
/// sign-in off, then fetches its ticket over HTTP/1.1 and keeps the body.
#[when(expr = "the browser downloads {string} of the plan's latest snapshot over HTTP")]
async fn browser_downloads(agent: &mut Agent, file: String) {
    use super::operations_steps::exchange;
    let (status, body) = exchange(agent, "GET", "/api/v1/repositories/demo/snapshots", "");
    assert_eq!(status, 200, "the snapshots are listed: {body}");
    let listed: serde_json::Value = serde_json::from_str(&body).expect("the list is JSON");
    let newest = &listed["snapshots"][0];
    let id = newest["id"].as_str().expect("a snapshot ID");
    let top = newest["paths"][0].as_str().expect("the snapshot's path");
    let request = serde_json::json!({
        "path": format!("{}/{file}", top.trim_start_matches('/')),
        "format": "tar",
        "confirmation": short_id_of(id),
    });
    let (status, body) = exchange(
        agent,
        "POST",
        &format!("/api/v1/repositories/demo/snapshots/{id}/download"),
        &request.to_string(),
    );
    assert_eq!(status, 201, "the download is confirmed: {body}");
    let started: serde_json::Value = serde_json::from_str(&body).expect("the answer is JSON");
    let ticket = started["ticket"].as_str().expect("a ticket").to_owned();
    let addr = agent.events.addr();
    let (status, bytes) =
        blocking(move || fetch_body(addr, &format!("/api/v1/downloads/{ticket}"))).await;
    assert_eq!(status, 200, "the download is handed over");
    agent.download.bytes = bytes;
}

/// The first eight hex digits of a snapshot's ID.
fn short_id_of(id: &str) -> &str {
    id.get(..8).expect("a snapshot ID of 64 hex digits")
}

/// GETs `path` over HTTP/1.1 as a browser does, and reads the body as its
/// headers frame it: by its length when one is given, as chunks otherwise.
fn fetch_body(addr: std::net::SocketAddr, path: &str) -> (u16, Vec<u8>) {
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\nConnection: close\r\n\r\n"
    );
    let mut stream = testkit::net::connect(addr).expect("a connection to the daemon");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .expect("a read timeout");
    stream
        .write_all(request.as_bytes())
        .expect("the request is sent");
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).expect("a status line");
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status code");
    let (mut length, mut chunked) = (None, false);
    loop {
        line.clear();
        reader.read_line(&mut line).expect("a header line");
        let Some((name, value)) = line.trim_end().split_once(':') else {
            break;
        };
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse::<usize>().expect("a length"));
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.trim().eq_ignore_ascii_case("chunked");
        }
    }
    let mut body = Vec::new();
    if let Some(length) = length {
        body.resize(length, 0);
        reader.read_exact(&mut body).expect("the body");
    } else if chunked {
        loop {
            line.clear();
            reader.read_line(&mut line).expect("a chunk's size");
            let size = usize::from_str_radix(line.trim_end(), 16).expect("a chunk size");
            let mut chunk = vec![0; size + 2];
            reader.read_exact(&mut chunk).expect("a chunk");
            chunk.truncate(size);
            body.extend(chunk);
            if size == 0 {
                break;
            }
        }
    } else {
        reader.read_to_end(&mut body).expect("the body");
    }
    (status, body)
}

#[then(expr = "the browser received {string}")]
fn browser_received(agent: &mut Agent, content: String) {
    assert_eq!(
        String::from_utf8_lossy(&agent.download.bytes),
        content,
        "the body handed over"
    );
}

/// Reads the daemon's history until its download has ended, then checks
/// how: the record is written once the body's end is known, just after
/// the browser has it.
#[then("the daemon's history shows the download as succeeded")]
async fn daemon_history_shows_download(agent: &mut Agent) {
    let started = std::time::Instant::now();
    loop {
        let page = super::operations_steps::history_page(agent, "limit=50").await;
        let download = page["operations"]
            .as_array()
            .expect("the page lists operations")
            .iter()
            .find(|op| op["kind"] == "download")
            .cloned();
        if let Some(op) = download
            && !matches!(op["status"].as_str(), Some("queued" | "running"))
        {
            assert_eq!(op["status"], "succeeded", "{op}");
            return;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "the download never ended in the history: {page}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
