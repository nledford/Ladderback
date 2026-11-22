use crate::ports::{LinkError, SigningError};

/// Why a connection after the handshake went on no further. None carries a
/// seed (I4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelError {
    /// The connection ended or didn't carry the message.
    Link(LinkError),
    /// A message the hub didn't sign on this connection, or signed out of
    /// sequence: nothing in it is acted on.
    Unsigned,
    /// A message whose expiry has passed on the hub's time, or lies further
    /// ahead than a hub signs.
    Expired,
    /// A message of a kind the agent doesn't take there, or whose payload
    /// isn't the kind's.
    OutOfTurn,
    /// The agent's count of its own messages ran out.
    SequenceSpent,
    Signing(SigningError),
}

impl From<LinkError> for ChannelError {
    fn from(error: LinkError) -> Self {
        Self::Link(error)
    }
}

impl From<SigningError> for ChannelError {
    fn from(error: SigningError) -> Self {
        Self::Signing(error)
    }
}
