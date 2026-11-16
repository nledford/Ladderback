use leptos::prelude::*;

/// A radio group: one choice of a few, named by the `<legend>` of its
/// `<fieldset>` and described by an optional hint under it, wired to the
/// group with `aria-describedby` so a screen reader reads it on entering the
/// group. Each choice is a native `<input type="radio">` sharing `name`, with
/// its visible label beside it as its click target, laid out as a
/// [`Checkbox`](super::Checkbox) is. The browser makes the group one Tab stop, on the choice
/// made, and the arrow keys move and choose within it, so nothing here
/// handles keys. `options` pairs each choice's value, which also makes its
/// id (`{name}-{value}`), with its label; `chosen` holds the value chosen,
/// so the caller reads it and saves it when it likes: arrowing through the
/// choices sets it at each step. One size and no color of its own (1.4.1).
#[component]
pub fn RadioGroup(
    name: &'static str,
    legend: String,
    #[prop(optional)] description: Option<String>,
    options: Vec<(&'static str, String)>,
    chosen: RwSignal<&'static str>,
) -> impl IntoView {
    let described_by = description.as_ref().map(|_| format!("{name}-description"));
    let description = description.map(|text| {
        view! { <p id=described_by.clone()>{text}</p> }
    });
    let choices = options
        .into_iter()
        .map(|(value, label)| {
            let id = format!("{name}-{value}");
            view! {
                <div class="field checkbox">
                    <input
                        id=id.clone()
                        type="radio"
                        name=name
                        value=value
                        prop:checked=move || chosen.get() == value
                        on:change=move |event| {
                            if event_target_checked(&event) {
                                chosen.set(value);
                            }
                        }
                    />
                    <label for=id>{label}</label>
                </div>
            }
        })
        .collect_view();
    view! {
        <fieldset class="choices" aria-describedby=described_by.clone()>
            <legend>{legend}</legend>
            {description}
            {choices}
        </fieldset>
    }
}
