//! The configuration file's text with every value that holds a secret
//! masked (I4), as a proposal's review shows it: each string is read as
//! TOML reads it, so a secret is found however the file writes it, escaped
//! (`\"`, `\\`, `\u0007`), over several lines, or with an escape a person
//! wrote where none was needed.

use domain::secrets::Redactor;
use toml_edit::{DocumentMut, Item, Table, Value};

/// `text` with each string value that holds a secret of `redactors`
/// written again as its masked text, and everything else as it was. A text
/// that isn't TOML is masked whole: nothing in it can be read as a value.
pub(crate) fn masked(text: &str, redactors: &[Redactor]) -> String {
    let Ok(mut document) = text.parse::<DocumentMut>() else {
        return Redactor::masking_everything().redact(text);
    };
    mask_table(document.as_table_mut(), redactors);
    document.to_string()
}

fn mask_table(table: &mut Table, redactors: &[Redactor]) {
    for (_, item) in table.iter_mut() {
        mask_item(item, redactors);
    }
}

fn mask_item(item: &mut Item, redactors: &[Redactor]) {
    match item {
        Item::Value(value) => mask_value(value, redactors),
        Item::Table(table) => mask_table(table, redactors),
        Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                mask_table(table, redactors);
            }
        }
        Item::None => {}
    }
}

fn mask_value(value: &mut Value, redactors: &[Redactor]) {
    match value {
        Value::String(string) => {
            let masked = Redactor::redact_together(redactors, string.value());
            if masked != *string.value() {
                let decor = string.decor().clone();
                let mut replaced = Value::from(masked);
                *replaced.decor_mut() = decor;
                *value = replaced;
            }
        }
        Value::Array(array) => {
            for value in array.iter_mut() {
                mask_value(value, redactors);
            }
        }
        Value::InlineTable(table) => {
            for (_, value) in table.iter_mut() {
                mask_value(value, redactors);
            }
        }
        Value::Integer(_) | Value::Float(_) | Value::Boolean(_) | Value::Datetime(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use domain::secrets::SecretValue;

    use super::*;

    fn redactors(secret: &str) -> Vec<Redactor> {
        vec![Redactor::new([&SecretValue::new(secret)])]
    }

    #[test]
    fn a_secret_is_masked_however_the_file_escapes_it() {
        let text = concat!(
            "# kept as written\n",
            "a = \"\\u0063anary-one\"\n",
            "b = \"\"\"\nca\\\n  nary-one\"\"\"\n",
            "c = { d = ['canary-one', 'other'] }\n",
            "[[e]]\n",
            "f = \"x canary-one y\" # a comment\n",
        );
        let masked = masked(text, &redactors("canary-one"));
        assert!(!masked.contains("anary"), "{masked}");
        assert!(masked.contains("# kept as written"), "{masked}");
        assert!(masked.contains("'other'"), "{masked}");
        assert!(masked.contains("# a comment"), "{masked}");
    }

    #[test]
    fn a_text_with_no_secret_is_left_as_it_was() {
        let text = "a = \"\\u0063anary\" # note\n[b]\nc = 1\n";
        assert_eq!(masked(text, &redactors("other")), text);
    }

    #[test]
    fn a_text_that_is_not_toml_is_masked_whole() {
        let masked = masked("a = \"canary-one", &redactors("unrelated"));
        assert!(!masked.contains("canary"), "{masked}");
    }
}
