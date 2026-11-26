//! The agent ID: a UUIDv7 generated once and kept in the data dir, never in
//! the database, so `db rebuild` can't change it (technical design §7.1).
//! Every backup is tagged with it (§7.3), so losing or replacing it would
//! detach every plan's history.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use uuid::Uuid;

use domain::AgentId;
use domain::fleet::KeyFile;

/// The file in the data dir holding the agent ID.
pub const MACHINE_ID_FILE: &str = "machine-id";

/// More than an ID and some whitespace: anything longer is not an ID file,
/// and is never read whole (technical design §13.5).
const MAX_ID_FILE_BYTES: u64 = 256;

/// The agent ID stored in `data_dir`, or `None` when there is none yet. A
/// command that only reads, such as `hub-status`, uses it rather than
/// [`load_or_create`]: it writes nothing but the one-off move of an earlier
/// build's names ([`move_earlier_names`]). A file that exists but doesn't
/// hold an ID is an error.
pub fn load(data_dir: &Path) -> io::Result<Option<AgentId>> {
    move_earlier_names(data_dir)?;
    match read_id_file(&data_dir.join(MACHINE_ID_FILE)) {
        Ok(text) => parse(&text).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The ID in an earlier build's `agent-id` the one-off move left beside
/// `machine-id`, which is the one used (decision 170), for the daemon's
/// log to name; `None` when there is none.
pub fn earlier_beside(data_dir: &Path) -> io::Result<Option<AgentId>> {
    match read_id_file(&data_dir.join(EARLIER_ID_FILE)) {
        Ok(text) => parse(&text).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The names an earlier build, 0.5.0 among them, gave the machine ID and
/// the machine key before decision 163.
const EARLIER_ID_FILE: &str = "agent-id";
const EARLIER_KEY_FILE: &str = "agent-key";

/// Moves an earlier build's `agent-id` and `agent-key` to `machine-id` and
/// `machine-key`, keeping the ID and the key as they are, so the machine's
/// snapshots stay its own (§16, decision 170). A one-off upgrade from 0.5.0,
/// a deliberate exception to "no shims": removed at 1.0.
///
/// It runs only while `machine-id` is absent and `agent-id` is there. The
/// key moves first, then the ID, each by a link and an unlink, so a start
/// that stops partway leaves a state the next one finishes, and starts at
/// once (a `pair` beside the daemon's start) agree. Then the files an
/// earlier build staged beside them and a crash left (`agent-key.*`,
/// `agent-id.*.tmp`, #1621) are deleted. A link keeps the file's owner-only
/// mode, and nothing reads the key (I4). When `machine-id` is there too,
/// both are left and `machine-id` is the ID: a build after decision 163
/// made it and may have used it since. An `agent-key` beside a
/// `machine-key` of its own is an error, every file left as it was: which
/// key is the machine's is the person's to say.
fn move_earlier_names(data_dir: &Path) -> io::Result<()> {
    let earlier_id = data_dir.join(EARLIER_ID_FILE);
    if !earlier_id.try_exists()? {
        return Ok(());
    }
    let id = data_dir.join(MACHINE_ID_FILE);
    if id.try_exists()? {
        // A move that stopped between the ID's link and its unlink left
        // two names of one file: finish it. Two files are both left.
        if !same_file(&earlier_id, &id)? {
            return Ok(());
        }
    } else {
        move_file(
            &data_dir.join(EARLIER_KEY_FILE),
            &data_dir.join(KeyFile::AgentKey.file_name()),
        )?;
    }
    move_file(&earlier_id, &id)?;
    remove_staged(data_dir)?;
    sync_dir(data_dir)
}

/// Gives `from`'s file the name `to` and drops the name `from`. `from`
/// already gone is a move done, by an earlier start or one beside this,
/// or one never needed. `to` there already is the same move made beside
/// this one, or, as another file, an error that names both files and
/// leaves them.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    match fs::hard_link(from, to) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            // Not one file: unless a move beside this one has just dropped
            // the name `from`, two files hold the name's content.
            if !same_file(from, to)? && from.try_exists()? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "{} and {} are two files, and the machine ID is still in {EARLIER_ID_FILE}: \
                         move away the one that isn't this machine's",
                        from.display(),
                        to.display()
                    ),
                ));
            }
        }
        Err(error) => return Err(error),
    }
    match fs::remove_file(from) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Whether `a` and `b` name one file; `false` when either is gone.
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let (a, b) = match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(false);
        }
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    Ok(a.dev() == b.dev() && a.ino() == b.ino())
}

/// Off Unix there is no inode to compare: two names are taken for two
/// files, the safe answer.
#[cfg(not(unix))]
fn same_file(_a: &Path, _b: &Path) -> io::Result<bool> {
    Ok(false)
}

/// Deletes what an earlier build staged beside its key and ID files and a
/// crash left: `agent-key.*` and `agent-id.*.tmp` (#1621).
fn remove_staged(data_dir: &Path) -> io::Result<()> {
    let key_prefix = format!("{EARLIER_KEY_FILE}.");
    let id_prefix = format!("{EARLIER_ID_FILE}.");
    for entry in fs::read_dir(data_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let staged = name.starts_with(&key_prefix)
            || (name.starts_with(&id_prefix) && name.ends_with(".tmp"));
        if !staged {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// The agent ID stored in `data_dir`, generating and storing one first if
/// there is none. A file that exists but doesn't hold an ID is an error,
/// never silently replaced.
pub fn load_or_create(data_dir: &Path) -> io::Result<AgentId> {
    if let Some(id) = load(data_dir)? {
        return Ok(id);
    }
    let path = data_dir.join(MACHINE_ID_FILE);
    fs::create_dir_all(data_dir)?;
    // Write the whole file under a private name, then link it into place:
    // the link fails if another process got there first, so concurrent first
    // starts agree on one ID and nobody ever reads a half-written file.
    let id = Uuid::now_v7();
    let staged = data_dir.join(format!("{MACHINE_ID_FILE}.{}.tmp", id.simple()));
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
/// backup can't bring the agent back with a different ID. The key files
/// rely on it too.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_data_dir_with_no_id_has_none_and_is_left_empty() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        assert_eq!(load(&data).unwrap(), None);
        assert!(!data.exists());
    }

    #[test]
    fn a_kept_id_is_loaded_as_it_is() {
        let data = tempfile::tempdir().unwrap();
        let id = load_or_create(data.path()).unwrap();
        assert_eq!(load(data.path()).unwrap(), Some(id));
    }

    #[test]
    fn a_damaged_id_file_is_an_error_when_only_loaded() {
        let data = tempfile::tempdir().unwrap();
        fs::write(data.path().join(MACHINE_ID_FILE), "not an id").unwrap();
        let error = load(data.path()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

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
        fs::write(data.path().join(MACHINE_ID_FILE), format!("{text}\n")).unwrap();
        let id = load_or_create(data.path()).unwrap();
        assert_eq!(id.as_uuid().hyphenated().to_string(), text);
    }

    /// The ID lives in `machine-id` (§16, decision 163).
    #[test]
    fn the_id_is_kept_in_machine_id() {
        let data = tempfile::tempdir().unwrap();
        let id = load_or_create(data.path()).unwrap();
        let kept = fs::read_to_string(data.path().join("machine-id")).unwrap();
        assert_eq!(kept.trim(), id.as_uuid().hyphenated().to_string());
    }

    /// The ID an earlier build kept in `agent-id`.
    const EARLIER_ID: &str = "0190a6e4-7a3b-7c3d-8e4f-0123456789ab";

    /// A canary, never a real key (§13.9).
    const KEY_CANARY: &str = "cacacacacacacacacacacacacacacacacacacacacacacacacacacacacacacaca\n";

    /// Writes `text` to `name` in `data`, its owner's alone, as every key
    /// file and ID file is written.
    fn write_private(data: &Path, name: &str, text: &str) {
        fs::write(data.join(name), text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(data.join(name), fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    /// A data folder as an earlier build left it (§16, decisions 163 and
    /// 170): `agent-id`, `agent-key`, and the files it staged beside them
    /// that a crash left (#1621).
    fn earlier_data_folder() -> tempfile::TempDir {
        let data = tempfile::tempdir().unwrap();
        write_private(data.path(), "agent-id", &format!("{EARLIER_ID}\n"));
        write_private(data.path(), "agent-key", KEY_CANARY);
        write_private(data.path(), "agent-key.0190a6e47a3b7c3d.tmp", KEY_CANARY);
        write_private(data.path(), "agent-id.0190a6e47a3b7c3d.tmp", EARLIER_ID);
        data
    }

    /// The names in `data`, sorted.
    fn names(data: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(data)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// An earlier build's `agent-id` and `agent-key` become `machine-id`
    /// and `machine-key`, keeping the ID, the key byte for byte and its
    /// owner-only mode; what it staged beside them is gone.
    #[test]
    fn an_earlier_builds_agent_id_and_key_become_the_machine_id_and_key() {
        let data = earlier_data_folder();

        let id = load_or_create(data.path()).unwrap();

        assert_eq!(id.as_uuid().hyphenated().to_string(), EARLIER_ID);
        assert_eq!(names(data.path()), ["machine-id", "machine-key"]);
        assert_eq!(
            fs::read_to_string(data.path().join("machine-key")).unwrap(),
            KEY_CANARY
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(data.path().join("machine-key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "the key stays its owner's alone");
        }
    }

    /// A command that only reads the ID, such as `hub-status`, finds it
    /// under its earlier name too, and moves it as a start does.
    #[test]
    fn a_command_that_only_reads_the_id_finds_it_under_its_earlier_name() {
        let data = earlier_data_folder();

        let id = load(data.path()).unwrap().unwrap();

        assert_eq!(id.as_uuid().hyphenated().to_string(), EARLIER_ID);
        assert_eq!(names(data.path()), ["machine-id", "machine-key"]);
    }

    /// A machine that never paired kept an ID and no key: the ID moves
    /// alone.
    #[test]
    fn an_earlier_id_with_no_key_moves_alone() {
        let data = tempfile::tempdir().unwrap();
        write_private(data.path(), "agent-id", &format!("{EARLIER_ID}\n"));

        let id = load_or_create(data.path()).unwrap();

        assert_eq!(id.as_uuid().hyphenated().to_string(), EARLIER_ID);
        assert_eq!(names(data.path()), ["machine-id"]);
    }

    /// The key moves first, then the ID: a start that stopped between the
    /// two left `machine-key` and `agent-id`, and the next one finishes.
    #[test]
    fn a_move_that_stopped_after_the_key_is_finished_by_the_next_start() {
        let data = tempfile::tempdir().unwrap();
        write_private(data.path(), "agent-id", &format!("{EARLIER_ID}\n"));
        write_private(data.path(), "machine-key", KEY_CANARY);

        let id = load_or_create(data.path()).unwrap();

        assert_eq!(id.as_uuid().hyphenated().to_string(), EARLIER_ID);
        assert_eq!(names(data.path()), ["machine-id", "machine-key"]);
        assert_eq!(
            fs::read_to_string(data.path().join("machine-key")).unwrap(),
            KEY_CANARY
        );
    }

    /// An `agent-key` beside a `machine-key` of its own, with the ID still
    /// under its earlier name, is no state a move leaves: which key is the
    /// machine's is the person's to say. The start is refused, every file
    /// left as it was, and the error holds neither key (I4).
    #[test]
    fn an_earlier_key_beside_another_machine_key_is_refused_and_left() {
        let data = earlier_data_folder();
        let other = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b\n";
        write_private(data.path(), "machine-key", other);
        let before = names(data.path());

        let error = load_or_create(data.path()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        let said = error.to_string();
        assert!(
            said.contains("agent-key") && said.contains("machine-key"),
            "{said}"
        );
        assert!(!said.contains("caca") && !said.contains("0b0b"), "{said}");
        assert_eq!(names(data.path()), before);
        assert_eq!(
            fs::read_to_string(data.path().join("machine-key")).unwrap(),
            other
        );
    }

    /// A `machine-id` beside a different `agent-id`: a build after
    /// decision 163 chose the machine's new ID, which may be in use since.
    /// Both are left, `machine-id` is the ID, and the earlier one can be
    /// named for the daemon's log (decision 170).
    #[test]
    fn a_machine_id_beside_an_earlier_agent_id_is_used_and_both_are_left() {
        let data = earlier_data_folder();
        let own = "0190a6e4-7a3b-7c3d-8e4f-ba9876543210";
        write_private(data.path(), "machine-id", &format!("{own}\n"));
        write_private(data.path(), "machine-key", "0b0b\n");
        let before = names(data.path());

        let id = load_or_create(data.path()).unwrap();

        assert_eq!(id.as_uuid().hyphenated().to_string(), own);
        assert_eq!(names(data.path()), before);
        let earlier = earlier_beside(data.path()).unwrap().unwrap();
        assert_eq!(earlier.as_uuid().hyphenated().to_string(), EARLIER_ID);
    }

    /// Starts at once on an earlier data folder, as a `pair` beside the
    /// daemon's start, agree on its ID.
    #[test]
    fn concurrent_starts_on_an_earlier_data_folder_keep_its_id() {
        let data = earlier_data_folder();
        let ids: Vec<AgentId> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| s.spawn(|| load_or_create(data.path()).unwrap()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for id in &ids {
            assert_eq!(id.as_uuid().hyphenated().to_string(), EARLIER_ID);
        }
        assert_eq!(names(data.path()), ["machine-id", "machine-key"]);
    }

    #[test]
    fn a_damaged_id_file_is_an_error_and_left_alone() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join(MACHINE_ID_FILE);
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
        assert_eq!(leftovers, [MACHINE_ID_FILE], "only the ID file remains");
    }
}
