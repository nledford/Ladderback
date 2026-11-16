//! The tooltip (components §4.5): a short note beside a control, shown on
//! keyboard focus at once and on a resting pointer after a moment. [`rules`]
//! decides what a screen reader meets, when it shows and where it stands,
//! host-tested; this module is the glue that measures the page and draws it.

pub(crate) mod rules;
