//! The agent ID: a UUIDv7 generated once and kept in the data dir, never in
//! the database, so `db rebuild` can't change it (technical design §7.1).
//! Every backup is tagged with it (§7.3), so losing or replacing it would
//! detach every plan's history.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use uuid::Uuid;

use domain::AgentId;
use domain::fleet::KeyFile;

use crate::data_dir_lock::DataDirLock;

/// Why [`move_earlier_names`] didn't move the earlier names.
#[derive(Debug)]
pub enum MoveError {
    /// `earlier`, an earlier build's `agent-key`, sits beside `own`, a
    /// `machine-key` of its own, while the ID is still in `agent-id`: which
    /// key is the machine's is the person's to say. Every file is left as
    /// it was.
    TwoKeys { earlier: PathBuf, own: PathBuf },
    /// The data folder couldn't be read or written, in the operating
    /// system's words.
    Io(io::Error),
}

impl From<io::Error> for MoveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The file in the data dir holding the agent ID.
pub const MACHINE_ID_FILE: &str = "machine-id";

/// More than an ID and some whitespace: anything longer is not an ID file,
/// and is never read whole (technical design §13.5).
const MAX_ID_FILE_BYTES: u64 = 256;

/// The agent ID stored in `data_dir`, or `None` when there is none yet. A
/// command that only reads, such as `hub-status`, uses it rather than
/// [`load_or_create`]: it writes nothing. A file that exists but doesn't
/// hold an ID is an error.
pub fn load(data_dir: &Path) -> io::Result<Option<AgentId>> {
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

/// Whether `data_dir` holds what [`move_earlier_names`] moves or deletes:
/// an earlier build's `agent-id` it would move, or a file an earlier build
/// staged beside its key or ID. It only looks, so a command that runs
/// without the data folder's lock asks it before it takes the lock.
pub fn earlier_names_left(data_dir: &Path) -> io::Result<bool> {
    let earlier_id = data_dir.join(EARLIER_ID_FILE);
    if earlier_id.try_exists()? {
        let id = data_dir.join(MACHINE_ID_FILE);
        if !id.try_exists()? || same_file(&earlier_id, &id)? {
            return Ok(true);
        }
    }
    match staged(data_dir) {
        Ok(staged) => Ok(!staged.is_empty()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Moves an earlier build's `agent-id` and `agent-key` to `machine-id` and
/// `machine-key`, keeping the ID and the key as they are, so the machine's
/// snapshots stay its own (§16, decision 170). A one-off upgrade from 0.5.0,
/// a deliberate exception to "no shims": removed at 1.0.
///
/// Only a process that holds the data folder's lock moves them, so no
/// other Ladderback, 0.5.0's daemon among them, reads the key while its
/// name changes. It moves only while `machine-id` is absent and `agent-id`
/// is there. The key moves first, then the ID, each by a link and an
/// unlink, so a start that stops partway leaves a state the next one
/// finishes. Then, whether it moved or not, the files an earlier build
/// staged beside them and a crash left (`agent-key.*.tmp`,
/// `agent-id.*.tmp`, #1621) are deleted, so a start that stopped before it
/// deleted them leaves them to the next. A link keeps the file's
/// owner-only mode, and nothing reads the key (I4). When `machine-id` is
/// there too, both are left and `machine-id` is the ID: a build after
/// decision 163 made it and may have used it since. An `agent-key` beside
/// a `machine-key` of its own is [`MoveError::TwoKeys`], every file left
/// as it was: which key is the machine's is the person's to say.
pub fn move_earlier_names(data_dir: &Path, _held: &DataDirLock) -> Result<(), MoveError> {
    let earlier_id = data_dir.join(EARLIER_ID_FILE);
    let id = data_dir.join(MACHINE_ID_FILE);
    let mut moved = false;
    if earlier_id.try_exists()? {
        // A move that stopped between the ID's link and its unlink left
        // two names of one file: finish it. Two files are both left.
        let unmoved = !id.try_exists()?;
        if unmoved {
            move_file(
                &data_dir.join(EARLIER_KEY_FILE),
                &data_dir.join(KeyFile::AgentKey.file_name()),
            )?;
        }
        if unmoved || same_file(&earlier_id, &id)? {
            move_file(&earlier_id, &id)?;
            moved = true;
        }
    }
    let staged = match staged(data_dir) {
        Ok(staged) => staged,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if staged.is_empty() && !moved {
        return Ok(());
    }
    remove_staged(staged)?;
    Ok(sync_dir(data_dir)?)
}

/// Gives `from`'s file the name `to` and drops the name `from`. `from`
/// already gone is a move done, by an earlier start, or one never needed.
/// `to` there already is the same move an earlier start stopped between
/// its link and its unlink, or, as another file, [`MoveError::TwoKeys`],
/// both left. The new name is made durable before the old one is dropped,
/// so a crash between leaves the file under one name or both, never
/// neither.
fn move_file(from: &Path, to: &Path) -> Result<(), MoveError> {
    match fs::hard_link(from, to) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            // Not one file: unless an earlier move has dropped the name
            // `from`, two files hold the name's content.
            if !same_file(from, to)? && from.try_exists()? {
                return Err(MoveError::TwoKeys {
                    earlier: from.to_owned(),
                    own: to.to_owned(),
                });
            }
        }
        Err(error) => return Err(error.into()),
    }
    match to.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => sync_dir(folder)?,
        _ => sync_dir(Path::new("."))?,
    }
    match fs::remove_file(from) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
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

/// Deletes `staged`, what an earlier build staged beside its key and ID
/// files and a crash left.
fn remove_staged(staged: Vec<std::path::PathBuf>) -> io::Result<()> {
    for path in staged {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// The files an earlier build staged beside its key and ID files, each
/// `agent-key.<hex>.tmp` or `agent-id.<hex>.tmp` (#1621). Any other name
/// beside them, as an `agent-key.bak` the person keeps, is not one.
fn staged(data_dir: &Path) -> io::Result<Vec<std::path::PathBuf>> {
    let prefixes = [
        format!("{EARLIER_KEY_FILE}."),
        format!("{EARLIER_ID_FILE}."),
    ];
    let mut staged = Vec::new();
    for entry in fs::read_dir(data_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.ends_with(".tmp") && prefixes.iter().any(|prefix| name.starts_with(prefix)) {
            staged.push(entry.path());
        }
    }
    Ok(staged)
}

/// The agent ID stored in `data_dir`, generating and storing one first if
/// there is none. A file that exists but doesn't hold an ID is an error,
/// never silently replaced. So is an earlier build's `agent-id` that
/// [`move_earlier_names`] hasn't moved: a new ID beside it would detach the
/// machine's snapshots.
pub fn load_or_create(data_dir: &Path) -> io::Result<AgentId> {
    if let Some(id) = load(data_dir)? {
        return Ok(id);
    }
    let earlier = data_dir.join(EARLIER_ID_FILE);
    if earlier.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            earlier.display().to_string(),
        ));
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

    /// The names in `data`, sorted, but the data folder's lock file.
    fn names(data: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(data)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name != crate::data_dir_lock::LOCK_FILE)
            .collect();
        names.sort();
        names
    }

    /// Moves the earlier names under the data folder's lock, as a start
    /// does, then reads the ID.
    fn moved(data: &Path) -> io::Result<AgentId> {
        let lock = DataDirLock::acquire(data).unwrap();
        move_earlier_names(data, &lock).unwrap();
        load_or_create(data)
    }

    /// An earlier build's `agent-id` and `agent-key` become `machine-id`
    /// and `machine-key`, keeping the ID, the key byte for byte and its
    /// owner-only mode; what it staged beside them is gone.
    #[test]
    fn an_earlier_builds_agent_id_and_key_become_the_machine_id_and_key() {
        let data = earlier_data_folder();

        let id = moved(data.path()).unwrap();

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

    /// Without the move, which only the data folder's holder makes, a
    /// command that reads the ID moves nothing, finds none, and makes none
    /// beside the earlier one: the folder is left as it was.
    #[test]
    fn reading_the_id_moves_nothing_and_makes_no_new_one_beside_an_earlier_id() {
        let data = earlier_data_folder();
        let before = names(data.path());

        assert!(earlier_names_left(data.path()).unwrap());
        assert_eq!(load(data.path()).unwrap(), None);
        let error = load_or_create(data.path()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(names(data.path()), before);
    }

    /// What a move would change is found by looking alone: nothing in a
    /// folder this build made, or once the move is made, or where a
    /// `machine-id` of its own sits beside an earlier `agent-id`.
    #[test]
    fn earlier_names_are_left_only_until_they_are_moved() {
        let fresh = tempfile::tempdir().unwrap();
        load_or_create(fresh.path()).unwrap();
        assert!(!earlier_names_left(fresh.path()).unwrap());
        assert!(!earlier_names_left(&fresh.path().join("missing")).unwrap());

        let earlier = earlier_data_folder();
        moved(earlier.path()).unwrap();
        assert!(!earlier_names_left(earlier.path()).unwrap());

        let both = tempfile::tempdir().unwrap();
        write_private(both.path(), "agent-id", &format!("{EARLIER_ID}\n"));
        let own = "0190a6e4-7a3b-7c3d-8e4f-ba9876543210";
        write_private(both.path(), "machine-id", &format!("{own}\n"));
        assert!(!earlier_names_left(both.path()).unwrap());
    }

    /// A machine that never paired kept an ID and no key: the ID moves
    /// alone.
    #[test]
    fn an_earlier_id_with_no_key_moves_alone() {
        let data = tempfile::tempdir().unwrap();
        write_private(data.path(), "agent-id", &format!("{EARLIER_ID}\n"));

        let id = moved(data.path()).unwrap();

        assert_eq!(id.as_uuid().hyphenated().to_string(), EARLIER_ID);
        assert_eq!(names(data.path()), ["machine-id"]);
    }

    /// A start that stopped between a move's link and its unlink left one
    /// file under two names, for the key or for the ID: the next start
    /// finishes the move, never taking the two names for two keys.
    #[cfg(unix)]
    #[test]
    fn a_move_that_stopped_between_its_link_and_unlink_is_finished_by_the_next_start() {
        for (from, to) in [("agent-key", "machine-key"), ("agent-id", "machine-id")] {
            let data = tempfile::tempdir().unwrap();
            write_private(data.path(), "agent-id", &format!("{EARLIER_ID}\n"));
            if from == "agent-id" {
                write_private(data.path(), "machine-key", KEY_CANARY);
            } else {
                write_private(data.path(), "agent-key", KEY_CANARY);
            }
            fs::hard_link(data.path().join(from), data.path().join(to)).unwrap();

            let id = moved(data.path()).unwrap();

            assert_eq!(id.as_uuid().hyphenated().to_string(), EARLIER_ID);
            assert_eq!(names(data.path()), ["machine-id", "machine-key"], "{from}");
            assert_eq!(
                fs::read_to_string(data.path().join("machine-key")).unwrap(),
                KEY_CANARY
            );
            assert!(!earlier_names_left(data.path()).unwrap(), "{from}");
        }
    }

    /// The key moves first, then the ID: a start that stopped between the
    /// two left `machine-key` and `agent-id`, and the next one finishes.
    #[test]
    fn a_move_that_stopped_after_the_key_is_finished_by_the_next_start() {
        let data = tempfile::tempdir().unwrap();
        write_private(data.path(), "agent-id", &format!("{EARLIER_ID}\n"));
        write_private(data.path(), "machine-key", KEY_CANARY);

        let id = moved(data.path()).unwrap();

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

        let lock = DataDirLock::acquire(data.path()).unwrap();
        let error = move_earlier_names(data.path(), &lock).unwrap_err();

        assert!(
            matches!(
                &error,
                MoveError::TwoKeys { earlier, own }
                    if *earlier == data.path().join("agent-key")
                        && *own == data.path().join("machine-key")
            ),
            "{error:?}"
        );
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

        let id = moved(data.path()).unwrap();

        assert_eq!(id.as_uuid().hyphenated().to_string(), own);
        assert_eq!(
            names(data.path()),
            ["agent-id", "agent-key", "machine-id", "machine-key"],
            "both IDs and keys are left; only what was staged is gone"
        );
        assert!(!earlier_names_left(data.path()).unwrap());
        let earlier = earlier_beside(data.path()).unwrap().unwrap();
        assert_eq!(earlier.as_uuid().hyphenated().to_string(), EARLIER_ID);
    }

    /// A move that finished but stopped before it deleted what an earlier
    /// build staged left `machine-id` alone with those files: the next
    /// start deletes them, so nothing is left to upgrade.
    #[test]
    fn staged_files_a_finished_move_left_are_deleted_by_the_next_start() {
        let data = tempfile::tempdir().unwrap();
        write_private(data.path(), "machine-id", &format!("{EARLIER_ID}\n"));
        write_private(data.path(), "machine-key", KEY_CANARY);
        write_private(data.path(), "agent-key.0190a6e47a3b7c3d.tmp", KEY_CANARY);
        write_private(data.path(), "agent-id.0190a6e47a3b7c3d.tmp", EARLIER_ID);
        assert!(earlier_names_left(data.path()).unwrap());

        moved(data.path()).unwrap();

        assert_eq!(names(data.path()), ["machine-id", "machine-key"]);
        assert!(!earlier_names_left(data.path()).unwrap());
    }

    /// Only what an earlier build staged is deleted, `.tmp` files: a file
    /// the person keeps beside the key, as `agent-key.bak`, stays, and is
    /// nothing left to upgrade.
    #[test]
    fn a_file_beside_the_key_that_no_build_staged_is_kept() {
        let data = earlier_data_folder();
        write_private(data.path(), "agent-key.bak", KEY_CANARY);

        moved(data.path()).unwrap();

        assert_eq!(
            names(data.path()),
            ["agent-key.bak", "machine-id", "machine-key"]
        );
        assert!(!earlier_names_left(data.path()).unwrap());
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
