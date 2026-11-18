//! The steps (components §4.5): a process's steps in order, each with its
//! state, and where the process has reached. [`rules`] decides each step's
//! state, its icon, its words and its connector, and what a change of step
//! is said as, host-tested; this module is the glue that draws them and says
//! the change through the announcer.

// What a list of steps shows and says, decided on the host; `Steps` is its
// glue.
pub(crate) mod rules;
