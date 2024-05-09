/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines [AppBlockTreeView], an app-specific read-only interface for the 
//! [block tree](crate::state::block_tree::BlockTree).

use crate::hotstuff::types::QuorumCertificate;
use crate::types::{
    basic::{AppStateUpdates, BlockHeight, CryptoHash, Data, DataLen, Datum},
    block::Block,
    validators::ValidatorSet
};

use super::block_tree::BlockTreeError;
use super::{block_tree::BlockTree, kv_store::KVStore};


pub struct AppBlockTreeView<'a, K: KVStore> {
    pub(super) block_tree: &'a BlockTree<K>,
    pub(super) parent_app_state_updates: Option<AppStateUpdates>,
    pub(super) grandparent_app_state_updates: Option<AppStateUpdates>,
    pub(super) great_grandparent_app_state_updates: Option<AppStateUpdates>,
}

impl<'a, K: KVStore> AppBlockTreeView<'a, K> {
    pub fn block(&self, block: &CryptoHash) -> Result<Option<Block>, BlockTreeError> {
        self.block_tree.block(block)
    }

    pub fn block_height(&self, block: &CryptoHash) -> Result<Option<BlockHeight>, BlockTreeError> {
        self.block_tree.block_height(block)
    }

    pub fn block_justify(&self, block: &CryptoHash) -> Result<QuorumCertificate, BlockTreeError> {
        self.block_tree.block_justify(block)
    }

    pub fn block_data_hash(&self, block: &CryptoHash) -> Result<Option<CryptoHash>, BlockTreeError> {
        self.block_tree.block_data_hash(block)
    }

    pub fn block_data_len(&self, block: &CryptoHash) -> Result<Option<DataLen>, BlockTreeError> {
        self.block_tree.block_data_len(block)
    }

    pub fn block_data(&self, block: &CryptoHash) -> Result<Option<Data>, BlockTreeError> {
        self.block_tree.block_data(block)
    }

    pub fn block_datum(&self, block: &CryptoHash, datum_index: u32) -> Option<Datum> {
        self.block_tree.block_datum(block, datum_index)
    }

    pub fn block_at_height(&self, height: BlockHeight) -> Result<Option<CryptoHash>, BlockTreeError> {
        self.block_tree.block_at_height(height)
    }

    pub fn app_state(&'a self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(parent_app_state_updates) = &self.parent_app_state_updates {
            if parent_app_state_updates.contains_delete(&key.to_vec()) {
                return None;
            } else if let Some(value) = parent_app_state_updates.get_insert(&key.to_vec()) {
                return Some(value.clone());
            }
        }

        if let Some(grandparent_app_state_changes) = &self.grandparent_app_state_updates {
            if grandparent_app_state_changes.contains_delete(&key.to_vec()) {
                return None;
            } else if let Some(value) = grandparent_app_state_changes.get_insert(&key.to_vec()) {
                return Some(value.clone());
            }
        }

        if let Some(great_grandparent_app_state_changes) = &self.great_grandparent_app_state_updates
        {
            if great_grandparent_app_state_changes.contains_delete(&key.to_vec()) {
                return None;
            } else if let Some(value) =
                great_grandparent_app_state_changes.get_insert(&key.to_vec())
            {
                return Some(value.clone());
            }
        }

        self.block_tree.committed_app_state(key)
    }

    pub fn validator_set(&self) -> Result<ValidatorSet, BlockTreeError> {
        self.block_tree.committed_validator_set()
    }
}