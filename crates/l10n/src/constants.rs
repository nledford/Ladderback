//! How a message ID becomes a constant name. Shared by `build.rs`, which
//! generates `msg.rs` with it, and, under `cfg(test)`, by the library, because
//! `cargo test` can't reach a build script's own functions.

use std::collections::BTreeMap;

/// The constant the generator emits for the list of every message, so no
/// message may be given this name.
pub(crate) const RESERVED: &str = "ALL";

/// The constant for a message ID: capitals, with `_` for `-`
/// (`cli-listening` is `CLI_LISTENING`).
pub(crate) fn constant_name(id: &str) -> String {
    id.replace('-', "_").to_uppercase()
}

/// The `(constant, id)` pairs of `ids`, sorted by constant. Each item is the
/// file an ID was defined in, then the ID. Fails, naming the IDs and the file,
/// when an ID is defined twice, when two IDs map to one constant (`a-b` and
/// `a_b`), or when an ID maps to [`RESERVED`].
pub(crate) fn constants<'a>(
    ids: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<(String, String)>, String> {
    // Constant -> ID, so the output is sorted whatever the order of `ids`.
    let mut constants: BTreeMap<String, String> = BTreeMap::new();
    for (file, id) in ids {
        let name = constant_name(id);
        if name == RESERVED {
            return Err(format!(
                "`{id}` in {file} would be the constant {name}, which is the list of every message \
                 that `msg.rs` also defines; give the message another name"
            ));
        }
        if let Some(first) = constants.insert(name.clone(), id.to_owned()) {
            let reason = if first == id {
                "defined twice".to_owned()
            } else {
                format!("given the constant {name}, which `{first}` already has")
            };
            return Err(format!("`{id}` in {file} is {reason}"));
        }
    }
    Ok(constants.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(ids: &[&'static str]) -> Vec<(&'static str, &'static str)> {
        ids.iter().map(|id| ("cli.ftl", *id)).collect()
    }

    #[test]
    fn an_id_is_named_in_capitals_with_underscores() {
        assert_eq!(constant_name("cli-listening"), "CLI_LISTENING");
        assert_eq!(constant_name("a_b"), "A_B");
    }

    #[test]
    fn two_ids_that_share_a_constant_are_rejected_naming_both() {
        let error = constants(pairs(&["a-b", "a_b"])).unwrap_err();
        assert!(error.contains("`a-b`"), "{error}");
        assert!(error.contains("`a_b`"), "{error}");
        assert!(error.contains("A_B"), "{error}");
    }

    #[test]
    fn an_id_defined_twice_is_rejected() {
        let error = constants([("cli.ftl", "a-b"), ("common.ftl", "a-b")]).unwrap_err();
        assert!(error.contains("`a-b`"), "{error}");
        assert!(error.contains("common.ftl"), "{error}");
        assert!(error.contains("twice"), "{error}");
    }

    #[test]
    fn the_id_all_is_rejected_because_the_generated_list_has_that_name() {
        let error = constants(pairs(&["cli-listening", "all"])).unwrap_err();
        assert!(error.contains("`all`"), "{error}");
        assert!(error.contains("ALL"), "{error}");
        assert!(error.contains("list"), "{error}");
    }

    #[test]
    fn every_spelling_of_all_is_rejected() {
        assert!(constants(pairs(&["ALL"])).is_err());
    }

    #[test]
    fn an_id_that_only_starts_with_all_is_accepted() {
        let constants = constants(pairs(&["all-files"])).unwrap();
        assert_eq!(
            constants,
            [("ALL_FILES".to_owned(), "all-files".to_owned())]
        );
    }

    #[test]
    fn the_constants_are_sorted_whatever_the_order_of_the_ids() {
        let forward = constants(pairs(&["b-one", "a-two", "c-three"])).unwrap();
        let backward = constants(pairs(&["c-three", "a-two", "b-one"])).unwrap();
        assert_eq!(forward, backward);
        let names: Vec<&str> = forward.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["A_TWO", "B_ONE", "C_THREE"]);
        assert_eq!(forward[0].1, "a-two");
    }
}
