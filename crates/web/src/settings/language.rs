//! Settings' "Language" section (technical design §8.4, decision 101): the
//! language the web UI is shown in, kept in this browser. The choices, the
//! one shown chosen and the tag a save keeps are `language_choice.rs`'s,
//! host-tested; keeping it and loading the page again are `page.rs`'s. This
//! is the DOM glue, which the `@ui` scenarios of `features/ui/language.feature`
//! run in Chromium.

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;

use crate::announcer::Politeness;
use crate::components::ACTIONS;
use crate::components::alert::rules::AlertTone;
use crate::components::button::{Appearance, Content, Kind};
use crate::components::{Alert, Button, LabelLanguage, RadioGroup};
use crate::language_choice::{GROUP, HEADING_ID, chosen, options, to_keep};
use crate::live::reply;
use crate::page;
use crate::settings_tabs::Section;
use crate::text;
use crate::variants::Tone;

/// The section: a radio group of the browser's languages and each language
/// this build offers, named in itself and marked as written in it, and a
/// button that keeps the choice and shows the page again in it, with focus
/// back on the section's heading. A browser that refuses to keep it is said
/// so in an error alert, and assertively, as the press's answer, since the
/// alert is no live region; the page stays as it was.
#[component]
pub(super) fn LanguageSection(localizer: Arc<Localizer>) -> impl IntoView {
    let languages = l10n::languages();
    let chosen = RwSignal::new(chosen(page::stored_language().as_deref(), &languages));
    let refused = RwSignal::new(false);
    let label_languages = languages
        .iter()
        .map(|language| {
            (
                language.tag,
                LabelLanguage {
                    lang: language.language.to_string(),
                    dir: language.direction,
                },
            )
        })
        .collect();
    let choices = options(text(&localizer, msg::UI_LANGUAGE_BROWSER), &languages);
    let heading = NodeRef::<leptos::html::H2>::new();
    Effect::new(move |_| {
        if let Some(heading) = heading.get() {
            page::focus_when_named(Section::Language.fragment(), &heading);
        }
    });
    let not_kept = text(&localizer, msg::UI_LANGUAGE_NOT_KEPT);
    let said = not_kept.clone();
    let alert_localizer = Arc::clone(&localizer);
    view! {
        <section aria-labelledby=HEADING_ID>
            <h2 id=HEADING_ID node_ref=heading tabindex="-1">
                {text(&localizer, msg::UI_LANGUAGE_HEADING)}
            </h2>
            <form
                novalidate
                on:submit=move |event| {
                    event.prevent_default();
                    let tag = to_keep(chosen.get_untracked(), &languages);
                    let kept = page::choose_language(tag, Section::Language.fragment());
                    if !kept {
                        reply(Politeness::Assertive, &said);
                    }
                    refused.set(!kept);
                }
            >
                <RadioGroup
                    name=GROUP
                    legend=text(&localizer, msg::UI_LANGUAGE_LEGEND)
                    description=text(&localizer, msg::UI_LANGUAGE_HINT)
                    options=choices
                    label_languages=label_languages
                    chosen=chosen
                />
                <p class=ACTIONS>
                    <Button
                        content=Content::Text(text(&localizer, msg::UI_LANGUAGE_SAVE).into())
                        appearance=Appearance::Filled
                        tone=Tone::Primary
                        kind=Kind::Submit
                    />
                </p>
            </form>
            {move || {
                refused
                    .get()
                    .then(|| {
                        let words = not_kept.clone();
                        view! {
                            <Alert localizer=Arc::clone(&alert_localizer) tone=AlertTone::Error>
                                {words}
                            </Alert>
                        }
                    })
            }}
        </section>
    }
}
