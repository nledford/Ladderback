//! The steps (components §4.5): a process's steps in order, each with its
//! state, and where the process has reached. [`rules`] decides each step's
//! state, its icon, its words and its connector, and what a change of step
//! is said as, host-tested; this module is the glue that draws them and says
//! the change through the announcer.

use std::sync::Arc;

use l10n::Localizer;
use leptos::prelude::*;

use super::Icon;
use crate::announcer::Politeness;
use crate::live::say;
use crate::text;
use crate::variants::Size;
use rules::StepState;
pub use rules::{Reached, StepsName};

// What a list of steps shows and says, decided on the host; `Steps` is its
// glue.
pub(crate) mod rules;

/// The steps of a process (§4.5): an ordered list (`<ol>`) named by its
/// caller's words (`name`), one item per step in `steps`, in order, each
/// with its name and its state in words under it, after an icon whose shape
/// is the state's own, so the state never rests on its color (A2).
/// `reached` is where the process stands: the steps before it are done and
/// those after it to do, and the step it stands at, under way or failed,
/// carries `aria-current="step"`.
///
/// It shows progress and doesn't navigate: nothing in it takes focus, and
/// it handles no keys. It is no live region either: when `reached` changes,
/// the announcer says the new step politely ("Step 3 of 5: Commit"), or the
/// failure, or that every step is done, as something that happened by
/// itself, which waits while live updates are paused; nothing is said as it
/// is first drawn, and focus never moves. So `reached` is a signal of a
/// list that stays drawn: a list drawn anew for each step starts with no
/// step before it and says nothing.
///
/// Its steps stand in a row while its own container has 42rem, a container
/// query in the reader's text size, and in a column below that, so a small
/// screen with larger text always shows a column (1.4.10, 1.4.4); a step's
/// name wraps and is never cut. Logical utilities only, so a page that runs
/// right to left mirrors the row. A connector joins each step to the next:
/// Ladder Blue as far as the process has gone, gray after. No tone, size
/// or placement of the shared vocabulary: each state picks its own color,
/// and the words carry the state.
///
/// What the caller owes WCAG 2.2 AA, since the list does none of it:
///
/// - **A name for the list** that says which process it shows.
/// - **Step names that say what each step does,** in the page's language.
/// - **The outcome said where it happened:** a failed step is shown and
///   said here, and the page says why beside the list, in an alert.
#[component]
pub fn Steps(
    localizer: Arc<Localizer>,
    name: StepsName,
    steps: Vec<String>,
    #[prop(into)] reached: Signal<Reached>,
) -> impl IntoView {
    let (label, labelled_by) = name.attributes();
    let count = steps.len();
    {
        let localizer = Arc::clone(&localizer);
        let names = steps.clone();
        // The previous run's answer is where the process stood before, none
        // on the first run, which says nothing.
        Effect::new(move |before: Option<Reached>| {
            let now = reached.get();
            if let Some(words) = rules::change(before, now, &names) {
                say(
                    Politeness::Polite,
                    &localizer.message(words.id, &words.args).into_text(),
                );
            }
            now
        });
    }
    let items = steps
        .into_iter()
        .enumerate()
        .map(|(place, name)| {
            let localizer = Arc::clone(&localizer);
            let state = Memo::new(move |_| StepState::of(place, count, reached.get()));
            // The connector into this step is the one leaving the step
            // before it, drawn in that step's color.
            let before = Memo::new(move |_| {
                place
                    .checked_sub(1)
                    .map(|previous| StepState::of(previous, count, reached.get()))
            });
            let (first, last) = (place == 0, place + 1 == count);
            view! {
                <li
                    aria-current=move || state.get().aria_current()
                    class="flex gap-x-3 [min-inline-size:0] @2xl:flex-1 @2xl:flex-col @2xl:items-center @2xl:gap-x-0 @2xl:text-center"
                >
                    // The icon between its two connectors: in a column, the
                    // one leaving it runs down beside the step's words to
                    // the next icon; in a row, both run across, meeting the
                    // steps on either side.
                    <span class="steps-track flex flex-none flex-col items-center @2xl:flex-row @2xl:self-stretch">
                        <span class=move || {
                            let color = before
                                .get()
                                .map_or("border-muted", StepState::connector_class);
                            let shown = if first { "invisible" } else { "" };
                            format!(
                                "steps-connector hidden flex-1 border-t-2 {color} {shown} @2xl:block",
                            )
                        } />
                        {move || {
                            let state = state.get();
                            view! {
                                <Icon glyph=state.glyph() tone=state.tone() size=Size::Medium />
                            }
                        }}
                        <span class=move || {
                            let color = state.get().connector_class();
                            let shown = if last { "invisible" } else { "" };
                            format!(
                                "steps-connector flex-1 border-s-2 {color} {shown} @2xl:border-s-0 @2xl:border-t-2",
                            )
                        } />
                    </span>
                    // The name, then the state in words, each wrapping
                    // inside the step's room, never cut (`min-inline-size`
                    // lets them shrink below their longest word, which no
                    // logical utility names). The space between them keeps
                    // the two apart where the item is read as one line.
                    <span class="flex flex-col pb-4 [min-inline-size:0] wrap-anywhere @2xl:pb-0 @2xl:pt-1 @2xl:px-2">
                        <span class=move || {
                            if state.get().aria_current().is_some() {
                                "steps-name font-semibold wrap-anywhere"
                            } else {
                                "steps-name wrap-anywhere"
                            }
                        }>{name}</span>
                        {" "}
                        <span class="steps-state text-sm text-muted">
                            {move || text(&localizer, state.get().word())}
                        </span>
                    </span>
                </li>
            }
        })
        .collect_view();
    view! {
        // The list's own container, which its query measures.
        <div class="@container">
            // `role="list"`: Safari drops a list's role once its markers
            // are gone.
            <ol
                role="list"
                class="m-0 flex list-none flex-col p-0 @2xl:flex-row"
                aria-label=label
                aria-labelledby=labelled_by
            >
                {items}
            </ol>
        </div>
    }
}
