//! The unused-ID lint (technical design §8.4, "Tests"): a message ID that no
//! product code names is a dead string, and dead strings pile up. Test code
//! doesn't count: a message only a test reads is still dead.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use l10n::msg;

use crate::support::sources::{exclude_test_modules, product_sources, scan};
use crate::support::workspace_root;

/// The constant the generator gives a message: `cli-listening` is `CLI_LISTENING`.
fn constant(id: &str) -> String {
    id.replace('-', "_").to_uppercase()
}

fn names(source: &str) -> BTreeSet<String> {
    scan(source).unwrap_or_else(|error| panic!("{error}")).names
}

#[test]
fn every_message_id_is_named_by_some_product_code() {
    assert!(!msg::ALL.is_empty());
    let root = workspace_root();
    let mut named = BTreeSet::new();
    let mut problems = Vec::new();
    let sources = product_sources(&root).unwrap_or_else(|error| panic!("{error}"));
    for path in sources {
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let source = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{shown}: {error}"));
        match scan(&source) {
            Ok(usage) => {
                if usage.globs {
                    problems.push(format!("{shown}: `msg::*` hides which messages it uses"));
                }
                named.extend(usage.names);
            }
            Err(error) => problems.push(format!("{shown}: not readable as Rust: {error}")),
        }
    }
    for id in msg::ALL {
        let constant = constant(id.as_str());
        if !named.contains(&constant) {
            problems.push(format!(
                "`{}` (msg::{constant}) is named by no product code",
                id.as_str()
            ));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn a_path_through_msg_names_its_constant() {
    let source = "
        fn banner() -> MessageId { msg::CLI_LISTENING }
        fn qualified() { let _ = l10n::msg::CLI_CANNOT_LISTEN; }
    ";
    let expected = ["CLI_CANNOT_LISTEN", "CLI_LISTENING"].map(str::to_owned);
    assert_eq!(names(source), BTreeSet::from(expected));
}

#[test]
fn a_use_declaration_names_what_it_imports() {
    let source = "
        use l10n::msg::{CLI_A, CLI_B as B};
        use l10n::msg::CLI_C;
    ";
    let named = names(source);
    for expected in ["CLI_A", "CLI_B", "CLI_C"] {
        assert!(named.contains(expected), "{expected} in {named:?}");
    }
}

#[test]
fn comments_strings_and_docs_name_nothing() {
    let source = r#"
        // msg::CLI_A
        /* msg::CLI_B */
        /// msg::CLI_C
        fn documented() { let _ = "msg::CLI_D"; let _ = r"msg::CLI_E"; }
    "#;
    assert!(names(source).is_empty(), "{:?}", names(source));
}

#[test]
fn test_code_names_nothing() {
    let source = "
        #[cfg(test)]
        mod tests {
            use super::*;
            fn only_a_test() { msg::CLI_A; }
        }
        #[test]
        fn also_a_test() { msg::CLI_B; }
        fn product() { msg::CLI_C; }
    ";
    assert_eq!(names(source), BTreeSet::from(["CLI_C".to_owned()]));
}

#[test]
fn an_inner_cfg_test_attribute_makes_the_whole_file_test_code() {
    let source = "
        #![cfg(test)]
        fn helper() { msg::CLI_A; }
        const NAME: MessageId = msg::CLI_B;
    ";
    assert!(names(source).is_empty(), "{:?}", names(source));
}

#[test]
fn an_inner_cfg_test_attribute_makes_the_whole_mod_body_test_code() {
    let source = "
        mod support {
            #![cfg(test)]
            fn helper() { msg::CLI_A; }
        }
        mod product {
            fn helper() { msg::CLI_B; }
        }
    ";
    assert_eq!(names(source), BTreeSet::from(["CLI_B".to_owned()]));
}

#[test]
fn a_cfg_that_only_holds_under_test_marks_test_code() {
    let source = "
        #[cfg(all(test, unix))]
        fn a() { msg::CLI_A; }
        #[cfg(all(unix, test))]
        fn b() { msg::CLI_B; }
        #[cfg(all(unix, all(windows, test)))]
        fn c() { msg::CLI_C; }
        #[cfg(any(test, test))]
        fn d() { msg::CLI_D; }
        #[cfg(any(all(test, unix), test))]
        fn e() { msg::CLI_E; }
        #[cfg(test)]
        const F: MessageId = msg::CLI_F;
        fn product() { msg::CLI_G; }
    ";
    assert_eq!(names(source), BTreeSet::from(["CLI_G".to_owned()]));
}

#[test]
fn an_attribute_whose_path_ends_in_test_marks_test_code() {
    let source = r#"
        #[tokio::test]
        async fn a() { msg::CLI_A; }
        #[tokio::test(flavor = "multi_thread")]
        async fn b() { msg::CLI_B; }
        #[async_std::test]
        async fn c() { msg::CLI_C; }
        #[test]
        fn d() { msg::CLI_D; }
        fn product() { msg::CLI_E; }
    "#;
    assert_eq!(names(source), BTreeSet::from(["CLI_E".to_owned()]));
}

#[test]
fn a_test_attribute_marks_test_code_whatever_its_place_among_the_attributes() {
    let source = "
        #[tokio::test]
        #[ignore]
        async fn a() { msg::CLI_A; }
        #[ignore]
        #[tokio::test]
        async fn b() { msg::CLI_B; }
        #[allow(dead_code)]
        #[cfg(test)]
        #[derive(Debug)]
        struct C { name: &'static str }
        #[allow(dead_code)]
        #[cfg(all(test, unix))]
        fn d() { msg::CLI_D; }
        fn product() { msg::CLI_E; }
    ";
    assert_eq!(names(source), BTreeSet::from(["CLI_E".to_owned()]));
}

#[test]
fn a_cfg_that_can_hold_outside_test_is_product_code() {
    let source = r#"
        #[cfg(not(test))]
        fn a() { msg::CLI_A; }
        #[cfg(all(not(test), unix))]
        fn b() { msg::CLI_B; }
        #[cfg(any(test, feature = "x"))]
        fn c() { msg::CLI_C; }
        #[cfg(feature = "test")]
        fn d() { msg::CLI_D; }
        #[cfg(unix)]
        fn e() { msg::CLI_E; }
        #[cfg(any(test, unix))]
        fn f() { msg::CLI_F; }
    "#;
    let expected = ["CLI_A", "CLI_B", "CLI_C", "CLI_D", "CLI_E", "CLI_F"].map(str::to_owned);
    assert_eq!(names(source), BTreeSet::from(expected));
}

#[test]
fn attributes_that_do_not_mark_test_code_leave_the_item_counted() {
    let source = "
        #[cfg_attr(test, derive(Debug))]
        fn a() { msg::CLI_A; }
        #[derive(Debug)]
        fn b() { msg::CLI_B; }
        #[allow(dead_code)]
        fn c() { msg::CLI_C; }
        #[testing]
        fn d() { msg::CLI_D; }
    ";
    let expected = ["CLI_A", "CLI_B", "CLI_C", "CLI_D"].map(str::to_owned);
    assert_eq!(names(source), BTreeSet::from(expected));
}

#[test]
fn a_glob_import_is_flagged_because_it_hides_what_is_used() {
    let usage = scan("use l10n::msg::*;").unwrap_or_else(|error| panic!("{error}"));
    assert!(usage.globs);
    let named = scan("use l10n::msg::CLI_A;").unwrap_or_else(|error| panic!("{error}"));
    assert!(!named.globs);
}

#[test]
fn text_a_lexer_cannot_read_is_an_error_not_silence() {
    assert!(scan("fn broken() { let _ = \"unterminated; }").is_err());
    assert!(scan("fn broken() { ").is_err());
}

/// What `exclude_test_modules` keeps of `files`, given their texts by path.
fn product_files(files: &[(&str, &str)]) -> Result<Vec<String>, String> {
    let texts: BTreeMap<PathBuf, String> = files
        .iter()
        .map(|(path, text)| (PathBuf::from(path), (*text).to_owned()))
        .collect();
    let paths = files.iter().map(|(path, _)| PathBuf::from(path)).collect();
    exclude_test_modules(paths, |path: &Path| texts.get(path).cloned())
        .map(|kept| kept.iter().map(|path| path.display().to_string()).collect())
}

#[test]
fn a_file_a_cfg_test_mod_declaration_points_at_is_not_product_code() {
    let kept = product_files(&[
        ("src/lib.rs", "#[cfg(test)]\nmod support;\nmod product;"),
        ("src/support.rs", "fn helper() { msg::CLI_A; }"),
        ("src/product.rs", "fn real() { msg::CLI_B; }"),
    ]);
    assert_eq!(
        kept,
        Ok(vec!["src/lib.rs".to_owned(), "src/product.rs".to_owned()])
    );
}

#[test]
fn a_test_module_directory_with_a_mod_rs_is_not_product_code_either() {
    let kept = product_files(&[
        ("src/lib.rs", "#[cfg(test)] mod support;"),
        ("src/support/mod.rs", "mod helper;"),
        ("src/support/helper.rs", "fn h() { msg::CLI_A; }"),
    ]);
    assert_eq!(kept, Ok(vec!["src/lib.rs".to_owned()]));
}

#[test]
fn the_files_a_test_module_declares_are_test_code_too() {
    let kept = product_files(&[
        ("src/lib.rs", "#[cfg(test)] mod support;"),
        ("src/support.rs", "mod deeper;"),
        ("src/support/deeper.rs", "mod deepest;"),
        ("src/support/deeper/deepest.rs", "fn h() { msg::CLI_A; }"),
    ]);
    assert_eq!(kept, Ok(vec!["src/lib.rs".to_owned()]));
}

#[test]
fn a_cfg_that_only_holds_under_test_and_other_attributes_still_mark_the_module() {
    let kept = product_files(&[
        (
            "src/lib.rs",
            "#[cfg(all(test, unix))] mod a;
             #[cfg(test)] pub(crate) mod b;
             #[allow(dead_code)] #[cfg(test)] #[doc = \"x\"] mod c;
             #[cfg(not(test))] mod d;
             #[cfg(any(test, unix))] mod e;
             #[cfg(unix)] mod f;",
        ),
        ("src/a.rs", ""),
        ("src/b.rs", ""),
        ("src/c.rs", ""),
        ("src/d.rs", ""),
        ("src/e.rs", ""),
        ("src/f.rs", ""),
    ]);
    assert_eq!(
        kept,
        Ok(["src/lib.rs", "src/d.rs", "src/e.rs", "src/f.rs"]
            .map(str::to_owned)
            .to_vec())
    );
}

#[test]
fn a_module_file_is_looked_for_where_rust_looks_for_it() {
    // A file that isn't a crate root or a `mod.rs` keeps its modules in a
    // folder named for it; so does an inline module; a crate root doesn't.
    let kept = product_files(&[
        (
            "src/lib.rs",
            "mod store; mod outer { #[cfg(test)] mod helpers; }",
        ),
        ("src/store.rs", "#[cfg(test)] mod fixtures;"),
        ("src/store/fixtures.rs", ""),
        ("src/outer/helpers.rs", ""),
        ("src/bin/tool.rs", "#[cfg(test)] mod parts;"),
        ("src/bin/parts.rs", ""),
        ("src/helpers.rs", "fn not_a_test_module() { msg::CLI_A; }"),
    ]);
    assert_eq!(
        kept,
        Ok([
            "src/lib.rs",
            "src/store.rs",
            "src/bin/tool.rs",
            "src/helpers.rs"
        ]
        .map(str::to_owned)
        .to_vec())
    );
}

#[test]
fn what_an_inline_test_module_or_a_test_only_file_declares_is_test_code() {
    let kept = product_files(&[
        ("src/lib.rs", "#[cfg(test)] mod tests { mod deep; } mod x;"),
        ("src/tests/deep.rs", "fn h() { msg::CLI_A; }"),
        ("src/x.rs", "#![cfg(test)]\nmod y;"),
        ("src/x/y.rs", "fn h() { msg::CLI_B; }"),
    ]);
    assert_eq!(
        kept,
        Ok(["src/lib.rs", "src/x.rs"].map(str::to_owned).to_vec())
    );
}

#[test]
fn a_path_attribute_names_the_file_of_a_test_module() {
    let kept = product_files(&[
        (
            "src/lib.rs",
            "#[cfg(test)] #[path = \"steps/config.rs\"] mod config;
             mod inline { #[cfg(test)] #[path = \"x.rs\"] mod y; }",
        ),
        ("src/steps/config.rs", ""),
        ("src/inline/x.rs", ""),
    ]);
    assert_eq!(kept, Ok(vec!["src/lib.rs".to_owned()]));
}

#[test]
fn a_test_module_with_no_file_is_an_error_not_a_hole() {
    let kept = product_files(&[("src/lib.rs", "#[cfg(test)] mod support;")]);
    let error = kept.expect_err("a module with no file");
    assert!(error.contains("support"), "{error}");
}
