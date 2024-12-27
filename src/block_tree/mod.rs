//! The persistent state of a replica.
//!
//! # The Block Tree
//!
//! The Block Tree is the fundamental persistent data structure in HotStuff-rs, the HotStuff
//! subprotocol, and "Chain-Based" state machine replication in general.
//!
//! A block tree is a directed acyclic graph (or "Tree") of [Blocks](crate::types::block::Block) rooted
//! at a starting block called the "Genesis Block". Block trees are very "narrow" trees, in the sense
//! that most blocks in a block tree will only have one edge coming into it. Specifically, all blocks
//! in a block tree "under" a highest committed block (i.e., closer to genesis block to than the highest
//! committed block) will only have one edge coming out of it.
//!
//! Because of this, a block tree can be understood as being a linked list with a tree attached to it
//! at the highest committed block. In this understanding, there are **two kinds of blocks** in a block
//! tree:
//! 1. **Committed Blocks**: blocks in the linked list. These are permanently part of the block tree.
//! 2. **Speculative Blocks**: blocks in the tree. These, along with their descendants, are "pruned"
//!   when a "conflicting" block is committed.
//!
//! A speculative block is committed when a 3-Chain is formed extending it. This is the point at which
//! the HotStuff subprotocol can guarantee that a block that conflicts with the block can never be
//! committed and that therefore, the block is now permanently part of the block tree. The logic that
//! makes this guarantee sound is implemented in [`invariants`].
//!
//! # Beyond the Block Tree
//!
//! The main purpose of the `block_tree` module is to store and maintain the local replica's persistent
//! block tree. However, the block tree is not the only persistent state in a replica.
//!
//! The `block_tree` module also stores additional data fundamental to the operation of the HotStuff-rs.
//! These data include data relating to the [two App-mutable
//! states](crate::app#two-app-mutable-states-app-state-and-validator-set), as well as state variables
//! used by the [HotStuff](crate::hotstuff) and [Pacemaker](crate::pacemaker) subprotocols to make sure
//! that the block tree is updated in a way that preserves its core [safety and liveness
//! invariants](invariants).
//!
//! The documentation for the [`variables`] submodule lists everything stored by the `block_tree`
//! module.
//!
//! # Pluggable persistence
//!
//! - The block tree is kept in persistent storage, most probably in the host's filesystem.
//! - Library users get to choose how exactly this is done.
//! - HotStuff-rs merely requires that whatever the user provides as a persistence mechanism implements the abstract functionality of
//!   a key-value store with atomic, batched writes.
//! - This abstract functionality is made concrete by the traits defined in the `pluggables` module.
//! - Implement this for whatever persistence mechanism you want and pass in the type to the constructor in `mod replica`.
//!
//! # Accessing the Block Tree
//!
//! - The provided implementations of the pluggable persistence traits then get wrapped inside block tree `accessors`.
//! - Implementations of the pluggable persistence traits then get wrapped inside block tree `accessors`.
//! - These put the block tree++ variables in the right places in the KVStore and provide methods for reading and writing them both
//!   from code internal to this library and in user code.

pub mod accessors;

pub mod invariants;

pub mod variables;

pub mod pluggables;
