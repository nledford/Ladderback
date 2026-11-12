//! Recording a repository ID in the file as a person wrote it (§6.1,
//! decision NN): one key added to one entry, every other byte left alone,
//! and the result read back before it may replace the file.

use std::path::Path;

use app::ports::{NotInPlace, RecordError};
use domain::planning::AgentConfig;
use domain::{RepositoryAlias, RepositoryId};
use toml_edit::{DocumentMut, Item, Value};

use crate::schema::{Schemas, over_cap, parse};

/// The key an entry records its repository ID under.
const KEY: &str = "repository-id";

/// The byte order mark a file may start with.
const BOM: &str = "\u{feff}";

/// `text` with `repository-id` set to `id` in the entry `alias`, and nothing
/// else in it changed. The edit is made on the text without a byte order
/// mark and with `\n` line endings, which is what the editor keeps, and the
/// file's own mark and line ending are put back: a file whose lines end
/// both ways has no own ending, and isn't edited.
pub(crate) fn with_id(
    text: &str,
    alias: &RepositoryAlias,
    id: &RepositoryId,
) -> Result<String, NotInPlace> {
    let (bom, body) = text
        .strip_prefix(BOM)
        .map_or(("", text), |body| (BOM, body));
    let lines = body.matches('\n').count();
    let crlf = body.matches("\r\n").count();
    let edited = if crlf == 0 {
        added(body, alias, id)?
    } else if crlf == lines {
        added(&body.replace("\r\n", "\n"), alias, id)?.replace('\n', "\r\n")
    } else {
        return Err(NotInPlace::Form);
    };
    Ok(format!("{bom}{edited}"))
}

/// `text`, with `\n` line endings, with the key added to the entry `alias`
/// after its last key and written as that key is: indented as it is in a
/// table, and before what follows it in an inline table.
fn added(text: &str, alias: &RepositoryAlias, id: &RepositoryId) -> Result<String, NotInPlace> {
    let mut doc: DocumentMut = text.parse().map_err(|_| NotInPlace::Form)?;
    let entry = doc
        .get_mut("repositories")
        .and_then(|repositories| repositories.as_table_like_mut())
        .and_then(|repositories| repositories.get_mut(alias.as_str()))
        .ok_or(NotInPlace::Form)?;
    match entry {
        Item::Table(table) if !table.contains_key(KEY) => {
            let indent = table
                .iter()
                .filter(|(_, item)| item.is_value())
                .last()
                .and_then(|(key, _)| table.key(key))
                .and_then(|key| key.leaf_decor().prefix())
                .and_then(|prefix| prefix.as_str())
                // The key's own line: the lines above it, blank or a comment,
                // stay where they are.
                .and_then(|prefix| prefix.rsplit('\n').next())
                .map(str::to_owned);
            table.insert(KEY, toml_edit::value(id.as_str()));
            if let (Some(indent), Some(mut key)) = (indent, table.key_mut(KEY)) {
                key.leaf_decor_mut().set_prefix(indent);
            }
        }
        Item::Value(Value::InlineTable(table)) if !table.contains_key(KEY) => {
            let last = table.iter().last().map(|(key, _)| key.to_owned());
            let closing = last
                .and_then(|key| table.get_mut(&key))
                .map(|value| {
                    let suffix = value
                        .decor()
                        .suffix()
                        .and_then(|suffix| suffix.as_str())
                        .unwrap_or_default()
                        .to_owned();
                    value.decor_mut().set_suffix("");
                    suffix
                });
            table.insert(KEY, id.as_str().into());
            if let (Some(closing), Some(value)) = (closing, table.get_mut(KEY)) {
                value.decor_mut().set_suffix(closing);
            }
        }
        _ => return Err(NotInPlace::Form),
    }
    Ok(doc.to_string())
}

/// Whether `edited` may replace `original`: it is `original` with one run of
/// text added and every other byte as it was, it is no larger than the store
/// reads, and it parses as `expected`, as the store's load would read it.
pub(crate) fn reads_back(
    path: &Path,
    original: &str,
    edited: &str,
    expected: &AgentConfig,
    schemas: &Schemas,
) -> Result<(), RecordError> {
    let refused = Err(RecordError::NotInPlace(NotInPlace::ReadBack));
    if !one_run_added(original.as_bytes(), edited.as_bytes()) {
        return refused;
    }
    if over_cap(edited.len()) {
        return Err(RecordError::NotInPlace(NotInPlace::TooLarge));
    }
    match parse(path, edited, schemas) {
        Ok(parsed) if parsed.config == *expected => Ok(()),
        _ => refused,
    }
}

/// Whether `edited` is `original` with one run of bytes, at least one,
/// added somewhere: it starts with what `original` has before the first
/// byte they differ in, ends with the rest of `original`, and is longer.
fn one_run_added(original: &[u8], edited: &[u8]) -> bool {
    let before = original
        .iter()
        .zip(edited)
        .take_while(|(was, is)| was == is)
        .count();
    let after = original.get(before..).unwrap_or_default();
    edited.len() > original.len() && edited.ends_with(after)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use app::ports::{NotInPlace, RecordError};
    use domain::{RepositoryAlias, RepositoryId};

    use super::{reads_back, with_id};
    use crate::schema::{MAX_CONFIG_BYTES, Schemas, parse};

    const FILE: &str = "\
schema = 1
# the NAS
[repositories.nas]
repository = \"/srv/demo\"   # by hand
password-env = \"DEMO\"

[repositories.usb]
repository = \"/media/demo\"
password-env = \"DEMO_USB\"
";

    fn alias(name: &str) -> RepositoryAlias {
        RepositoryAlias::parse(name).expect("an alias")
    }

    fn id() -> RepositoryId {
        RepositoryId::parse(&"07".repeat(32)).expect("a repository ID")
    }

    fn id_line() -> String {
        format!("repository-id = \"{}\"\n", "07".repeat(32))
    }

    /// The configuration in `FILE` with the ID recorded for `nas`.
    fn expected() -> domain::planning::AgentConfig {
        let config = parse(Path::new("config.toml"), FILE, &Schemas::production())
            .expect("the file parses")
            .config;
        let read = config
            .repository(&alias("nas"))
            .cloned()
            .expect("the entry");
        config
            .record_id(&alias("nas"), &read, &id())
            .expect("the rule records it")
    }

    fn checked(edited: &str) -> Result<(), RecordError> {
        reads_back(
            Path::new("config.toml"),
            FILE,
            edited,
            &expected(),
            &Schemas::production(),
        )
    }

    fn added_to_nas() -> String {
        FILE.replacen(
            "password-env = \"DEMO\"\n",
            &format!("password-env = \"DEMO\"\n{}", id_line()),
            1,
        )
    }

    #[test]
    fn the_id_goes_after_the_last_key_of_its_entry() {
        assert_eq!(with_id(FILE, &alias("nas"), &id()), Ok(added_to_nas()));
    }

    #[test]
    fn an_entry_that_is_not_there_is_not_added() {
        assert_eq!(with_id(FILE, &alias("cloud"), &id()), Err(NotInPlace::Form));
        assert_eq!(
            with_id("schema = 1\n", &alias("nas"), &id()),
            Err(NotInPlace::Form)
        );
        assert_eq!(
            with_id("schema = 1\nrepositories = 3\n", &alias("nas"), &id()),
            Err(NotInPlace::Form)
        );
        assert_eq!(
            with_id(
                "schema = 1\n[repositories]\nnas = 3\n",
                &alias("nas"),
                &id()
            ),
            Err(NotInPlace::Form)
        );
    }

    #[test]
    fn an_entry_that_has_the_key_already_is_not_given_another() {
        let inline = format!(
            "schema = 1\n[repositories]\nnas = {{ repository = \"/srv/demo\", password-env = \"DEMO\", repository-id = \"{}\" }}\n",
            "07".repeat(32)
        );
        for recorded in [added_to_nas(), inline] {
            assert_eq!(
                with_id(&recorded, &alias("nas"), &id()),
                Err(NotInPlace::Form),
                "{recorded}"
            );
        }
    }

    #[test]
    fn the_edit_that_adds_the_id_alone_reads_back() {
        assert_eq!(checked(&added_to_nas()), Ok(()));
    }

    #[test]
    fn an_edit_that_changes_another_setting_is_refused() {
        let also_moved = added_to_nas().replace("/media/demo", "/media/other");
        assert_eq!(
            checked(&also_moved),
            Err(RecordError::NotInPlace(NotInPlace::ReadBack))
        );
    }

    #[test]
    fn an_edit_that_changes_only_how_the_file_is_written_is_refused() {
        for reformatted in [
            added_to_nas().replace("   # by hand", ""),
            added_to_nas().replace("# the NAS\n", ""),
            added_to_nas().replacen("schema = 1\n", "schema = 1\n\n", 1),
            added_to_nas().replacen(&id_line(), "", 1).replace(
                "password-env = \"DEMO_USB\"\n",
                &format!("password-env = \"DEMO_USB\"\n{}", id_line()),
            ),
        ] {
            assert_eq!(
                checked(&reformatted),
                Err(RecordError::NotInPlace(NotInPlace::ReadBack)),
                "{reformatted}"
            );
        }
    }

    #[test]
    fn an_edit_that_is_not_a_configuration_is_refused() {
        for broken in [
            format!("{}[[plans]]\n", added_to_nas()),
            format!("{}\"", added_to_nas()),
            FILE.to_owned(),
        ] {
            assert_eq!(
                checked(&broken),
                Err(RecordError::NotInPlace(NotInPlace::ReadBack)),
                "{broken}"
            );
        }
    }

    #[test]
    fn an_edit_that_adds_nothing_is_refused_even_when_it_reads_as_expected() {
        let recorded = added_to_nas();
        let config = parse(Path::new("config.toml"), &recorded, &Schemas::production())
            .expect("the file parses")
            .config;
        assert_eq!(
            reads_back(
                Path::new("config.toml"),
                &recorded,
                &recorded,
                &config,
                &Schemas::production(),
            ),
            Err(RecordError::NotInPlace(NotInPlace::ReadBack))
        );
    }

    /// `text` with the ID recorded for `nas`, checked against the
    /// configuration it holds.
    fn recorded_in(text: &str) -> Result<String, RecordError> {
        let config = parse(Path::new("config.toml"), text, &Schemas::production())
            .expect("the file parses")
            .config;
        let read = config
            .repository(&alias("nas"))
            .cloned()
            .expect("the entry");
        let expected = config
            .record_id(&alias("nas"), &read, &id())
            .expect("the rule records it");
        let edited = with_id(text, &alias("nas"), &id()).map_err(RecordError::NotInPlace)?;
        reads_back(
            Path::new("config.toml"),
            text,
            &edited,
            &expected,
            &Schemas::production(),
        )?;
        Ok(edited)
    }

    #[test]
    fn a_file_whose_lines_end_in_crlf_gains_a_line_that_ends_so() {
        let crlf = FILE.replace('\n', "\r\n");
        assert_eq!(
            recorded_in(&crlf),
            Ok(added_to_nas().replace('\n', "\r\n"))
        );
    }

    #[test]
    fn a_file_that_starts_with_a_byte_order_mark_keeps_it() {
        for file in [FILE.to_owned(), FILE.replace('\n', "\r\n")] {
            let marked = format!("\u{feff}{file}");
            let expected = if file.contains('\r') {
                added_to_nas().replace('\n', "\r\n")
            } else {
                added_to_nas()
            };
            assert_eq!(recorded_in(&marked), Ok(format!("\u{feff}{expected}")));
        }
    }

    #[test]
    fn a_file_whose_lines_end_both_ways_is_not_edited() {
        for mixed in [
            FILE.replacen('\n', "\r\n", 1),
            FILE.replace('\n', "\r\n").replacen("\r\n", "\n", 1),
        ] {
            assert_eq!(
                with_id(&mixed, &alias("nas"), &id()),
                Err(NotInPlace::Form),
                "{mixed:?}"
            );
        }
    }

    #[test]
    fn the_id_is_indented_as_the_key_before_it_without_the_lines_above_that_key() {
        for above in ["", "\n  # the password\n"] {
            let indented = FILE
                .replace("repository = ", "  repository = ")
                .replace("password-env = ", &format!("{above}  password-env = "));
            let expected = indented.replacen(
                "  password-env = \"DEMO\"\n",
                &format!("  password-env = \"DEMO\"\n  {}", id_line()),
                1,
            );
            assert_eq!(recorded_in(&indented), Ok(expected), "{above:?}");
        }
    }

    #[test]
    fn an_inline_entry_gets_the_id_before_its_closing_brace() {
        let id = "07".repeat(32);
        for (inline, expected) in [
            (
                "{ repository = \"/srv/demo\", password-env = \"DEMO\" }",
                format!(
                    "{{ repository = \"/srv/demo\", password-env = \"DEMO\", repository-id = \"{id}\" }}"
                ),
            ),
            (
                "{repository = \"/srv/demo\", password-env = \"DEMO\"}",
                format!(
                    "{{repository = \"/srv/demo\", password-env = \"DEMO\", repository-id = \"{id}\"}}"
                ),
            ),
        ] {
            let file = format!("schema = 1\n[repositories]\nnas = {inline}\n");
            assert_eq!(
                recorded_in(&file),
                Ok(format!("schema = 1\n[repositories]\nnas = {expected}\n"))
            );
        }
    }

    /// `FILE` with a comment at its end that makes it `len` bytes long.
    fn padded_to(len: usize) -> String {
        let comment = len - FILE.len() - "# \n".len();
        format!("{FILE}# {}\n", "x".repeat(comment))
    }

    #[test]
    fn an_edit_that_would_make_the_file_larger_than_the_store_reads_is_refused() {
        let cap = usize::try_from(MAX_CONFIG_BYTES).expect("a size");
        let line = id_line().len();
        assert_eq!(
            recorded_in(&padded_to(cap - line + 1)),
            Err(RecordError::NotInPlace(NotInPlace::TooLarge))
        );
        let at_cap = recorded_in(&padded_to(cap - line)).expect("a file at the cap is read");
        assert_eq!(at_cap.len(), cap);
    }
}
