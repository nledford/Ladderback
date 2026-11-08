//! Typed message IDs. Call sites name a message through a constant of
//! [`crate::msg`], so an ID that isn't in the `en-US` catalog doesn't compile.

/// The ID of a catalog message, such as `cli-listening`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MessageId(&'static str);

impl MessageId {
    /// An ID from its text. Use the constants in [`crate::msg`]: this is for
    /// tests and tools that bring their own catalog, where no constant
    /// exists.
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// The ID as written in the catalog.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}
