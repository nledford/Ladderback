//! Golden files: a config file written in every version of the format there
//! has been, which every build from then on must read as the same
//! configuration (technical design §6.1).

mod support;

use std::path::{Path, PathBuf};

use config_adapter::{CURRENT_SCHEMA, Schemas, parse, render};
use support::model::expected;

fn golden_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("the golden folder")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('v') && name.ends_with(".toml"))
        })
        .collect();
    files.sort();
    files
}

#[test]
fn every_golden_file_is_read_as_the_same_configuration() {
    for path in golden_files() {
        let text = std::fs::read_to_string(&path).expect("a golden file");
        let parsed = parse(&path, &text, &Schemas::production())
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(parsed.config, expected(), "{}", path.display());
        let current = text.starts_with(&format!("schema = {CURRENT_SCHEMA}\n"));
        assert_eq!(parsed.migrated, !current, "{}", path.display());
    }
}

#[test]
fn a_configuration_written_is_read_back_as_it_was() {
    let file = Path::new("/home/demo/.config/ladderback/config.toml");
    let text = render(file, &expected()).expect("the configuration can be written");
    assert!(
        text.starts_with(&format!("schema = {CURRENT_SCHEMA}\n")),
        "the schema comes first: {text}"
    );
    let again = parse(file, &text, &Schemas::production()).expect("what was written is valid");
    assert_eq!(again.config, expected());
    assert!(!again.migrated);
    assert_eq!(
        render(file, &again.config).expect("written again"),
        text,
        "writing is stable: the same configuration gives the same text"
    );
}
