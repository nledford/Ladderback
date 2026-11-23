//! The disclosure menu (components §4.5): a button that shows and hides a
//! panel of links under it. [`rules`] decides when it opens and closes,
//! host-tested; this module is the glue that draws it.

// When the menu opens and closes and what its button tells, decided on the
// host; `DisclosureMenu` is its glue.
pub(crate) mod rules;
