//! The page the app runs in (technical design §8.4): which language it is
//! shown in, and what it tells the browser about it. This is DOM glue and
//! nothing else. The choice of language is `l10n`'s, and is tested there; the
//! scenarios of `features/ui/pseudolocales.feature` run this in a browser.

use l10n::{BrowserLanguages, Localizer, msg};
use web_sys::UrlSearchParams;

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
/// `<html>`, which a screen reader and the layout follow, and the start
/// page's title, the only page there is yet.
pub(crate) fn declare(localizer: &Localizer) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if let Some(root) = document.document_element() {
        // Setting an attribute of a fixed, valid name does not fail.
        let _ = root.set_attribute("lang", &localizer.language().to_string());
        let _ = root.set_attribute("dir", localizer.direction().as_str());
    }
    document.set_title(localizer.message(msg::UI_START_TITLE, &[]).text());
}
