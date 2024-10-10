//! Subprotocol that works to bring a quorum of validators into the same view so that the HotStuff
//! subprotocol can drive them to make progress.

pub mod messages;

pub(crate) mod protocol;

pub mod types;
