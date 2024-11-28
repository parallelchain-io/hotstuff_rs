/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Pluggable, replicable applications.
//!
//! # Applications
//!
//! The basic interaction between HotStuff-rs and the library user is this:
//! 1. Library user specifies a state machine.
//! 2. Library user provides the state machine to HotStuff-rs
//! 3. HotStuff-rs replicates the state machine.
//!
//! We refer to the state machine in this basic interaction as the **"application"**. In short, an
//! application is a state machine that mutates
//! [two states](#two-app-mutable-states-app-state-and-validator-set), namely the *App State* and the
//! *Validator Set*, in response to receiving a payload called a [`Block`]. In other words, an
//! application can be thought of as a function (a "state transition function") with the following type
//! signature: ```(AppState, ValidatorSet, Block) -> (AppState, ValidatorSet)```.
//!
//! To specify an application, the library user must implement the [`App`] trait. This entails
//! implementing a state transition function and exposing this function to HotStuff-rs through three
//! different [required methods](App#required-methods), namely `produce_block`, `validate_block`, and
//! `validate_block_for_sync`.
//!
//! Each of the above three methods correspond to a distinct context in which HotStuff-rs will call the
//! application's state transition function. For example, HotStuff-rs will call `produce_block` when the
//! local replica is a leader and needs to produce a new block to propose it. The context in which each
//! function is called is documented in their respective Rustdocs.
//!
//! # Two app-mutable states: App State and Validator Set
//!
//! The two states that an application can mutate in response to a block are the App State, and the
//! Validator Set. Both states are stored in the block tree, which itself is stored in the pluggable
//! [`KVStore`](crate::block_tree::pluggables::KVStore) provided to HotStuff-rs by the library user.
//!
//! The **App State** is a set of key-value mappings, in which the keys and values are be arbitrary byte
//! strings. The library user is completely free to decide the structure and contents of this mapping
//! in whichever way supports their use-case. HotStuff-rs itself does not assume anything about the app
//! state, nor does it insert anything into the app state when a replica is
//! [`initialize`](crate::replica::Replica::initialize)d besides what the library user configures as the
//! `initial_app_state`.
//!
//! For more information about the **Validator Set**, read the rustdocs in the
//! [`validator_set`](crate::types::validator_set) module.
//!
//! `App` code tells HotStuff-rs to make changes to the app state and the validator set by listing the
//! changes that should be made in an [`AppStateUpdates`] or [`ValidatorSetUpdates`] (respectively) and
//! returning these changes from calls to its three methods.

use crate::{
    block_tree::{accessors::app::AppBlockTreeView, pluggables::KVStore},
    types::{
        block::Block,
        data_types::{CryptoHash, Data, ViewNumber},
        update_sets::{AppStateUpdates, ValidatorSetUpdates},
    },
};

/// Trait implemented by applications that are to be replicated by HotStuff-rs.
///
/// # Determinism requirements
///
/// In order for the state machine to be correctly replicated across all replicas, types that implement
/// `App` must ensure that their implementations of `produce_block`, `validate_block`, and
/// `validate_block_for_sync` are deterministic. That is, for any particular request (e.g.,
/// [`ProduceBlockRequest`]), a function must always produce the same response
/// ([`ProduceBlockResponse`]), regardless of whether the machine executing the call has an Intel CPU
/// or an AMD CPU, whether a pseudorandom number generator spits out 0 or 1, or whether it is currently
/// raining outside, etc.
///
/// The key to making sure that these methods are deterministic is ensuring that their respective
/// method bodies depend on and **only on** the information that is available through the public methods
/// of their request type. In particular, this means not reading the block tree through a
/// `BlockTreeSnapshot`, but reading it only through [`AppBlockTreeView`].

/// # Timing requirements
///
/// HotStuff-rs calls `produce_block` when a replica has to produce a block, and calls `validate_block`
/// when a replica has to validate a block.
///
/// These calls are **blocking** (synchronous) calls, so a replica will not continue making progress
/// until they complete. Because of this, library users should implement `produce_block` and
/// `validate_block` satisfy a specific timing constraint so that replicas do not spend too long in
/// these functions and cause views to timeout and progress to stall. This timing constraint is
/// exactly:
///
/// `4 * EWNL + produce_block_duration + validate_block_duration < max_view_time`,
///
/// where:
/// - `EWNL`: "Expected Worst-case Network Latency". That is, the maximum duration of time needed to send
///   a message from a validator to another validator under "normal" network conditions.
/// - `produce_block_duration`: how long `produce_block` takes to execute.
/// - `validate_block_duration`: how long `validate_block` takes to execute.
/// - `max_view_time`: the provided
///   [`Configuration::max_view_time`](crate::replica::Configuration::max_view_time).
///
/// ## Rationale behind timing requirements
///
/// Let's say a view begins at time T for the proposer of the view, and assume that:
/// - Messages take `EWNL` to deliver between validators,
/// - The time taken to process messages is negligible besides `produce_block_duration` and
///   `validate_block_duration`, and
/// - Every step begins after the previous step has ended (this is a simplifying assumption. In reality,
///   for example, the Proposer does not have to wait for replicas to receive `AdvanceView` in order to
///   start producing a block, and therefore, step 3 should really happen at
///   `T+diff(EWNL, produce_block_duration)` instead of at `T+EWNL+produce_block_duration` as below).
///
/// Then the view will proceed as follows:
///
/// |Step No.|Time after T (cumulative)|Events|
/// |---|---|---|
/// |1|`+0`|<ul><li>Proposer enters view.</li><li>Proposer broadcasts `AdvanceView`.</li><li>Proposer broadcasts `Proposal`</li></ul>|
/// |2|`+EWNL`|<ul><li>Replicas receive `AdvanceView`.</li><li>Replicas enter view.</li></ul>|
/// |3|`+produce_block_duration`|<ul><li>Proposer enters view.</li></ul>|
/// |4|`+EWNL`|<ul><li>Replicas receive `Proposal`.</li></ul>|
/// |5|`+validate_block_duration`|<ul><li>Replicas send `PhaseVote`s.</li></ul>|
/// |6|`+EWNL`|<ul><li>Next Leader collects `PhaseCertificate`.</li><li>Next Leader leaves view.</li><li>Next Leader broadcasts `AdvanceView`.</li><li>Next Leader broadcasts `Proposal`</li></ul>|
/// |7|`+EWNL`|<ul><li>Replicas receive `AdvanceView`.</li><li>Replicas leave view.</li></ul>|
///
/// In the view above (and indeed in any view), there are two possible cases about the identities of
/// Proposer and Next Leader:
/// 1. If Proposer == Next Leader, then:
///     1. The Proposer/Next Leader enters the view at step no. 1 and leaves at step no. 6, spending `3EWNL +
///       produce_block_duration + validate_block_duration` in the view.
///     2. Other replicas enter the view at step no. 2 and leave at step no. 7, spending `3EWNL +
///       produce_block_duration + validate_block_duration` in the view.
/// 2. If Proposer != Next Leader, then:
///     1. Proposer enters the view at step no.1 and leaves at step no. 7, spending `4EWNL +
///       produce_block_duration + validate_block_duration` in the view.
///     2. Next Leader enters the view at step no. 2 and leaves at step no. 6, spending `2EWNL +
///       produce_block_duration + validate_block_duration` in the view.
///     3. Other replicas enter the view at step no. 2 and leave at step no. 7, spending `3EWNL +
///       produce_block_duration + validate_block_duration` in the view.
///
/// This shows that the most time a replica will spend in a view under normal network conditions is
/// `4EWNL + produce_block_duration + validate_block_duration` (case 2.1), which is exactly the
/// left-hand-side of the timing constraint inequality.
pub trait App<K: KVStore>: Send {
    /// Called by HotStuff-rs when the replica becomes a leader and has to produce a new `Block` to be
    /// inserted into the block tree and proposed to other validators.
    fn produce_block(&mut self, request: ProduceBlockRequest<K>) -> ProduceBlockResponse;

    /// Called by HotStuff-rs when the replica receives a `Proposal` and has to validate the `Block` inside
    /// it to decide whether or not it should insert it into the block tree and vote for it.
    fn validate_block(&mut self, request: ValidateBlockRequest<K>) -> ValidateBlockResponse;

    /// Called when the replica is syncing and receives a `BlockSyncResponse` and has to validate the
    /// `Block` inside it to decide whether or not it should insert it into the block tree and vote for it.
    ///
    /// # Purpose
    ///
    /// There are several reasons why implementations of `App` may want calls to `validate_block` to take
    /// *at least* a certain amount of time.
    ///
    /// For example, implementors may want to limit the rate at which views proceed (e.g., to prevent the
    /// block tree from growing too quickly and exhausting disk space), and may choose to do so by making
    /// their implementations of `produce_block` and `validate_block` cause a thread sleep until a minimum
    /// amount of time is reached.
    ///
    /// Such a thread sleep solution works to slow down consensus decisions by, among other things, causing
    /// replicas to block for the amount of time when receiving a block through a `Proposal`, delaying
    /// the sending of a `PhaseVote` to the next leader.
    ///
    /// However, such a solution will not only slow down consensus decisions, it will *also* slow down
    /// Block Sync. In general, implementors will want `validate_block` to take at least a minimum amount of
    /// time when it is called within a view, but complete as quickly as possible when called during block
    /// sync.
    ///
    /// This is where `validate_block_for_sync` comes in. With `validate_block_for_sync`, implementers can
    /// rely on the fact that `validate_block` will *only* be called within a view, adding a thread sleep
    /// or other mechanism in it will not slow down sync as long as the mechanism is not also added to
    /// `validate_block_for_sync`.
    fn validate_block_for_sync(
        &mut self,
        request: ValidateBlockRequest<K>,
    ) -> ValidateBlockResponse;
}

/// Request for an `App` to produce a new block extending a specific `parent_block`.
pub struct ProduceBlockRequest<'a, K: KVStore> {
    cur_view: ViewNumber,
    parent_block: Option<CryptoHash>,
    block_tree_view: AppBlockTreeView<'a, K>,
}

impl<'a, K: KVStore> ProduceBlockRequest<'a, K> {
    /// Create a new `ProduceBlockRequest`.
    pub(crate) fn new(
        cur_view: ViewNumber,
        parent_block: Option<CryptoHash>,
        block_tree_view: AppBlockTreeView<'a, K>,
    ) -> Self {
        Self {
            cur_view,
            parent_block,
            block_tree_view,
        }
    }

    /// Get the current view of the replica.
    pub fn cur_view(&self) -> ViewNumber {
        self.cur_view
    }

    /// Get the parent block of the block that this `produce_block` call should extend.
    pub fn parent_block(&self) -> Option<CryptoHash> {
        self.parent_block
    }

    /// Get a current view of the block tree that this `produce_block` call can safely read from without
    /// risking [non-determinism](Self#determinism-requirements).
    pub fn block_tree(&self) -> &AppBlockTreeView<'a, K> {
        &self.block_tree_view
    }
}

/// Response from an `App` to a [`ProduceBlockRequest`].
pub struct ProduceBlockResponse {
    /// The `data_hash` field of the produced block.
    pub data_hash: CryptoHash,

    /// The `data` field of the produced block.
    pub data: Data,

    /// The `AppStateUpdates` that the produced block will cause when it is committed.
    pub app_state_updates: Option<AppStateUpdates>,

    /// The `ValidatorSetUpdates` that the produced block will cause when it is committed.
    pub validator_set_updates: Option<ValidatorSetUpdates>,
}

/// Request for the app to validate a proposed block. Contains information about the proposed
/// [block](crate::types::block::Block), and the relevant
/// [state of the Block Tree](crate::block_tree::accessors::app::AppBlockTreeView).
pub struct ValidateBlockRequest<'a, 'b, K: KVStore> {
    proposed_block: &'a Block,
    block_tree_view: AppBlockTreeView<'b, K>,
}

impl<'a, 'b, K: KVStore> ValidateBlockRequest<'a, 'b, K> {
    /// Create a new `ValidateBlockRequest`.
    pub(crate) fn new(proposed_block: &'a Block, block_tree_view: AppBlockTreeView<'b, K>) -> Self {
        Self {
            proposed_block,
            block_tree_view,
        }
    }

    /// Get the block that this `validate_block` call should validate.
    pub fn proposed_block(&self) -> &Block {
        self.proposed_block
    }

    /// Get a current view of the block tree that this `validate_block` call can safely read from without
    /// risking [non-determinism](Self#determinism-requirements).
    pub fn block_tree(&self) -> &AppBlockTreeView<K> {
        &self.block_tree_view
    }
}

/// Response from an `App` upon receiving a [`ValidateBlockRequest`].
pub enum ValidateBlockResponse {
    /// Indicates that [`ValidateBlockRequest::proposed_block`] is valid according to the `App`'s semantics.
    Valid {
        /// The `AppStateUpdates` that the proposed block will cause when it is committed.
        app_state_updates: Option<AppStateUpdates>,

        /// The `ValidatorSetUpdates` that the proposed block will cause when it is committed.
        validator_set_updates: Option<ValidatorSetUpdates>,
    },

    /// Indicates either that [`ValidateBlockRequest::proposed_block`] is invalid according to the
    /// `App`'s semantics, or that the execution of `validate_block` for the proposed block will exceed the
    /// [timing requirements](App#timing-requirements).
    Invalid,
}
