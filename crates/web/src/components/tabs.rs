//! The tabs (components §4.5, "Tabs"): the WAI-ARIA tabs pattern on native
//! buttons. [`rules`] decides which tab a key moves to, which is selected
//! and which is the list's Tab stop, and each element's ids and ARIA
//! attributes, host-tested; this module is the glue that draws them.

// What a set of tabs does, decided on the host; `Tabs` is its glue.
pub(crate) mod rules;
