/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! The [App] trait, HotStuff-rs' interface into the business logic of users' applications.
//!
//! ## The App trait
//!
//! After receiving your app through [crate::replica::Replica::start], replicas communicate with it
//! by calling the methods it has implemented as part of the App trait when specific things happen.
//!
//! The App trait has three methods.
//!
//! The two most important ones--[App::produce_block] and [App::validate_block]--take in a request and return a response:
//! 1. `produce_block` is called when the replica becomes a leader and has to produce a new
//!    block. Your app should respond with the data and the data hash of a block extending the [parent block](ProduceBlockRequest::parent_block) included in the request, as well as the [app state updates](crate::types::AppStateUpdates) and [validator set updates](crate::types::ValidatorSetUpdates) that executing it causes.
//! 2. `validate_block` is called in two situations: when the replica receives a proposal, and when
//!    the replica is syncing. Your app should respond with whether the block is valid (according to
//!    the semantics of the application), and again with the app state updates and validator set
//!    updates that executing the block causes.
//!
//! ## Chain ID
//!
//! The [third method](App::chain_id) is called to get a "chain ID".
//!
//! Each HotStuff-rs blockchain should be identified by a [chain ID](crate::types::ChainID). This
//! is included in votes and other messages so that replicas don't mistake messages and blocks for
//! one HotStuff-rs blockchain does not get mistaken for those for another blockchain.
//!
//! In most cases, having a chain ID that collides with another blockchain is harmless. But
//! if your application is a Proof of Stake public blockchain, this may cause a slashable offence
//! if you operate validators in two chains that use the same keypair. So ensure that you don't
//! operate a validator in two blockchains with the same keypair.

use crate::types::*;
use crate::state::{AppBlockTreeView, KVStore};

pub trait App<K: KVStore>: Send {
    fn chain_id(&self) -> ChainID;
    fn produce_block(&mut self, request: ProduceBlockRequest<K>) -> ProduceBlockResponse;
    fn validate_block(&mut self, request: ValidateBlockRequest<K>) -> ValidateBlockResponse;
}

pub struct ProduceBlockRequest<'a, K: KVStore> {
    cur_view: ViewNumber,
    parent_block: Option<CryptoHash>,
    block_tree_view: AppBlockTreeView<'a, K>,
}

impl<'a, K: KVStore> ProduceBlockRequest<'a, K> {
    pub(crate) fn new(cur_view: ViewNumber, parent_block: Option<CryptoHash>, block_tree_view: AppBlockTreeView<'a, K>) -> Self {
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

pub struct ProduceBlockResponse {
    pub data_hash: CryptoHash,
    pub data: Data,
    pub app_state_updates: Option<AppStateUpdates>,
    pub validator_set_updates: Option<ValidatorSetUpdates>
}

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
        &self.proposed_block
    }

    pub fn block_tree(&self) -> &AppBlockTreeView<K> {
        &self.block_tree_view
    }
}

pub enum ValidateBlockResponse {
    Valid {
        app_state_updates: Option<AppStateUpdates>,
        validator_set_updates: Option<ValidatorSetUpdates>
    },
    Invalid,
}
