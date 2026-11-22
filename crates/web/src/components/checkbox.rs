//! The checkbox (components §4.5): a native `<input type="checkbox">` drawn
//! from the tokens, its label beside it and its description and error under
//! the label. [`rules`] decides how its box and its mark are drawn, and a
//! radio's, host-tested; this module is the glue that draws it.

use std::sync::Arc;

use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::Icon;
use super::text_field::{
    Access,
    rules::{described_by, description_id, error_id, invalid},
};

pub(crate) mod rules;

use rules::Shape;

/// A checkbox: a native `<input type="checkbox">` with its visible label
/// beside it, which is also its click target, then, under the label, an
/// optional description and its error while it has one, both wired to it
/// with `aria-describedby` and read after its name, as a
/// [`TextField`](super::TextField)'s are; its error marks it invalid
/// (`aria-invalid`), edged thicker in the Error tone beside the words. The
/// box is drawn from the tokens, never under 24px and growing with the
/// text: edged in Field Edge, and filled in Ladder Blue under a check in
/// the page's color once on. `checked` holds the choice, so the caller
/// reads and resets it. Space turns it on and off, and the browser tells a
/// screen reader its state, so nothing here announces it.
///
/// Like a text field's, its error is said by its page, through the
/// announcer or an error summary, and only once the form is checked.
#[component]
pub fn Checkbox(
    #[prop(into)] id: Arc<str>,
    label: String,
    #[prop(optional)] description: Option<String>,
    #[prop(optional, into)] error: Signal<Option<String>>,
    checked: RwSignal<bool>,
) -> impl IntoView {
    let has_description = description.is_some();
    let has_error = move || error.with(Option::is_some);
    let description = description.map(|text| {
        view! {
            <p id=description_id(&id) class=rules::UNDER_LABEL>
                {text}
            </p>
        }
    });
    let (described, erred) = (Arc::clone(&id), Arc::clone(&id));
    view! {
        <div class=rules::LINE>
            <span class=rules::CELL>
                <input
                    id=Arc::clone(&id)
                    type="checkbox"
                    class=rules::box_class(Shape::Square)
                    prop:checked=move || checked.get()
                    on:change=move |event| checked.set(event_target_checked(&event))
                    aria-describedby=move || {
                        described_by(
                            &described,
                            has_description,
                            Access::Editable,
                            has_error(),
                            None,
                        )
                    }
                    aria-invalid=move || invalid(has_error())
                />
                <span class=rules::mark_class(Shape::Square) aria-hidden="true">
                    <Icon glyph=LucideGlyph::Check />
                </span>
            </span>
            <label for=id class=rules::LABEL>
                {label}
            </label>
            {description}
            {move || {
                error
                    .get()
                    .map(|text| {
                        view! {
                            <p id=error_id(&erred) class=rules::ERROR_UNDER_LABEL>
                                {text}
                            </p>
                        }
                    })
            }}
        </div>
    }
}
