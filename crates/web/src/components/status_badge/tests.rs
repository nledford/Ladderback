//! The status badge as the browser build draws it (components §4.5), run by
//! `just test-wasm` under Node: Node has no DOM, so each badge is rendered to
//! HTML, which is the markup the page mounts.

use leptos::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

use super::rules::BadgeState;
use crate::components::StatusBadge;

/// The badge for `state`, labeled `label`, as HTML.
fn rendered(state: BadgeState, label: &str) -> String {
    let label = label.to_owned();
    Owner::new().with(|| view! { <StatusBadge state=state label=label /> }.to_html())
}

/// The opening tag of the one `<svg>` in `html` and what it draws.
fn icon(html: &str) -> (&str, &str) {
    let start = html.find("<svg").expect("the badge has an icon");
    let from_start = html.get(start..).expect("`find` answers a char boundary");
    let tag_end = start + from_start.find('>').unwrap();
    let end = html.find("</svg>").unwrap();
    assert_eq!(html.matches("<svg").count(), 1, "one icon: {html}");
    let tag = html
        .get(start..=tag_end)
        .expect("`>` is one byte, so its end is a boundary");
    let drawn = html
        .get(tag_end + 1..end)
        .expect("both ends come from `find`");
    (tag, drawn)
}

#[wasm_bindgen_test]
fn each_state_draws_an_icon_of_its_own_in_its_texts_color_which_a_screen_reader_skips() {
    let mut shapes = Vec::new();
    for state in BadgeState::ALL {
        let html = rendered(state, "Demo label");
        let (tag, shape) = icon(&html);
        assert!(tag.contains(r#"aria-hidden="true""#), "{state:?}: {tag}");
        assert!(tag.contains(r#"focusable="false""#), "{state:?}: {tag}");
        assert!(tag.contains(r#"stroke="currentColor""#), "{state:?}: {tag}");
        assert_eq!(
            shape,
            lepticons_data::Glyph::svg(&state.glyph()),
            "{state:?}"
        );
        shapes.push(shape.to_owned());
    }
    shapes.sort();
    shapes.dedup();
    assert_eq!(
        shapes.len(),
        BadgeState::ALL.len(),
        "no two states share an icon"
    );
}

#[wasm_bindgen_test]
fn the_label_stands_beside_the_icon_on_the_states_pill() {
    for state in BadgeState::ALL {
        let html = rendered(state, "Demo label");
        let pill = format!(r#"class="badge {}""#, state.pill());
        assert!(html.contains(&pill), "{state:?}: {html}");
        let after_icon = html.split("</svg>").nth(1).unwrap_or_default();
        assert!(after_icon.contains("Demo label"), "{state:?}: {html}");
    }
}
