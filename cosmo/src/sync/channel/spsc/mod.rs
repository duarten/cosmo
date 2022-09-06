//! Fast single-producer, single-consumer channels.

mod bounded;

pub use bounded::{bounded, Receiver, Sender};
