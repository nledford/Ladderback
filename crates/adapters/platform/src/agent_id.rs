//! The agent ID: a UUIDv7 generated once and kept in the data dir, never in
//! the database, so `db rebuild` can't change it (technical design §7.1).
//! Every backup is tagged with it (§7.3), so losing or replacing it would
//! detach every plan's history.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use uuid::Uuid;

use domain::AgentId;

/// The file in the data dir holding the agent ID.
pub const AGENT_ID_FILE: &str = "agent-id";

/// More than an ID and some whitespace: anything longer is not an ID file,
/// and is never read whole (technical design §13.5).
const MAX_ID_FILE_BYTES: u64 = 256;

/// The agent ID stored in `data_dir`, generating and storing one first if
/// there is none. A file that exists but doesn't hold an ID is an error,
/// never silently replaced.
pub fn load_or_create(data_dir: &Path) -> io::Result<AgentId> {
    let path = data_dir.join(AGENT_ID_FILE);
    match read_id_file(&path) {
        Ok(text) => return parse(&text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::create_dir_all(data_dir)?;
    // Write the whole file under a private name, then link it into place:
    // the link fails if another process got there first, so concurrent first
    // starts agree on one ID and nobody ever reads a half-written file.
    let id = Uuid::now_v7();
    let staged = data_dir.join(format!("{AGENT_ID_FILE}.{}.tmp", id.simple()));
    let written = stage(&staged, id).and_then(|()| fs::hard_link(&staged, &path));
    let removed = fs::remove_file(&staged);
    match written {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    removed?;
    sync_dir(data_dir)?;
    parse(&read_id_file(&path)?)
}

fn stage(path: &Path, id: Uuid) -> io::Result<()> {
    let mut file = File::create_new(path)?;
    writeln!(file, "{}", id.hyphenated())?;
    file.sync_all()
}

/// The ID file's text, refused unread past [`MAX_ID_FILE_BYTES`].
fn read_id_file(path: &Path) -> io::Result<String> {
    let mut text = String::new();
    File::open(path)?
        .take(MAX_ID_FILE_BYTES + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_ID_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is longer than {MAX_ID_FILE_BYTES} bytes",
                path.display()
            ),
        ));
    }
    Ok(text)
}

fn parse(text: &str) -> io::Result<AgentId> {
    Uuid::try_parse(text.trim())
        .map(AgentId::from_uuid)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Makes the new directory entry durable, so a crash right after the first
/// backup can't bring the agent back with a different ID.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn an_agent_keeps_the_id_it_was_first_given() {
        let data = tempfile::tempdir().unwrap();
        let first = load_or_create(data.path()).unwrap();
        assert_eq!(load_or_create(data.path()).unwrap(), first);
    }

    #[test]
    fn a_new_id_is_a_uuid_v7() {
        let data = tempfile::tempdir().unwrap();
        let id = load_or_create(data.path()).unwrap();
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn separate_data_dirs_get_separate_ids() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        assert_ne!(
            load_or_create(a.path()).unwrap(),
            load_or_create(b.path()).unwrap()
        );
    }

    #[test]
    fn a_missing_data_dir_is_created() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let id = load_or_create(&data).unwrap();
        assert_eq!(load_or_create(&data).unwrap(), id);
    }

    #[test]
    fn an_id_written_by_hand_is_accepted() {
        let data = tempfile::tempdir().unwrap();
        let text = "0190a6e4-7a3b-7c3d-8e4f-0123456789ab";
        fs::write(data.path().join(AGENT_ID_FILE), format!("{text}\n")).unwrap();
        let id = load_or_create(data.path()).unwrap();
        assert_eq!(id.as_uuid().hyphenated().to_string(), text);
    }

    #[test]
    fn a_damaged_id_file_is_an_error_and_left_alone() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join(AGENT_ID_FILE);
        fs::write(&path, "not an id").unwrap();
        let error = load_or_create(data.path()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read_to_string(&path).unwrap(), "not an id");
    }

    #[test]
    fn concurrent_first_starts_agree_on_one_id() {
        let data = tempfile::tempdir().unwrap();
        let ids: Vec<AgentId> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| s.spawn(|| load_or_create(data.path()).unwrap()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(ids.iter().all(|id| *id == ids[0]), "{ids:?}");
        let leftovers: Vec<_> = fs::read_dir(data.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, [AGENT_ID_FILE], "only the ID file remains");
    }
}
