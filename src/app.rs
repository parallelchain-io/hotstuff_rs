/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! The [App] trait, HotStuff-rs' interface into the business logic of users' applications.

use crate::types::*;
use crate::state::{AppBlockTreeView, KVStore};

pub trait App<K: KVStore>: Send {
    fn id(&self) -> AppID;
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
