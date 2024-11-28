//! Special, read-only interface for querying the Block Tree used only by `App`s.

use crate::{
    hotstuff::types::PhaseCertificate,
    types::{
        block::Block,
        data_types::{BlockHeight, CryptoHash, Data, DataLen, Datum},
        update_sets::AppStateUpdates,
        validator_set::ValidatorSet,
    },
};

use super::super::pluggables::KVStore;

use super::internal::{BlockTreeError, BlockTreeSingleton};

/// View of the block tree made available to [method calls](crate::app::App#required-methods) on `App`s.
///
/// # Purpose
///
/// The view of the block tree that `AppBlockTreeView` provides is special in two important ways to the
/// correct implementation of the `App` trait:
/// 1. `AppBlockTreeView` presents the App State as it is at a specific "point in time", that is:
///     - Just after [`parent_block`](crate::app::App::produce_block) is executed in the case of
///       `produce_block`.
///     - Just before [`proposed_block`](crate::app::App::validate_block) is executed in the case of
///       `validate_block`.
/// 2. `AppBlockTreeView` does not implement all of the block tree getters available through, e.g.,
///    [`BlockTreeSnapshot`](super::block_tree_snapshot::BlockTreeSnapshot). Instead, `AppBlockTreeView`'s
///    getters are limited to those that get the fields of the block tree that should be consistent
///    across all replicas at a specific point in time. `App` methods can therefore safely depend on the
///    data in these fields without risking [non-determinism](crate::app::App#determinism-requirements).
///
/// # Constructor
///
/// To create an instance of `AppBlockTreeView`, use [`BlockTree::app_view`].
pub struct AppBlockTreeView<'a, K: KVStore> {
    // Reference to the block tree.
    pub(super) block_tree: &'a BlockTreeSingleton<K>,

    // The pending `AppStateUpdates` in the chain preceding the block this `AppBlockTreeView` is supposed
    // to see the block tree from the perspective of.
    pub(super) pending_ancestors_app_state_updates: Vec<Option<AppStateUpdates>>,
}

impl<'a, K: KVStore> AppBlockTreeView<'a, K> {
    /// Get `block`, if it is currently in the block tree.
    pub fn block(&self, block: &CryptoHash) -> Result<Option<Block>, BlockTreeError> {
        self.block_tree.block(block)
    }

    /// Get the height of `block`, if `block` is currently in the block tree.
    pub fn block_height(&self, block: &CryptoHash) -> Result<Option<BlockHeight>, BlockTreeError> {
        self.block_tree.block_height(block)
    }

    /// Get `block.justify`, if `block` is currently in the block tree.
    pub fn block_justify(&self, block: &CryptoHash) -> Result<PhaseCertificate, BlockTreeError> {
        self.block_tree.block_justify(block)
    }

    /// Get `block.data_hash`, if `block` is currently in the block tree.
    pub fn block_data_hash(
        &self,
        block: &CryptoHash,
    ) -> Result<Option<CryptoHash>, BlockTreeError> {
        self.block_tree.block_data_hash(block)
    }

    /// Get `block.data.len()`, if `block` is currently in the block tree.
    pub fn block_data_len(&self, block: &CryptoHash) -> Result<Option<DataLen>, BlockTreeError> {
        self.block_tree.block_data_len(block)
    }

    /// Get the whole of `block.data`, if `block` is currently in the block tree.
    pub fn block_data(&self, block: &CryptoHash) -> Result<Option<Data>, BlockTreeError> {
        self.block_tree.block_data(block)
    }

    /// Get one `block.data[datum_index]`, if `block` is currently in the block tree.
    pub fn block_datum(&self, block: &CryptoHash, datum_index: u32) -> Option<Datum> {
        self.block_tree.block_datum(block, datum_index)
    }

    /// Get the committed block at `height`, if it is currently in the block tree.
    pub fn block_at_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<CryptoHash>, BlockTreeError> {
        self.block_tree.block_at_height(height)
    }

    /// Get the value associated with `key` in the current app state.
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

    /// Get the current committed validator set.
    pub fn validator_set(&self) -> Result<ValidatorSet, BlockTreeError> {
        self.block_tree.committed_validator_set()
    }
}
