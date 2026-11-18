use l10n::Direction;
use leptos::prelude::*;

use super::checkbox::rules::{self, Shape};
use super::text_field::{
    Access,
    rules::{described_by, description_id, error_id, invalid},
};

/// The language a choice's label is written in when it isn't the page's,
/// such as a language named in itself: its `lang`, which a screen reader
/// reads the label in (WCAG 3.1.2), and its `dir`, the way it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelLanguage {
    /// The language's tag, as `lang` takes it: `ja`, `ar`.
    pub lang: String,
    /// The way it runs, which its `dir` declares.
    pub dir: Direction,
}

/// A radio group: one choice of a few, named by the `<legend>` of its
/// `<fieldset>` and described by an optional hint under it and its error
/// while it has one, under the hint and above the choices, both wired to the
/// group with `aria-describedby` so a screen reader reads them on entering
/// the group; the error marks the group invalid (`aria-invalid` on its
/// `role="radiogroup"`, which ARIA 1.2 allows there and not on a radio), each
/// choice edged thicker in the Error tone beside the words. Each choice is a native
/// `<input type="radio">` sharing `name`, with its visible label beside it as
/// its click target, drawn as a [`Checkbox`](super::Checkbox) is but round,
/// filled in Ladder Blue under a dot in the page's color once chosen. The
/// browser makes the group one Tab stop, on the choice made (the first while
/// none is), and the arrow keys move and choose within it, so nothing here
/// handles keys. `options` pairs each choice's value, which also makes its
/// id (`{name}-{value}`), with its label; `label_languages` pairs the value
/// of a choice whose label is in another language with that language, which
/// its `<label>` declares; `chosen` holds the value chosen, none while it
/// matches no choice's, so the caller reads it and saves it when it likes:
/// arrowing through the choices sets it at each step. Like a text field's,
/// its error is said by its page, only once the form is checked.
#[component]
pub fn RadioGroup(
    name: &'static str,
    legend: String,
    #[prop(optional)] description: Option<String>,
    options: Vec<(&'static str, String)>,
    #[prop(optional)] label_languages: Vec<(&'static str, LabelLanguage)>,
    #[prop(optional, into)] error: Signal<Option<String>>,
    chosen: RwSignal<&'static str>,
) -> impl IntoView {
    let has_description = description.is_some();
    let has_error = move || error.with(Option::is_some);
    let description = description.map(|text| {
        view! { <p id=description_id(name)>{text}</p> }
    });
    let choices = options
        .into_iter()
        .map(|(value, label)| {
            let id = format!("{name}-{value}");
            let language = label_languages
                .iter()
                .find(|(of, _)| *of == value)
                .map(|(_, language)| language.clone());
            let (lang, dir) = language.map_or((None, None), |language| {
                (Some(language.lang), Some(language.dir.as_str()))
            });
            view! {
                <div class=rules::LINE>
                    <span class=rules::CELL>
                        <input
                            id=id.clone()
                            type="radio"
                            name=name
                            value=value
                            class=rules::box_class(Shape::Round)
                            prop:checked=move || chosen.get() == value
                            on:change=move |event| {
                                if event_target_checked(&event) {
                                    chosen.set(value);
                                }
                            }
                        />
                        <span class=rules::mark_class(Shape::Round) aria-hidden="true"></span>
                    </span>
                    <label for=id lang=lang dir=dir class=rules::LABEL>
                        {label}
                    </label>
                </div>
            }
        })
        .collect_view();
    view! {
        <fieldset
            class=rules::GROUP
            role="radiogroup"
            aria-invalid=move || invalid(has_error())
            aria-describedby=move || {
                described_by(name, has_description, Access::Editable, has_error())
            }
        >
            <legend>{legend}</legend>
            {description}
            {move || {
                error
                    .get()
                    .map(|text| {
                        view! {
                            <p id=error_id(name) class="form-error">
                                {text}
                            </p>
                        }
                    })
            }}
            {choices}
        </fieldset>
    }
}
