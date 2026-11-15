//! A snapshot's page's DOM glue (technical design §4.4): the route
//! `/snapshots/{alias}/{snapshot}`, the snapshot's folders and files in a
//! tree, each folder read as it is opened. It decides nothing: the address
//! of a page is `tree::folder_url`, and the tree's keys, rows and answers are
//! `tree.rs`'s, host-tested. What stays here is asking the daemon and drawing
//! the page; the journeys of `features/ui/snapshot_browser.feature` run it in
//! a browser, the `ws.rs` precedent.

use std::rc::Rc;
use std::sync::Arc;

use api::{FolderDto, MsgArg};
use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::components::{FolderLoader, FolderPage, Tree};
use crate::repositories::Sentence;
use crate::routes::{PageHeading, refused_sentence, short_id, text_with};
use crate::text;
use crate::tree::folder_url;
use crate::ws::{self, Got};

/// The page of one snapshot of one repository, both named by its address.
/// Download and Restore of what the tree selects are later slices' (#563,
/// #564).
#[component]
pub(crate) fn Snapshot(localizer: Arc<Localizer>) -> impl IntoView {
    let params = use_params_map();
    // Another snapshot's address draws the page anew.
    move || {
        let (alias, snapshot) = params.with(|params| {
            (
                params.get("alias").unwrap_or_default(),
                params.get("snapshot").unwrap_or_default(),
            )
        });
        snapshot_view(Arc::clone(&localizer), alias, snapshot)
    }
}

fn snapshot_view(localizer: Arc<Localizer>, alias: String, snapshot: String) -> impl IntoView {
    let heading = text_with(
        &localizer,
        msg::UI_SNAPSHOT_HEADING,
        &[
            ("snapshot", MsgArg::Text(short_id(&snapshot))),
            ("alias", MsgArg::Text(alias.clone())),
        ],
    );
    let base = ws::origin();
    let refusals = Arc::clone(&localizer);
    let loader: FolderLoader = Rc::new(move |path: String, from: u64| -> FolderPage {
        let url = folder_url(&base, &alias, &snapshot, &path, from);
        let localizer = Arc::clone(&refusals);
        Box::pin(async move {
            match ws::get::<FolderDto>(&url).await {
                Some(Got::Read(folder)) => Ok(folder),
                Some(Got::Refused(code, args, detail)) => {
                    Err(refused_sentence(&localizer, code, &args, detail.as_deref()))
                }
                None => Err(Sentence::new(
                    &text(&localizer, msg::UI_UNREACHABLE_BODY),
                    None,
                )),
            }
        })
    });
    view! {
        <PageHeading>{heading}</PageHeading>
        <p>{text(&localizer, msg::UI_SNAPSHOT_BODY)}</p>
        <Tree
            localizer=Arc::clone(&localizer)
            label=text(&localizer, msg::UI_SNAPSHOT_TREE)
            id="snapshot-tree".to_owned()
            loader=loader
            empty=text(&localizer, msg::UI_SNAPSHOT_EMPTY)
        />
        <p>
            <A href="/snapshots">{text(&localizer, msg::UI_SNAPSHOT_BACK)}</A>
        </p>
    }
}
