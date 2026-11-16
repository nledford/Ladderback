use leptos::prelude::*;

use crate::spinner::{self, LabelShown};
use crate::variants::{Placement, Size, Tone};

/// A spinner (§4.5): a drawing that turns while the page waits for something
/// it can't show yet, always with its `label`, which says what it waits for.
/// The label is required, so a spinner without one can't be built; it must
/// also say something, since an empty one leaves a spinner whose label is
/// hidden an image without a name (1.1.1).
///
/// By default the label is shown beside the drawing as ordinary text, which
/// a screen reader reads, and the drawing is `aria-hidden`; where there is no
/// room for it (`shown=LabelShown::Hidden`), the drawing is `role="img"`
/// named by the label. [`crate::spinner`] decides both. Its `tone`, `size`
/// and `placement` come from the shared vocabulary (decision 108): Primary,
/// Small and Inline unless the caller names others. The tone colors the
/// drawing only, the label keeping the text's color; the size is the
/// drawing's length (Small 1em, Medium 1.5em, Large 2em, XLarge 3em); the
/// placement goes on the spinner's line, an element around it, never on the
/// spinner itself; on a line of its own (Start, Center, End) a long label
/// wraps beside the drawing, never under it. The drawing is a faint track
/// and an arc, which turns once in 0.9 s and stands still when the reader
/// asks for less motion, the label carrying the meaning.
///
/// What the caller owes WCAG 2.2 AA, since the spinner does none of it:
///
/// - **Announcing (4.1.3, Status Messages).** The spinner is never a live
///   region (`role="status"`, `aria-live`): the page keeps one of each kind,
///   the announcer's (A6). When a spinner appears because of something the
///   reader did, such as a search or a save, the page says the wait, and its
///   end, through the announcer ([`crate::live::reply`]).
/// - **Moving (2.2.2, Pause, Stop, Hide).** A spinner that starts by
///   itself, not from something the reader did, beside content the reader
///   can use, and may turn for more than 5 s, would need a way to stop or
///   hide it: show a [`Progress`](super::Progress) bar or words there instead, as a running
///   [`StatusBadge`](super::StatusBadge) does. One the reader started, one that is over within
///   5 s, or one that is all the page shows while nothing on it can be used
///   yet needs nothing. Technical design §4.5 has the rule in full.
/// - **Busy.** The region it stands for, such as the table being read, gets
///   `aria-busy="true"` until it is filled.
/// - **A busy button** keeps focus while the spinner stands in it: it takes
///   `aria-disabled="true"`, not `disabled`, which would drop focus to the
///   page (A1), and its handler ignores presses until the work ends, as
///   `aria-disabled` stops none.
#[component]
pub fn Spinner(
    #[prop(into)] label: Signal<String>,
    #[prop(optional)] shown: LabelShown,
    #[prop(default = spinner::default_size())] size: Size,
    #[prop(optional)] tone: Tone,
    #[prop(optional)] placement: Placement,
) -> impl IntoView {
    view! {
        <span class=placement.class()>
            <span class=spinner::spinner_class(tone, size)>
                // A track and a quarter arc of one circle (r 9: 56.55
                // around), the arc starting at the top, stroked in the
                // drawing's color, its tone. The arc turns inside its group,
                // so the drawing's box never does. The attributes give it
                // the text's height without the stylesheet, which sets each
                // size's length.
                <svg
                    class="spinner-graphic"
                    xmlns="http://www.w3.org/2000/svg"
                    width="1em"
                    height="1em"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="3"
                    stroke-linecap="round"
                    focusable="false"
                    role=shown.graphic_role()
                    aria-hidden=shown.graphic_hidden()
                    aria-label=move || shown.graphic_name(&label.get())
                >
                    <circle class="spinner-track" cx="12" cy="12" r="9" />
                    <g class="spinner-turn">
                        <circle
                            class="spinner-arc"
                            cx="12"
                            cy="12"
                            r="9"
                            stroke-dasharray="14.14 42.41"
                            transform="rotate(-90 12 12)"
                        />
                    </g>
                </svg>
                {shown
                    .shows_label()
                    .then(|| view! { <span class="spinner-label">{move || label.get()}</span> })}
            </span>
        </span>
    }
}
