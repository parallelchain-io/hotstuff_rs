//! Types and traits that are used across multiple sub-protocols or components of HotStuff-rs.
//!
//! Other types and traits, specific to single components of HotStuff-rs protocols, can be found in
//! the "types" submodules of their components, e.g., [`crate::hotstuff::types`].

pub mod basic;

pub mod block;

pub mod validators;

pub mod keypair;

pub mod signed_messages;
