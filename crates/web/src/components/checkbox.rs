use leptos::prelude::*;

pub(crate) mod rules;

/// A checkbox: a native `<input type="checkbox">` with its visible label
/// beside it, which is also its click target, and an optional description
/// wired to it with `aria-describedby`, read after its name, as a
/// [`TextField`](super::TextField)'s is. `checked` holds the choice, so the caller reads and
/// resets it. Space turns it on and off, and the browser tells a screen
/// reader its state, so nothing here announces it. One size and no color of
/// its own: its meaning is in its words (1.4.1). Off and on only: no
/// disabled, indeterminate, error or group state until a page needs one.
#[component]
pub fn Checkbox(
    id: &'static str,
    label: String,
    #[prop(optional)] description: Option<String>,
    #[prop(optional, into)] error: Signal<Option<String>>,
    checked: RwSignal<bool>,
) -> impl IntoView {
    let _ = error;
    let described_by = description.as_ref().map(|_| format!("{id}-description"));
    let description = description.map(|text| {
        view! { <p id=described_by.clone()>{text}</p> }
    });
    view! {
        <div class="field checkbox">
            <input
                id=id
                type="checkbox"
                prop:checked=move || checked.get()
                on:change=move |event| checked.set(event_target_checked(&event))
                aria-describedby=described_by.clone()
            />
            <label for=id>{label}</label>
            {description}
        </div>
    }
}
