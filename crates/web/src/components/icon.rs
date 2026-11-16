//! The icon (components §4.5): one Lucide glyph, decoration unless its
//! caller gives it a label. [`rules`] decides what a screen reader meets and
//! how it is drawn, host-tested; this module is the glue that draws it.

pub(crate) mod rules;
