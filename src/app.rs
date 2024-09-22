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

/// # Timing
///
///
///
/// TODO: how long should `produce_block`, `validate_block` take to execute? How about `validate_block_for_sync`?
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
    /// # Difference compared to `validate_block`
    ///
    /// Read ["Timing"](App#Timing).
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
