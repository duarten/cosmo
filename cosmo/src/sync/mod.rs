//! Synchronization utilities.

use std::fmt;

pub mod channel;
pub mod queue;

/// An error that may be emitted when attempting to send a value on a bounded
/// channel.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TrySendError<T> {
    /// The channel is bounded and was full when the send was attempted.
    Full(T),
    /// There are no receivers to get the value.
    Disconnected(T),
}

impl<T> TrySendError<T> {
    /// Consume the error and return the value that wasn't sent.
    pub fn into_inner(self) -> T {
        match self {
            Self::Full(value) | Self::Disconnected(value) => value,
        }
    }
}

impl<T> fmt::Debug for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TrySendError::Full(..) => "Full(..)".fmt(f),
            TrySendError::Disconnected(..) => "Disconnected(..)".fmt(f),
        }
    }
}

impl<T> fmt::Display for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrySendError::Full(..) => "sending on a full queue".fmt(f),
            TrySendError::Disconnected(..) => "sending on a disconnected queue".fmt(f),
        }
    }
}

impl<T: core::fmt::Debug> std::error::Error for TrySendError<T> {}

/// An error that may be emitted when attempting to send a value.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SendError<T> {
    /// There are no more receiver, so no values can be sent.
    Disconnected(T),
}

impl<T> fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::Disconnected(..) => "sending on a disconnected queue".fmt(f),
        }
    }
}

impl<T: core::fmt::Debug> std::error::Error for SendError<T> {}

/// An error that may be emitted when attempting to receive a value on bounded
/// queues.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TryRecvError {
    /// The queue was empty when the receive was attempted.
    Empty,
    /// There are no more senders, so no values can be received.
    Disconnected,
}

impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TryRecvError::Empty => "receiving on an empty queue".fmt(f),
            TryRecvError::Disconnected => "receiving on a disconnected queue".fmt(f),
        }
    }
}

impl std::error::Error for TryRecvError {}

/// An error that may be emitted when attempting to receive a value.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RecvError {
    /// There are no more senders, so no values can be received.
    Disconnected,
}

impl fmt::Display for RecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecvError::Disconnected => "receiving on a disconnected queue".fmt(f),
        }
    }
}

impl std::error::Error for RecvError {}
