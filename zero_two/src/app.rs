/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//!
//! 
//! ## Timing

use crate::types::*;
use crate::state::{ BlockTreeSnapshot, KVGet, SpeculativeAppState };

pub trait App<S: KVGet>: Send {
    fn id(&self) -> AppID;
    fn produce_block(&mut self, request: ProduceBlockRequest<S>) -> ProduceBlockResponse;
    fn validate_block(&mut self, request: ValidateBlockRequest<S>) -> ValidateBlockResponse;
}

pub struct ProduceBlockRequest<'a, S: KVGet> {
    cur_view: ViewNumber,
    parent_block: Option<CryptoHash>,
    block_tree: BlockTreeSnapshot<S>,
    app_state: SpeculativeAppState<'a, S>,
}

impl<'a, S: KVGet> ProduceBlockRequest<'a, S> {
    pub(crate) fn new(cur_view: ViewNumber, parent_block: Option<CryptoHash>, block_tree: BlockTreeSnapshot<S>) -> Self {
        let app_state = block_tree.speculative_app_state(parent_block.as_ref());
        Self {
            cur_view,
            parent_block,
            block_tree,
            app_state,
        }
    }

    pub fn cur_view(&self) -> ViewNumber {
        self.cur_view
    }

    pub fn parent_block(&self) -> Option<CryptoHash> {
        self.parent_block
    }

    pub fn block_tree(&self) -> &BlockTreeSnapshot<S> {
        &self.block_tree
    }

    pub fn app_state(&self, key: &[u8]) -> Option<&Vec<u8>> {
        self.app_state.get(key)
    }
}

pub struct ProduceBlockResponse {
    pub data_hash: CryptoHash,
    pub data: Data,
    pub app_state_updates: Option<AppStateUpdates>,
    pub validator_set_updates: Option<ValidatorSetUpdates>
}

pub struct ValidateBlockRequest<'a, S: KVGet> {
    proposed_block: Block,
    block_tree: BlockTreeSnapshot<S>,
    app_state: SpeculativeAppState<'a, S>,
}

impl<'a, S: KVGet> ValidateBlockRequest<'a, S> {
    pub(crate) fn new(proposed_block: Block, block_tree: BlockTreeSnapshot<S>) -> Self {
        let parent_block = if proposed_block.justify.is_genesis_qc() {
            None
        } else {
            Some(proposed_block.justify.block)
        };
        let app_state = block_tree.speculative_app_state(parent_block.as_ref());
        Self {
            proposed_block,
            block_tree,
            app_state
        }
    }

    pub fn proposed_block(&self) -> &Block {
        &self.proposed_block
    }

    pub fn block_tree(&self) -> &BlockTreeSnapshot<S> {
        &self.block_tree
    }

    pub fn app_state(&self, key: &[u8]) -> Option<&Vec<u8>> {
        self.app_state.get(key)
    }
}

pub enum ValidateBlockResponse {
    Valid {
        app_state_updates: Option<AppStateUpdates>,
        validator_set_updates: Option<ValidatorSetUpdates>
    },
    Invalid,
}
