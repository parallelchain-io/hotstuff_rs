//! Types and traits that are used across multiple sub-protocols or components of HotStuff-rs.
//!
//! Other types and traits, specific to single components of HotStuff-rs protocols, can be found in
//! the "types" submodules of their components, e.g., [`crate::hotstuff::types`].

pub mod data_types;

pub mod update_sets;

pub mod crypto_primitives;

pub mod block;

pub mod validator_set;

pub mod signed_messages;
