//! The fingerprint (components §4.5; §16, decision 121, "The fingerprint"):
//! a key's fingerprint as a person compares it with another screen. It is
//! shown in full, never cut short, in monospace, in groups of four that
//! wrap between them and never inside one. A screen reader is given a group
//! named by the caller, then one image per character named by that
//! character alone, so it is spoken a character at a time; the hyphens
//! between groups are hidden from it, and a copy is the plain string.
//! [`rules`] decides the groups, host-tested. Left to right in every
//! locale: a fingerprint's characters keep their order.

use leptos::prelude::*;

// What a fingerprint draws, decided on the host.
pub(crate) mod rules;

/// A key's fingerprint named `name`, as "Hub key fingerprint".
#[component]
pub fn Fingerprint(
    /// The fingerprint, as `7K3Q-M9XD-2RTV-8HFA-QW5N`.
    #[prop(into)]
    value: String,
    /// What the fingerprint is of, its group's accessible name.
    #[prop(into)]
    name: String,
) -> impl IntoView {
    let groups = rules::groups(&value);
    let last = groups.len().saturating_sub(1);
    let drawn = groups
        .into_iter()
        .enumerate()
        .map(|(place, group)| {
            let characters = group
                .chars()
                .map(|character| {
                    let said = character.to_string();
                    let label = said.clone();
                    view! {
                        <span role="img" aria-label=label>
                            {said}
                        </span>
                    }
                })
                .collect_view();
            view! {
                <span class="inline-block whitespace-nowrap">{characters}</span>
                {(place < last).then(|| view! { <span aria-hidden="true">"-"</span> })}
            }
        })
        .collect_view();
    view! {
        <span role="group" aria-label=name dir="ltr" class="font-mono break-normal">
            {drawn}
        </span>
    }
}
