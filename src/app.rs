/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! The [`App`] trait, HotStuff-rs' interface into the business logic of users' applications.
//!
//! TODO: Talk about dependency injection.

use crate::{
    state::{app_block_tree_view::AppBlockTreeView, kv_store::KVStore},
    types::{
        basic::{AppStateUpdates, CryptoHash, Data, ViewNumber},
        block::Block,
        validators::ValidatorSetUpdates,
    },
};

///
///
/// # Timing
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
/// ## Reasoning behind timing constraint
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
/// |5|`+validate_block_duration`|<ul><li>Replicas send `Vote`.</li></ul>|
/// |6|`+EWNL`|<ul><li>Next Leader collects QC.</li><li>Next Leader leaves view.</li><li>Next Leader broadcasts `AdvanceView`.</li><li>Next Leader broadcasts `Proposal`</li></ul>|
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
    /// the sending of a `Vote` to the next leader.
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

/// Request for the app to produce a new block extending the parent block. Contains information about
/// the current [view number](crate::types::basic::ViewNumber), the parent
/// [block](crate::types::basic::CryptoHash) (if any), and the relevant
/// [state of the Block Tree](crate::state::app_block_tree_view::AppBlockTreeView).
pub struct ProduceBlockRequest<'a, K: KVStore> {
    cur_view: ViewNumber,
    parent_block: Option<CryptoHash>,
    block_tree_view: AppBlockTreeView<'a, K>,
}

impl<'a, K: KVStore> ProduceBlockRequest<'a, K> {
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

    pub fn cur_view(&self) -> ViewNumber {
        self.cur_view
    }

    pub fn parent_block(&self) -> Option<CryptoHash> {
        self.parent_block
    }

    pub fn block_tree(&self) -> &AppBlockTreeView<'a, K> {
        &self.block_tree_view
    }
}

/// Response from the app upon receiving a [request to produce a new block](ProduceBlockRequest).
/// Contains the new block's [data](crate::types::basic::Data), the
/// [hash of the data](crate::types::basic::CryptoHash),
/// the [app state updates](crate::types::basic::AppStateUpdates) associated with the block (if any),
/// and the [validator set updates](crate::types::validators::ValidatorSetUpdates) associated with the
/// block (if any).
pub struct ProduceBlockResponse {
    pub data_hash: CryptoHash,
    pub data: Data,
    pub app_state_updates: Option<AppStateUpdates>,
    pub validator_set_updates: Option<ValidatorSetUpdates>,
}

/// Request for the app to validate a proposed block. Contains information about the proposed
/// [block](crate::types::block::Block), and the relevant
/// [state of the Block Tree](crate::state::app_block_tree_view::AppBlockTreeView).
pub struct ValidateBlockRequest<'a, 'b, K: KVStore> {
    proposed_block: &'a Block,
    block_tree_view: AppBlockTreeView<'b, K>,
}

impl<'a, 'b, K: KVStore> ValidateBlockRequest<'a, 'b, K> {
    pub(crate) fn new(proposed_block: &'a Block, block_tree_view: AppBlockTreeView<'b, K>) -> Self {
        Self {
            proposed_block,
            block_tree_view,
        }
    }

    pub fn proposed_block(&self) -> &Block {
        self.proposed_block
    }

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

    /// Indicates that [`ValidateBlockRequest::proposed_block`] is invalid according the the `App`'s
    /// semantics.
    Invalid,
}
