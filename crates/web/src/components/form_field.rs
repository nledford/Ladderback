use leptos::prelude::*;

/// A form field: a label, an input, a description and an error, both wired to
/// the input with `aria-describedby`. `id` is the input's (an error summary's
/// link points at it), and `value` holds what is typed, so the caller reads
/// and clears it. The caller decides when `error` is `Some`; the caller
/// announces it politely (as the gallery's button does) or moves focus to an
/// [`ErrorSummary`](super::ErrorSummary). `kind` is the input's `type`: `password` for a secret,
/// which the browser then never shows. `autocomplete` is the input's token
/// (1.3.5): `off` unless the field has a known purpose, such as
/// `current-password` for the admin password, which a password manager then
/// fills in (3.3.8).
///
/// `lines` makes it a multi-line field, a native `<textarea>` that many rows
/// tall, for a list a person types an entry a line, such as a plan's folders:
/// Enter starts a line there, and never submits the form. Its label,
/// description and error are wired as a one-line field's are, and its `kind`
/// does not apply. Focus brings the whole of it into view (2.4.11): a browser
/// scrolls only far enough to show a textarea's caret, its first line, so a
/// field that starts near the viewport's bottom edge would stay mostly below
/// it.
#[component]
pub fn FormField(
    id: &'static str,
    label: String,
    description: String,
    error: RwSignal<Option<String>>,
    value: RwSignal<String>,
    #[prop(default = "text")] kind: &'static str,
    #[prop(default = "off")] autocomplete: &'static str,
    #[prop(optional)] lines: Option<u32>,
) -> impl IntoView {
    let described_by = format!("{id}-description");
    let error_id = format!("{id}-error");
    let error_in_error = error_id.clone();
    // What describes the input: its description, and its error while it
    // shows one.
    let description_ids = {
        let described_by = described_by.clone();
        move || {
            if error.get().is_some() {
                format!("{described_by} {error_id}")
            } else {
                described_by.clone()
            }
        }
    };
    let invalid = move || error.get().is_some().then_some("true");
    let input = match lines {
        Some(rows) => view! {
            <textarea
                id=id
                rows=rows
                autocomplete=autocomplete
                spellcheck="false"
                prop:value=move || value.get()
                on:input=move |event| value.set(event_target_value(&event))
                on:focus=move |event| {
                    let options = web_sys::ScrollIntoViewOptions::new();
                    options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                    event_target::<web_sys::Element>(&event)
                        .scroll_into_view_with_scroll_into_view_options(&options);
                }
                aria-describedby=description_ids
                aria-invalid=invalid
            ></textarea>
        }
        .into_any(),
        None => view! {
            <input
                id=id
                type=kind
                autocomplete=autocomplete
                prop:value=move || value.get()
                on:input=move |event| value.set(event_target_value(&event))
                aria-describedby=description_ids
                aria-invalid=invalid
            />
        }
        .into_any(),
    };
    view! {
        <div class="field">
            <label for=id>{label}</label>
            {input}
            <p id=described_by>{description}</p>
            {move || {
                error
                    .get()
                    .map(|text| {
                        view! {
                            <p id=error_in_error.clone() class="form-error">
                                {text}
                            </p>
                        }
                    })
            }}
        </div>
    }
}
