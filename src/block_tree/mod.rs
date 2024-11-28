//! The persistent state of a replica.
//!
//! # The Block Tree
//!
//! -  
//!
//!
//!
//! # Pluggable persistence
//!
//! These are traits that implement the basic functionality of a transactional key-value store...
//!
//! - Traits:
//!     - KVStore
//!         - KVGet
//!         - KVWriteBatch
//!
//! # Accessing the Block Tree
//!
//! Wrapping the pluggable persistence traits are traits that implement the semantics of the block tree...
//!
//! The Block Tree may be stored in any key-value store of the library user's own choosing, as long as that
//! KV store can provide a type that implements [`KVStore`]. This state can be mutated through an instance
//! of [`BlockTree`], and read through an instance of [`BlockTreeSnapshot`], which can be created using
//! [`BlockTreeCamera`](crate::state::block_tree_snapshot::BlockTreeCamera).

pub mod accessors;

pub mod invariants;

pub mod variables;

pub mod pluggables;
