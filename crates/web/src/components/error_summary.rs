use leptos::prelude::*;

use super::{focus_element, sentence_view};
use crate::repositories::Sentence;

/// One problem of an [`ErrorSummary`]: what is wrong, and the field it is
/// about when it is about one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SummaryItem {
    pub(crate) field: Option<&'static str>,
    pub(crate) sentence: Sentence,
}

/// The error summary of a form that failed to submit (§4.5): a heading that
/// takes focus as it appears, so a screen reader reads it and a keyboard
/// reader is at the top of the list, and one entry per problem that links to
/// its field. The engine's own words in a sentence are marked as English
/// (3.1.2). `advice` follows the list when there is any. It is not a live
/// region: focus is what tells the reader.
#[component]
pub(crate) fn ErrorSummary(
    heading: String,
    items: Vec<SummaryItem>,
    advice: Option<String>,
) -> impl IntoView {
    let heading_ref = NodeRef::<leptos::html::H2>::new();
    Effect::new(move |_| {
        if let Some(heading) = heading_ref.get() {
            // A failed focus leaves focus where it was.
            let _ = heading.focus();
        }
    });
    view! {
        <div class="error-summary">
            <h2 tabindex="-1" node_ref=heading_ref>
                {heading}
            </h2>
            <ul>
                {items
                    .into_iter()
                    .map(|item| {
                        let SummaryItem { field, sentence } = item;
                        let shown = sentence_view(sentence);
                        match field {
                            Some(field) => {
                                view! {
                                    <li>
                                        // The router takes a click on any same-origin
                                        // link and would only scroll, so the link moves
                                        // focus itself and stops the click.
                                        <a
                                            href=format!("#{field}")
                                            on:click=move |event| {
                                                event.prevent_default();
                                                focus_element(field);
                                            }
                                        >
                                            {shown}
                                        </a>
                                    </li>
                                }
                                    .into_any()
                            }
                            None => view! { <li>{shown}</li> }.into_any(),
                        }
                    })
                    .collect_view()}
            </ul>
            {advice.map(|advice| view! { <p>{advice}</p> })}
        </div>
    }
}
