//! The routes of the app (technical design §4.5, "Route changes"): the pages a
//! path shows. Each page has its own `<h1>`, the heading focus moves to when a
//! reader navigates to it, so it carries `tabindex="-1"`.

use std::sync::Arc;

use l10n::{Localizer, msg};
use leptos::prelude::*;
use leptos_router::components::A;

use crate::text;

/// The start page, the route `/`.
#[component]
pub(crate) fn Start(localizer: Arc<Localizer>) -> impl IntoView {
    view! {
        <h1 tabindex="-1">{text(&localizer, msg::UI_START_HEADING)}</h1>
        <p>{text(&localizer, msg::UI_START_BODY)}</p>
    }
}

/// What any path nothing serves shows.
#[component]
pub(crate) fn NotFound(localizer: Arc<Localizer>) -> impl IntoView {
    view! {
        <h1 tabindex="-1">{text(&localizer, msg::UI_NOT_FOUND_HEADING)}</h1>
        <p>{text(&localizer, msg::UI_NOT_FOUND_BODY)}</p>
        <p><A href="/">{text(&localizer, msg::UI_NOT_FOUND_LINK)}</A></p>
    }
}

/// The title of the page at `path`, for the browser's tab and history: the
/// page's own, in the page's language (technical design §4.5, "Route
/// changes"). A path with no page is the not-found page, and so is
/// `/_gallery` in a build without the gallery.
// Commit 1 of the red-first history: a `todo!()` stub that nothing calls yet.
// The allowances go with the implementation.
#[allow(dead_code, clippy::todo)]
pub(crate) fn title(localizer: &Localizer, path: &str) -> String {
    let _ = (
        localizer,
        path,
        msg::UI_NOT_FOUND_TITLE,
        msg::UI_GALLERY_TITLE,
    );
    todo!("title")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title_of(path: &str) -> String {
        title(&Localizer::new(&[]), path)
    }

    #[test]
    fn the_start_page_is_titled_with_the_product_name() {
        assert_eq!(title_of("/"), "Ladderback");
    }

    #[test]
    fn a_path_with_no_page_is_titled_page_not_found() {
        assert_eq!(title_of("/no/such/page"), "Page not found – Ladderback");
    }

    #[test]
    fn the_gallery_has_its_title_only_in_a_build_that_has_it() {
        let expected = if cfg!(feature = "gallery") {
            "Component gallery – Ladderback"
        } else {
            "Page not found – Ladderback"
        };
        assert_eq!(title_of("/_gallery"), expected);
    }

    #[test]
    fn a_trailing_slash_names_the_same_page() {
        assert_eq!(title_of("/_gallery/"), title_of("/_gallery"));
        assert_eq!(title_of("/no/such/page/"), "Page not found – Ladderback");
    }

    #[test]
    fn a_path_that_only_starts_like_a_page_is_not_that_page() {
        assert_eq!(title_of("/_gallery-2"), "Page not found – Ladderback");
        assert_eq!(title_of("//"), "Page not found – Ladderback");
    }
}
