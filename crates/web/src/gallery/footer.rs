//! The gallery's footer demo (technical design §4.5, "Tests").

use std::sync::Arc;

use api::BuildDto;
use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::Footer;
use crate::page_width::PageWidth;
use crate::text;

/// The footer's demo (`#footer-demo`). Inside a section a footer is no
/// landmark, so the page's own footer, after `<main>`, stays its one
/// `contentinfo`.
#[component]
pub(super) fn FooterDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let heading = text(&localizer, msg::UI_GALLERY_FOOTER_HEADING);
    // A release's build, then the longest a build between releases shows,
    // which wraps in a narrow room, then none, which draws nothing.
    let build = |commit: Option<&str>, dirty: bool| {
        BuildDto {
            version: "0.4.0".to_owned(),
            commit: commit.map(str::to_owned),
            dirty,
            release: commit.is_none(),
        }
        .display()
    };
    let release = build(None, false);
    let between = build(Some("4444444444444444444444444444444444444444"), true);
    let none: Option<String> = None;
    view! {
        <section id="footer-demo">
            <DemoHeading text=heading />
            <Footer
                localizer=Arc::clone(&localizer)
                version=Some(release)
                width=PageWidth::Narrow
            />
            <Footer
                localizer=Arc::clone(&localizer)
                version=Some(between)
                width=PageWidth::Narrow
            />
            <Footer localizer=localizer version=none width=PageWidth::Narrow />
        </section>
    }
}
