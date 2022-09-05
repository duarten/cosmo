//! Fast single-producer, single-consumer queues.

mod bounded;

pub use bounded::{bounded, Receiver, Sender};
