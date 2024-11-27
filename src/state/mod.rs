//! Persistent state of a replica.
//!
//! # Accessing the Block Tree
//!
//! The Block Tree may be stored in any key-value store of the library user's own choosing, as long as that
//! KV store can provide a type that implements [`KVStore`]. This state can be mutated through an instance
//! of [`BlockTree`], and read through an instance of [`BlockTreeSnapshot`], which can be created using
//! [`BlockTreeCamera`](crate::state::block_tree_snapshot::BlockTreeCamera).

pub mod app_block_tree_view;
pub mod block_tree;
pub mod block_tree_snapshot;
pub mod invariants;
pub mod kv_store;
pub mod state_variables;
pub mod write_batch;
