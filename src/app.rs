/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! The [App] trait, HotStuff-rs' interface into the business logic of users' applications.
//!
//! ## The App trait
//!
//! After receiving your app through [start](crate::replica::Replica::start), replicas communicate with it
//! by calling the methods it has implemented as part of the App trait when specific things happen.
//!
//! The App trait has three methods, each of which returns a response when called with a request:
//! 1. `produce_block` is called when the replica becomes a leader and has to produce a new
//!    block. Your app should respond with the data and the data hash of a block extending the
//!    parent block included in the request, as well as the [app state updates](crate::types::AppStateUpdates) 
//!    and [validator set updates](crate::types::ValidatorSetUpdates) that executing it causes.
//! 2. `validate_block` is called  when the replica receives a proposal. Your app should respond
//!    with whether the block is valid (according to the semantics of the application), and again 
//!    with the [app state updates](crate::types::AppStateUpdates) and [validator set updates](crate::types::ValidatorSetUpdates)
//!    that executing the block causes.
//! 3. `validate_block_for_sync` is called when the replica is syncing. Your app should respond
//!    with whether the block is valid (according to the semantics of the application), and again
//!    with the [app state updates](crate::types::AppStateUpdates) and [validator set updates](crate::types::ValidatorSetUpdates)\
//!    that executing the block causes.

use crate::state::{AppBlockTreeView, KVStore};
use crate::types::{
    basic::{AppStateUpdates, CryptoHash, Data, ViewNumber},
    block::Block,
    validators::ValidatorSetUpdates,
};

pub trait App<K: KVStore>: Send {
    fn produce_block(&mut self, request: ProduceBlockRequest<K>) -> ProduceBlockResponse;
    fn validate_block(&mut self, request: ValidateBlockRequest<K>) -> ValidateBlockResponse;
    fn validate_block_for_sync(&mut self, request: ValidateBlockRequest<K>) -> ValidateBlockResponse;
}

/// Request for the app to produce a new block extending the parent block. Contains information about the current [view number](crate::types::ViewNumber),
/// the parent [block](crate::types::CryptoHash) (if any), and the relevant [state of the Block Tree](crate::state::AppBlockTreeView).
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
/// [state of the Block Tree](crate::state::AppBlockTreeView).
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

/// Response from the app upon receiving a [request to validate a block](ValidateBlockRequest). 
/// The response can either:
/// 1. Assert that the block is valid, and return the 
///    [app state updates](crate::types::basic::AppStateUpdates) associated with the block (if any), as
///    well as the [validator set updates](crate::types::validators::ValidatorSetUpdates) associated with
///    the block (if any), or
/// 2. Assert that the block is invalid.
pub enum ValidateBlockResponse {
    Valid {
        app_state_updates: Option<AppStateUpdates>,
        validator_set_updates: Option<ValidatorSetUpdates>,
    },
    Invalid,
}
