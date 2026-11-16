use crate::announcer::Politeness;
use lepticons_data::LucideGlyph;
use leptos::prelude::*;

use super::Button;
use super::button::{Appearance, Content};
use crate::live::reply;
use crate::variants::Size;

/// A data table: a caption, `scope`d headers, and a column sortable through a
/// button in its header, with each sort announced politely.
#[component]
pub fn SortableTable(
    caption: String,
    column: String,
    sort_label: String,
    sorted_message: String,
    rows: Vec<String>,
) -> impl IntoView {
    // Starts descending, so the first sort click is the ascending one the
    // gallery's journey checks.
    let sorted = RwSignal::new(false);
    let message = StoredValue::new(sorted_message);
    let shown = Memo::new(move |_| {
        let mut rows = rows.clone();
        if sorted.get() {
            rows.sort();
        }
        rows
    });
    view! {
        <table>
            <caption class="text-muted">{caption}</caption>
            <thead>
                <tr>
                    <th
                        scope="col"
                        aria-sort=move || { if sorted.get() { "ascending" } else { "descending" } }
                    >
                        <span class="sort-header">
                            {column.clone()}
                            <Button
                                content=Content::IconAndText {
                                    glyph: Signal::derive(move || {
                                        if sorted.get() {
                                            LucideGlyph::ArrowUp
                                        } else {
                                            LucideGlyph::ArrowDown
                                        }
                                    }),
                                    label: sort_label.into(),
                                }
                                appearance=Appearance::Ghost
                                size=Size::Small
                                on_press=Callback::new(move |()| {
                                    sorted.set(!sorted.get_untracked());
                                    reply(Politeness::Polite, &message.with_value(String::clone));
                                })
                            />
                        </span>
                    </th>
                </tr>
            </thead>
            <tbody>
                {move || {
                    shown
                        .get()
                        .into_iter()
                        .map(|row| {
                            view! {
                                <tr>
                                    <td>{row}</td>
                                </tr>
                            }
                        })
                        .collect::<Vec<_>>()
                }}
            </tbody>
        </table>
    }
}
