//! The badge as the browser build draws it (components §4.5), run by
//! `just test-wasm` under Node: Node has no DOM, so each badge is rendered to
//! HTML, which is the markup the page mounts.

use lepticons_data::LucideGlyph;
use leptos::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

use super::rules::{BadgeTone, badge_class};
use crate::components::Badge;

#[wasm_bindgen_test]
fn a_badge_says_its_words_on_its_tones_pill_with_no_role_nothing_to_focus_and_no_icon_unless_given_one()
 {
    for tone in BadgeTone::ALL {
        let html =
            Owner::new().with(|| view! { <Badge label="Pending changes 3" tone=tone /> }.to_html());
        assert!(
            html.starts_with(&format!(r#"<span class="{}">"#, badge_class(tone))),
            "{tone:?}: {html}"
        );
        assert!(html.contains("Pending changes 3"), "{tone:?}: {html}");
        for absent in ["<svg", "role=", "tabindex", "aria-hidden", "aria-live"] {
            assert!(!html.contains(absent), "{tone:?}, {absent}: {html}");
        }
    }
    let html = Owner::new().with(|| view! { <Badge label="Demo tag" /> }.to_html());
    assert!(
        html.starts_with(&format!(
            r#"<span class="{}">"#,
            badge_class(BadgeTone::Secondary)
        )),
        "gray unless its page names a tone: {html}"
    );
}

#[wasm_bindgen_test]
fn a_badges_icon_stands_before_its_words_in_their_color_a_decoration_a_screen_reader_skips() {
    let html = Owner::new()
        .with(|| view! { <Badge label="Pending changes 3" icon=LucideGlyph::Pencil /> }.to_html());
    assert_eq!(html.matches("<svg").count(), 1, "one icon: {html}");
    let (before, after) = html.split_once("</svg>").expect("the badge has an icon");
    let tag = before
        .split_once("<svg")
        .and_then(|(_, svg)| svg.split_once('>'))
        .map(|(attributes, _)| attributes)
        .expect("the icon's opening tag");
    assert!(tag.contains(r#"aria-hidden="true""#), "{tag}");
    assert!(tag.contains(r#"focusable="false""#), "{tag}");
    assert!(tag.contains(r#"stroke="currentColor""#), "{tag}");
    assert!(
        before.ends_with(&lepticons_data::Glyph::svg(&LucideGlyph::Pencil)),
        "{html}"
    );
    assert!(
        after.contains("Pending changes 3"),
        "after the icon: {html}"
    );
    assert!(
        !before.contains("Pending changes 3"),
        "after the icon: {html}"
    );
}
