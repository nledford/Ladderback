//! The page the app runs in (technical design §8.4): which language it is
//! shown in, and what it tells the browser about it. This is DOM glue and
//! nothing else. The choice of language is `l10n`'s, and is tested there, and
//! which choice of Settings' is kept is `language_choice.rs`'s; the scenarios
//! of `features/ui/pseudolocales.feature`, `features/ui/language.feature` and
//! `features/ui/accessibility.feature` run this in a browser.

use std::cell::Cell;

use l10n::{BrowserLanguages, Localizer, msg};
use web_sys::{HtmlElement, Storage, UrlSearchParams};

use crate::routes;
use crate::session::Title;

/// Where the browser keeps the language chosen in Settings, in its local
/// storage: this browser's choice, not Ladderback's (decision 101).
const CHOSEN_KEY: &str = "ladderback-language";

/// The localizer of this page: the language its address asks for (`?locale=`,
/// a development override), then the language chosen in this browser, then
/// the languages the browser lists, over the catalogs this build carries.
/// Isolation marks are on, because a page mixes text that runs in both
/// directions.
pub(crate) fn localizer() -> Localizer {
    Localizer::new(&browser_languages().preferences()).with_isolation(true)
}

/// What the page's address and the browser ask for. Both are outside input,
/// and are only read here: `BrowserLanguages` reads them within bounds, and
/// what is not a language is ignored.
fn browser_languages() -> BrowserLanguages {
    let Some(window) = web_sys::window() else {
        return BrowserLanguages::default();
    };
    let requested = window
        .location()
        .search()
        .ok()
        .and_then(|query| UrlSearchParams::new_with_str(&query).ok())
        .and_then(|params| params.get("locale"));
    let languages = window
        .navigator()
        .languages()
        .iter()
        .filter_map(|language| language.as_string())
        .collect();
    BrowserLanguages {
        requested,
        chosen: stored_language(),
        languages,
    }
}

/// The browser's local storage, when it lets this page have it: its settings,
/// or a private window, may refuse it.
fn storage() -> Option<Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// The language this browser keeps as chosen, if any. Outside input: whoever
/// reads it holds it to the languages offered.
pub(crate) fn stored_language() -> Option<String> {
    storage()?.get_item(CHOSEN_KEY).ok().flatten()
}

/// Keeps `tag` as the language chosen in this browser, or forgets the one
/// kept for `None`, then loads the page again at its section `section`, so
/// the page is shown in the language from its start and focus comes back to
/// that section ([`focus_when_named`]). `false` when the browser refused to
/// keep it, and the page stays as it was.
pub(crate) fn choose_language(tag: Option<&str>, section: &str) -> bool {
    let Some(storage) = storage() else {
        return false;
    };
    let kept = match tag {
        Some(tag) => storage.set_item(CHOSEN_KEY, tag),
        None => storage.remove_item(CHOSEN_KEY),
    };
    if kept.is_err() {
        return false;
    }
    if let Some(window) = web_sys::window() {
        // The fragment replaces the address's own, so Back doesn't stop at
        // the page before the save. Failing, the page loads at its top.
        if let Ok(history) = window.history() {
            let _ = history.replace_state_with_url(
                &leptos::wasm_bindgen::JsValue::NULL,
                "",
                Some(&format!("#{section}")),
            );
        }
        // A failed reload leaves the page as it was, its language kept for
        // the next load.
        let _ = window.location().reload();
    }
    true
}

/// Tells the browser what the page is written in: the `lang` and `dir` of
/// `<html>`, which a screen reader and the layout follow, and the title of the
/// page the address names.
pub(crate) fn declare(localizer: &Localizer) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    if let Some(root) = document.document_element() {
        // Setting an attribute of a fixed, valid name does not fail.
        let _ = root.set_attribute("lang", &localizer.language().to_string());
        let _ = root.set_attribute("dir", localizer.direction().as_str());
    }
    let path = window.location().pathname().unwrap_or_default();
    document.set_title(&routes::title(localizer, &path));
}

/// The reader has navigated to the page at `path`: the title follows
/// (technical design §4.5, "Route changes"). Focus is the heading's own, see
/// [`focus_on_arrival`].
pub(crate) fn retitle(localizer: &Localizer, path: &str) {
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        document.set_title(&routes::title(localizer, path));
    }
}

/// The page's standing with sign-in changed (security §10.1): the sign-in
/// page has a title of its own, and once signed in the page takes the title
/// its address names.
pub(crate) fn retitle_as(localizer: &Localizer, title: Title) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    match title {
        Title::SignIn => {
            document.set_title(&localizer.message(msg::UI_SIGN_IN_TITLE, &[]).into_text())
        }
        Title::Address => {
            let path = window.location().pathname().unwrap_or_default();
            document.set_title(&routes::title(localizer, &path));
        }
    }
}

thread_local! {
    /// Whether the page's first heading is still to be shown: the page load,
    /// not a navigation.
    static LOADING: Cell<bool> = const { Cell::new(true) };

    /// Whether the section the page's address names by its fragment is still
    /// to be shown: once a page load.
    static NAMED_PENDING: Cell<bool> = const { Cell::new(true) };
}

/// A page's `<h1>` has been shown. After a navigation, focus moves to it,
/// which a screen reader then reads, so the announcer stays silent (technical
/// design §4.5, "Route changes"). On the page load, focus stays where the
/// browser put it, at the top of the page, unless the address names a
/// section ([`focus_when_named`]).
pub(crate) fn focus_on_arrival(heading: &HtmlElement) {
    if LOADING.replace(false) {
        return;
    }
    // A failed focus leaves focus where it was.
    let _ = heading.focus();
}

/// The heading of the section `section` has been shown. When the page's
/// address names that section by its fragment (`/settings#language`), focus
/// moves to the heading, which scrolls it into view and which a screen reader
/// then reads, once a page load: a page shown again after a save starts where
/// the reader was, not at its top (technical design §4.5, "Route changes").
pub(crate) fn focus_when_named(section: &str, heading: &HtmlElement) {
    let named = web_sys::window()
        .and_then(|window| window.location().hash().ok())
        .is_some_and(|hash| hash.strip_prefix('#') == Some(section));
    if named && NAMED_PENDING.replace(false) {
        // A failed focus leaves focus where it was.
        let _ = heading.focus();
    }
}
