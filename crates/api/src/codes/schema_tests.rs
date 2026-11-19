use schemars::{JsonSchema, SchemaGenerator, json_schema, schema_for};
use serde_json::json;

use crate::*;

/// `T`'s schema on its own is titled `name`, and in another schema it
/// is written in place as text described by `description`, never as a
/// reference to a definition.
fn is_text_written_in_place<T: JsonSchema>(name: &str, description: &str) {
    let alone = schema_for!(T);
    assert_eq!(alone.get("title"), Some(&json!(name)), "{alone:?}");
    assert_eq!(alone.get("type"), Some(&json!("string")), "{alone:?}");
    let mut generator = SchemaGenerator::default();
    assert_eq!(
        generator.subschema_for::<T>(),
        json_schema!({"type": "string", "description": description})
    );
    assert!(
        generator.definitions().is_empty(),
        "{:?}",
        generator.definitions()
    );
}

#[test]
fn an_error_code_is_text_written_in_place() {
    is_text_written_in_place::<ErrorCode>(
        "ErrorCode",
        "Why it failed, in kebab-case, such as `repository-unknown`.",
    );
}

#[test]
fn a_warning_code_is_text_written_in_place() {
    is_text_written_in_place::<WarningCode>(
        "WarningCode",
        "What to know, in kebab-case, such as `glob-never-matches`.",
    );
}
