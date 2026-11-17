use l10n::Direction;
use leptos::prelude::*;

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
/// `<fieldset>` and described by an optional hint under it, wired to the
/// group with `aria-describedby` so a screen reader reads it on entering the
/// group. Each choice is a native `<input type="radio">` sharing `name`, with
/// its visible label beside it as its click target, laid out as a
/// [`Checkbox`](super::Checkbox) is. The browser makes the group one Tab stop, on the choice
/// made, and the arrow keys move and choose within it, so nothing here
/// handles keys. `options` pairs each choice's value, which also makes its
/// id (`{name}-{value}`), with its label; `label_languages` pairs the value
/// of a choice whose label is in another language with that language, which
/// its `<label>` declares; `chosen` holds the value chosen,
/// so the caller reads it and saves it when it likes: arrowing through the
/// choices sets it at each step. One size and no color of its own (1.4.1).
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
    let _ = error;
    let described_by = description.as_ref().map(|_| format!("{name}-description"));
    let description = description.map(|text| {
        view! { <p id=described_by.clone()>{text}</p> }
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
                    <label for=id lang=lang dir=dir>
                        {label}
                    </label>
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
