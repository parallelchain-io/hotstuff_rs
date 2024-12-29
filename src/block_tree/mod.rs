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
//! The main purpose of the definitions in the `block_tree` module is to store and maintain the local
//! replica's persistent block tree. However, the block tree is not the only persistent state in a
//! replica.
//!
//! In addition, The block tree module also stores additional data fundamental to the operation of the
//! HotStuff-rs. These data include data relating to the [two App-mutable
//! states](crate::app#two-app-mutable-states-app-state-and-validator-set), as well as state variables
//! used by the [HotStuff](crate::hotstuff) and [Pacemaker](crate::pacemaker) subprotocols to make sure
//! that the block tree is updated in a way that preserves its core [safety and liveness
//! invariants](invariants).
//!
//! The [List of State Variables](variables#list-of-state-variables) section of the `variables`
//! submodule comprehensively lists everything stored by the block tree module.
//!
//! # Pluggable persistence
//!
//! A key feature of HotStuff-rs is "pluggable persistence". The key enabler for pluggable persistence
//! is the fact that the `block_tree` module does not care about how its state variables are stored in
//! persistent storage, as long as whatever mechanism the library user chooses implements the abstract
//! functionality of a key-value store with atomic, batched writes.
//!
//! The interface that `block_tree` code uses to interact with this functionality is expressed in the
//! traits defined in the [`pluggables`] module. To plug a new persistence mechanism for use by a
//! replica, the user simply needs to implement these traits and provide an implementation of the top-
//! level [`KVStore`](pluggables::KVStore) trait to the `ReplicaSpecBuilder` by calling its
//! [`builder`](crate::replica::ReplicaSpecBuilder::kv_store) method.
//!
//! Upon replica startup, the provided implementations of the pluggable persistence traits are wrapped
//! inside types called block tree [`accessors`], which provide safe interfaces for accessing the block
//! tree for both library-internal and public use.

pub mod accessors;

pub mod invariants;

pub mod variables;

pub mod pluggables;
