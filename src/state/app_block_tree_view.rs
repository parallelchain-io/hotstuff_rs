/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Special, read-only interface for querying the Block Tree used only by `App`s.

use crate::hotstuff::types::QuorumCertificate;
use crate::types::{
    basic::{AppStateUpdates, BlockHeight, CryptoHash, Data, DataLen, Datum},
    block::Block,
    validators::ValidatorSet,
};

use super::block_tree::BlockTreeError;
use super::{block_tree::BlockTree, kv_store::KVStore};

/// View of the block tree, which may be used by the [`App`](crate::app::App) to produce or validate a
/// block.
///
/// Internally, it contains:
/// 1. A reference to the block tree,
/// 2. A vector of optional app state updates associated with the ancestors of a given block, starting
///    from the block's parent (if any) and ending at the oldest uncommitted ancestor.
pub struct AppBlockTreeView<'a, K: KVStore> {
    pub(super) block_tree: &'a BlockTree<K>,
    pub(super) pending_ancestors_app_state_updates: Vec<Option<AppStateUpdates>>,
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

    pub fn block_data_hash(
        &self,
        block: &CryptoHash,
    ) -> Result<Option<CryptoHash>, BlockTreeError> {
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

    pub fn block_at_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<CryptoHash>, BlockTreeError> {
        self.block_tree.block_at_height(height)
    }

    /// Get the current app state associated with a given key, as per the state of the key value store
    /// reflecting the app state changes (possibly pending) introduced by the chain of ancestors of a
    /// given block.
    pub fn app_state(&'a self, key: &[u8]) -> Option<Vec<u8>> {
        let latest_key_update =
            self.pending_ancestors_app_state_updates
                .iter()
                .find(|app_state_updates_opt| {
                    if let Some(app_state_updates) = app_state_updates_opt {
                        app_state_updates.contains_delete(&key.to_vec())
                            || app_state_updates.get_insert(&key.to_vec()).is_some()
                    } else {
                        false
                    }
                });

        if let Some(Some(app_state_updates)) = latest_key_update {
            if app_state_updates.contains_delete(&key.to_vec()) {
                return None;
            } else if let Some(value) = app_state_updates.get_insert(&key.to_vec()) {
                return Some(value.clone());
            }
        }

        self.block_tree.committed_app_state(key)
    }

    pub fn validator_set(&self) -> Result<ValidatorSet, BlockTreeError> {
        self.block_tree.committed_validator_set()
    }
}
