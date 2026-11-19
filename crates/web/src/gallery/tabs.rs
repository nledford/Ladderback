//! The gallery's tabs demos (technical design §4.5, "Tests").

use std::sync::Arc;

use api::MsgArg;
use l10n::{Localizer, msg};
use leptos::prelude::*;

use super::DemoHeading;
use crate::components::tabs::{Activation, Holds, Orientation, TabSet, TabsName};
use crate::components::{Tab, Tabs, TextField};
use crate::routes::text_with;
use crate::text;

/// The tabs' demo (`#tabs-demo`).
#[component]
pub(super) fn TabsDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    // The tab the manual demo's set tells it is chosen.
    let manual_chosen = RwSignal::new(0_usize);
    view! {
        <section id="tabs-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABS_HEADING) />
            // Each tab the arrow keys arrive at is selected, the set named by
            // the heading above it. The middle panel holds a field, so it is
            // no Tab stop of its own, and what is typed there stays while
            // another panel shows.
            <h3 id="gallery-tabs-repository">{t(msg::UI_GALLERY_TABS_REPOSITORY)}</h3>
            <Tabs
                id="gallery-tabs-automatic"
                name=TabsName::LabelledBy("gallery-tabs-repository".to_owned())
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_OVERVIEW),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_OVERVIEW_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_NOTE),
                            Holds::Controls,
                            view! {
                                <TextField
                                    id="gallery-tabs-note-text"
                                    localizer=Arc::clone(&localizer)
                                    value=RwSignal::new(String::new())
                                    label=t(msg::UI_GALLERY_TABS_NOTE_FIELD)
                                    description=t(msg::UI_GALLERY_TABS_NOTE_HINT)
                                />
                            },
                        ),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_HISTORY),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_HISTORY_TEXT)}</p> },
                        ),
                    ],
                )
            />
            // Chosen by hand, as tabs whose panels read from the daemon
            // would be: the arrow keys only move focus. The paragraph above
            // says so and describes the list; the one below shows the tab
            // the set tells its caller is chosen.
            <p id="gallery-tabs-manual-hint">{t(msg::UI_GALLERY_TABS_MANUAL_HINT)}</p>
            <Tabs
                id="gallery-tabs-manual"
                name=TabsName::Label(t(msg::UI_GALLERY_TABS_AGENT))
                activation=Activation::Manual
                described_by="gallery-tabs-manual-hint"
                selected=manual_chosen
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_LOG),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_LOG_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_STATS),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_STATS_TEXT)}</p> },
                        ),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_PLANS),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_PLANS_TEXT)}</p> },
                        ),
                    ],
                )
            />
            <p>
                {
                    let localizer = Arc::clone(&localizer);
                    let labels = [
                        t(msg::UI_GALLERY_TABS_LOG),
                        t(msg::UI_GALLERY_TABS_STATS),
                        t(msg::UI_GALLERY_TABS_PLANS),
                    ];
                    move || {
                        let tab = labels.get(manual_chosen.get()).cloned().unwrap_or_default();
                        text_with(
                            &localizer,
                            msg::UI_GALLERY_TABS_CHOSEN,
                            &[("tab", MsgArg::Text(tab))],
                        )
                    }
                }
            </p>
            // In a part of the page that runs right to left: the first tab
            // stands at the right, and the left arrow goes to the next.
            <div dir="rtl">
                <Tabs
                    id="gallery-tabs-rtl"
                    name=TabsName::Label(t(msg::UI_GALLERY_TABS_RTL))
                    tabs=TabSet::new(
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_FIRST),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_FIRST_TEXT)}</p> },
                        ),
                        vec![
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_SECOND),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_SECOND_TEXT)}</p> },
                            ),
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_THIRD),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_THIRD_TEXT)}</p> },
                            ),
                        ],
                    )
                />
            </div>
        </section>
    }
}

/// The vertical tabs' demo (`#vertical-tabs-demo`).
#[component]
pub(super) fn VerticalTabsDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    view! {
        <section id="vertical-tabs-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABS_VERTICAL_HEADING) />
            // A column at the start of the panel: the down arrow goes to
            // the next tab and the up arrow to the one before.
            <Tabs
                id="gallery-tabs-vertical"
                name=TabsName::Label(t(msg::UI_GALLERY_TABS_PLAN))
                orientation=Orientation::Vertical
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_SOURCES),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_SOURCES_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_TIMES),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_TIMES_TEXT)}</p> },
                        ),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_HOOKS),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_HOOKS_TEXT)}</p> },
                        ),
                    ],
                )
            />
            // The same in a part of the page that runs right to left: the
            // column at the panel's start, on the right, and the down arrow
            // still going to the next tab.
            <div dir="rtl">
                <Tabs
                    id="gallery-tabs-vertical-rtl"
                    name=TabsName::Label(t(msg::UI_GALLERY_TABS_VERTICAL_RTL))
                    orientation=Orientation::Vertical
                    tabs=TabSet::new(
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_VERTICAL_RTL_FIRST),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_VERTICAL_RTL_FIRST_TEXT)}</p> },
                        ),
                        vec![
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_VERTICAL_RTL_SECOND),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_VERTICAL_RTL_SECOND_TEXT)}</p> },
                            ),
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_VERTICAL_RTL_THIRD),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_VERTICAL_RTL_THIRD_TEXT)}</p> },
                            ),
                        ],
                    )
                />
            </div>
        </section>
    }
}

/// The responsive tabs' demo (`#responsive-tabs-demo`).
#[component]
pub(super) fn ResponsiveTabsDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    let address_value = RwSignal::new(String::new());
    view! {
        <section id="responsive-tabs-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABS_RESPONSIVE_HEADING) />
            // As wide as the page: a column where the page is wide, a row
            // in a narrow window or with larger text. What is typed in the
            // middle panel stays while the layout changes.
            <Tabs
                id="gallery-tabs-responsive"
                name=TabsName::Label(t(msg::UI_GALLERY_TABS_SETTINGS))
                orientation=Orientation::Responsive
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_GENERAL),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_GENERAL_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_ADDRESS),
                            Holds::Controls,
                            view! {
                                <TextField
                                    id="gallery-tabs-address-input"
                                    localizer=Arc::clone(&localizer)
                                    value=address_value
                                    label=t(msg::UI_GALLERY_TABS_ADDRESS_FIELD)
                                    description=t(msg::UI_GALLERY_TABS_ADDRESS_HINT)
                                />
                            },
                        ),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_ABOUT),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_ABOUT_TEXT)}</p> },
                        ),
                    ],
                )
            />
            // The same tabs in a narrow column of a part of the page that
            // runs right to left: too narrow for the list beside the panel,
            // so a row, the left arrow going to the next tab.
            <div dir="rtl" class="max-w-sm">
                <Tabs
                    id="gallery-tabs-column"
                    name=TabsName::Label(t(msg::UI_GALLERY_TABS_COLUMN))
                    orientation=Orientation::Responsive
                    tabs=TabSet::new(
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_COLUMN_FIRST),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_COLUMN_FIRST_TEXT)}</p> },
                        ),
                        vec![
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_COLUMN_SECOND),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_COLUMN_SECOND_TEXT)}</p> },
                            ),
                            Tab::new(
                                t(msg::UI_GALLERY_TABS_COLUMN_THIRD),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_COLUMN_THIRD_TEXT)}</p> },
                            ),
                        ],
                    )
                />
            </div>
        </section>
    }
}

/// The demo of tabs starting on a marked tab (`#tabs-starting-on-a-marked-tab-demo`).
#[component]
pub(super) fn MarkedTabsDemo(localizer: Arc<Localizer>) -> impl IntoView {
    let localizer_t = Arc::clone(&localizer);
    let t = move |id| text(&localizer_t, id);
    view! {
        <section id="tabs-starting-on-a-marked-tab-demo">
            <DemoHeading text=t(msg::UI_GALLERY_TABS_MARKED_HEADING) />
            // A set started on its middle tab, which the gallery marks as
            // Settings marks the tab its address names: that tab is
            // selected and is the one Tab stop into the set, and the arrow
            // keys, Home and End move on from it.
            <Tabs
                id="gallery-tabs-marked"
                name=TabsName::Label(t(msg::UI_GALLERY_TABS_MARKED))
                tabs=TabSet::new(
                    Tab::new(
                        t(msg::UI_GALLERY_TABS_MARKED_SUMMARY),
                        Holds::Text,
                        view! { <p>{t(msg::UI_GALLERY_TABS_MARKED_SUMMARY_TEXT)}</p> },
                    ),
                    vec![
                        Tab::new(
                                t(msg::UI_GALLERY_TABS_MARKED_FILES),
                                Holds::Text,
                                view! { <p>{t(msg::UI_GALLERY_TABS_MARKED_FILES_TEXT)}</p> },
                            )
                            .selected_at_start(true),
                        Tab::new(
                            t(msg::UI_GALLERY_TABS_MARKED_TAGS),
                            Holds::Text,
                            view! { <p>{t(msg::UI_GALLERY_TABS_MARKED_TAGS_TEXT)}</p> },
                        ),
                    ],
                )
            />
        </section>
    }
}
