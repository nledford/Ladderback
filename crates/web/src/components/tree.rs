use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use api::{EntryDto, FolderDto, MsgArg};
use l10n::{Direction, Localizer, msg};
use leptos::prelude::*;
use leptos::tachys::html::attribute::custom::custom_attribute;
use leptos::task::{spawn_local, tick};

use super::{focus_element, sentence_view};
use crate::repositories::Sentence;
use crate::routes::{instant, text_with};
use crate::text;
use crate::tree;

/// What reads a page of a folder of a snapshot's tree: given the folder's
/// path and the position the page starts at, the page, or the refusal as the
/// page says it.
pub(crate) type FolderLoader = Rc<dyn Fn(String, u64) -> FolderPage>;

/// A page of a folder on its way, or the refusal as the page says it.
pub(crate) type FolderPage = Pin<Box<dyn Future<Output = Result<FolderDto, Sentence>>>>;

/// The `id` of a row of the tree whose rows' ids start with `prefix`.
fn row_id(prefix: &str, row: tree::Row) -> String {
    match row {
        tree::Row::Entry(node) => format!("{prefix}-{node}"),
        tree::Row::More(folder) => format!("{prefix}-more-{folder}"),
    }
}

/// A snapshot's tree (§4.5, "Tree"): a WAI-ARIA tree, one stop for Tab with
/// a roving `tabindex`, each folder read through `loader` as it is first
/// opened, a page at a time. What a key does, which rows show and what an
/// answer changes are `tree.rs`'s, host-tested; this draws the rows, sends
/// what they ask for and moves focus where the tree says. Each row is named
/// by its entry's name and described by what it is: its kind, size and
/// modification time. A refused page is said in an alert under the tree;
/// `empty` is said in place of a tree when the top folder holds nothing.
/// `selected` follows what the tree selects ([`tree::Tree::selection`]), for
/// a pane beside it that acts on that entry.
#[component]
pub(crate) fn Tree(
    localizer: Arc<Localizer>,
    label: String,
    /// The start of each row's `id`, unique on the page.
    id: String,
    loader: FolderLoader,
    empty: String,
    #[prop(optional)] selected: Option<RwSignal<Option<tree::Selection>>>,
) -> impl IntoView {
    let rtl = localizer.direction() == Direction::Rtl;
    let loading = text(&localizer, msg::UI_LOADING);
    let localizer = StoredValue::new(localizer);
    let zone = StoredValue::new(l10n::viewer_zone());
    let prefix = StoredValue::new(id);
    let loader = StoredValue::new_local(loader);
    let mut initial = tree::Tree::new();
    let first = initial.start();
    let state = RwSignal::new(initial);
    if let Some(selected) = selected {
        let selection = Memo::new(move |_| state.with(tree::Tree::selection));
        Effect::new(move |_| selected.set(selection.get()));
    }
    // Puts focus on the row holding the tab stop once it is drawn.
    let focus_stop = move || {
        let Some(stop) = state.with_untracked(tree::Tree::tab_stop) else {
            return;
        };
        let id = row_id(&prefix.get_value(), stop);
        spawn_local(async move {
            tick().await;
            focus_element(&id);
        });
    };
    // Sends a request, and takes its answer if the tree is still there. An
    // answer that moves the tab stop from a row that went (the row that read
    // a page) moves focus with it, when focus was on that row.
    let fetch = move |request: tree::Request| {
        let future = loader.with_value(|load| load(request.path.clone(), request.from));
        spawn_local(async move {
            let answer = future.await;
            let Some(before) = state.try_with_untracked(tree::Tree::tab_stop) else {
                return;
            };
            state.update(|tree| tree.answered(&request, answer));
            let after = state.with_untracked(tree::Tree::tab_stop);
            if let (Some(before), Some(after)) = (before, after)
                && before != after
            {
                let (before, after) = (
                    row_id(&prefix.get_value(), before),
                    row_id(&prefix.get_value(), after),
                );
                tick().await;
                let focus_fell = web_sys::window()
                    .and_then(|window| window.document())
                    .and_then(|document| document.active_element())
                    .is_none_or(|active| active.id() == before || active.tag_name() == "BODY");
                if focus_fell {
                    focus_element(&after);
                }
            }
        });
    };
    if let Some(first) = first {
        fetch(first);
    }
    // Each change walks the tree once: every row reads what it says from
    // `views`, and redraws only when that changes.
    let views = Memo::new(move |_| state.with(tree::Tree::views));
    let has_rows = Memo::new(move |_| views.with(|views| !views.rows.is_empty()));
    let starting = Memo::new(move |_| state.with(tree::Tree::starting));
    let is_empty = Memo::new(move |_| state.with(tree::Tree::is_empty));
    let failure = Memo::new(move |_| state.with(|tree| tree.failure().cloned()));
    let describe = move |kind: tree::State, entry: &EntryDto| {
        let localizer = localizer.get_value();
        let mut args = vec![
            ("kind", MsgArg::Text(kind.kind().to_owned())),
            ("size", MsgArg::Bytes(entry.size.unwrap_or_default())),
        ];
        let id = match &entry.modified {
            Some(raw) => {
                let when = instant(&localizer, raw, zone.get_value().as_ref());
                args.push(("modified", MsgArg::Text(when)));
                msg::UI_TREE_ENTRY_MODIFIED
            }
            None => msg::UI_TREE_ENTRY,
        };
        text_with(&localizer, id, &args)
    };
    // A row's level and its entry never change; what it says besides is its
    // `RowView`.
    let row_view = move |row: tree::Row| {
        let id = row_id(&prefix.get_value(), row);
        let about_id = format!("{id}-about");
        let level = state.with_untracked(|tree| tree.place(row).0);
        let indent = format!("{}rem", 0.25 + 1.5 * f64::from(tree::indent_steps(level)));
        let entry = match row {
            tree::Row::Entry(node) => {
                state.with_untracked(|tree| tree.node(node).map(|node| node.entry.clone()))
            }
            tree::Row::More(_) => None,
        };
        let says = Memo::new(move |_| views.with(|views| views.get(row)).unwrap_or_default());
        let entry_name = entry
            .as_ref()
            .map(|entry| entry.display.clone())
            .unwrap_or_default();
        let name = move || match row {
            tree::Row::Entry(_) => entry_name.clone(),
            tree::Row::More(_) => says.with(|says| {
                text_with(
                    &localizer.get_value(),
                    msg::UI_TREE_MORE,
                    &[
                        ("shown", MsgArg::Count(says.shown)),
                        ("total", MsgArg::Count(says.set_size)),
                    ],
                )
            }),
        };
        let stop = move || says.with(|says| says.stop);
        let described_by = entry.is_some().then(|| about_id.clone());
        view! {
            <li
                // tachys has no typed `aria-level`, and the macro reads any
                // `aria-` name as one, so it is set as a custom attribute,
                // first: after an unbraced value it would parse as a struct.
                {..custom_attribute("aria-level", level.to_string())}
                role="treeitem"
                id=id
                class="tree-row"
                style:padding-inline-start=indent
                aria-posinset=move || says.with(|says| says.position)
                aria-setsize=move || says.with(|says| says.set_size)
                aria-expanded=move || says.with(|says| says.expanded.map(|open| open.to_string()))
                aria-busy=move || says.with(|says| says.busy).then_some("true")
                aria-selected=move || stop().to_string()
                tabindex=move || if stop() { "0" } else { "-1" }
                aria-describedby=described_by
                on:click=move |_| {
                    if let Some(request) = state.try_update(|tree| tree.activate(row)).flatten() {
                        fetch(request);
                    }
                    focus_stop();
                }
            >
                // A name keeps its own direction: one written right to left
                // reads so in any page, and the text beside it can't
                // reorder it.
                <bdi>{name}</bdi>
                {entry
                    .map(|entry| {
                        let kind = Memo::new(move |_| says.with(|says| says.state));
                        // The row is named by its content, which leaves out
                        // what is hidden: the description is heard once,
                        // through `aria-describedby`, and the name is the
                        // visible name (2.5.3, Label in Name).
                        view! {
                            " "
                            <span id=about_id class="text-muted" aria-hidden="true">
                                {move || {
                                    kind.get()
                                        .map(|kind| describe(kind, &entry))
                                        .unwrap_or_default()
                                }}
                            </span>
                        }
                    })}
            </li>
        }
    };
    view! {
        {move || starting.get().then(|| view! { <p>{loading.clone()}</p> })}
        {move || is_empty.get().then(|| view! { <p>{empty.clone()}</p> })}
        {move || {
            has_rows
                .get()
                .then(|| {
                    view! {
                        <ul
                            role="tree"
                            class="tree"
                            aria-label=label.clone()
                            on:keydown=move |event: web_sys::KeyboardEvent| {
                                if event.ctrl_key() || event.alt_key() || event.meta_key() {
                                    return;
                                }
                                let key = event.key();
                                let now = event.time_stamp();
                                let Some(pressed) = state
                                    .try_update(|tree| tree.key(&key, rtl, now)) else {
                                    return;
                                };
                                if pressed.handled {
                                    event.prevent_default();
                                    focus_stop();
                                }
                                if let Some(request) = pressed.request {
                                    fetch(request);
                                }
                            }
                        >
                            <For
                                each=move || views.with(|views| views.rows.clone())
                                key=|row| *row
                                children=move |row| row_view(row)
                            />
                        </ul>
                    }
                })
        }}
        {move || {
            failure.get().map(|sentence| view! { <p role="alert">{sentence_view(sentence)}</p> })
        }}
    }
}
