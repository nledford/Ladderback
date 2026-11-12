//! The page the app runs in (technical design §8.4): which language it is
//! shown in, and what it tells the browser about it. This is DOM glue and
//! nothing else. The choice of language is `l10n`'s, and is tested there; the
//! scenarios of `features/ui/pseudolocales.feature` and
//! `features/ui/accessibility.feature` run this in a browser.

use std::cell::Cell;

use l10n::{BrowserLanguages, Localizer, msg};
use web_sys::{HtmlElement, UrlSearchParams};

use crate::routes;
use crate::session::Title;

/// The localizer of this page: the language its address asks for (`?locale=`,
/// a development override), then the languages the browser lists, over the
/// catalogs this build carries. Isolation marks are on, because a page mixes
/// text that runs in both directions.
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
        languages,
    }
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
}

/// A page's `<h1>` has been shown. After a navigation, focus moves to it,
/// which a screen reader then reads, so the announcer stays silent (technical
/// design §4.5, "Route changes"). On the page load, focus stays where the
/// browser put it, at the top of the page.
pub(crate) fn focus_on_arrival(heading: &HtmlElement) {
    if LOADING.replace(false) {
        return;
    }
    // A failed focus leaves focus where it was.
    let _ = heading.focus();
}
