//! The table (components §4.5): a real `<table>` of keyed rows, sortable by
//! its columns' buttons. [`rules`] decides the order of its rows, what its
//! headers tell a screen reader and what shows in its place, host-tested;
//! this module is the glue that draws it.

pub(crate) mod rules;
