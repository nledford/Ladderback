//! The text field as the browser build draws it (components §4.5), run by
//! `just test-wasm` under Node: Node has no DOM, so each field is rendered to
//! HTML, which is the markup the page mounts.

use std::sync::Arc;

use l10n::Localizer;
use leptos::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

use super::Kind;
use crate::components::TextField;

/// A list's fields, each with an id built from its place in the list, a
/// description and an error, as HTML.
fn list_of(kind: Kind) -> String {
    Owner::new().with(|| {
        let localizer = Arc::new(Localizer::new(&[]));
        (1..=2_u8)
            .map(|change| {
                view! {
                    <TextField
                        id=format!("demo-reason-{change}")
                        localizer=Arc::clone(&localizer)
                        label=format!("Reason {change}")
                        description="Why"
                        error=RwSignal::new(Some("Too long".to_owned()))
                        value=RwSignal::new(String::new())
                        kind=kind
                    />
                }
            })
            .collect_view()
            .to_html()
    })
}

#[wasm_bindgen_test]
fn a_field_of_a_list_wires_its_parts_by_the_id_built_for_it() {
    for (kind, element) in [(Kind::Line, "input"), (Kind::Lines { rows: 3 }, "textarea")] {
        let html = list_of(kind);
        for change in 1..=2 {
            let id = format!("demo-reason-{change}");
            for wired in [
                format!(r#"<label id="{id}-label" for="{id}">"#),
                format!(r#"<{element} id="{id}" "#),
                format!(r#"aria-describedby="{id}-description {id}-error""#),
                format!(r#"<p id="{id}-description">"#),
                format!(r#"<p id="{id}-error" class="form-error""#),
            ] {
                assert_eq!(
                    html.matches(&wired).count(),
                    1,
                    "{kind:?}: {wired} in {html}"
                );
            }
        }
    }
}
