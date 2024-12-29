//! General purpose, read-only interface for querying the Block Tree.

use crate::{
    hotstuff::types::PhaseCertificate,
    pacemaker::types::TimeoutCertificate,
    types::{
        block::Block,
        data_types::{BlockHeight, ChildrenList, CryptoHash, Data, DataLen, Datum, ViewNumber},
        update_sets::AppStateUpdates,
        validator_set::{ValidatorSet, ValidatorSetState, ValidatorSetUpdatesStatus},
    },
};

use super::super::pluggables::{KVGet, KVStore};

use super::internal::BlockTreeError;

/// A factory for [`BlockTreeSnapshot`]s.
#[derive(Clone)]
pub struct BlockTreeCamera<K: KVStore>(K);

impl<K: KVStore> BlockTreeCamera<K> {
    pub fn new(kv_store: K) -> Self {
        BlockTreeCamera(kv_store)
    }

    pub fn snapshot(&self) -> BlockTreeSnapshot<K::Snapshot<'_>> {
        BlockTreeSnapshot(self.0.snapshot())
    }
}

/// A read-only view into the block tree that is guaranteed to stay unchanged.
pub struct BlockTreeSnapshot<S: KVGet>(pub(super) S);

impl<S: KVGet> BlockTreeSnapshot<S> {
    pub(crate) fn new(kv_snapshot: S) -> Self {
        BlockTreeSnapshot(kv_snapshot)
    }

    /* ↓↓↓ Used for syncing ↓↓↓ */

    /// Get a chain of blocks starting from the specified tail block and going towards the newest block,
    /// up until the limit.
    ///
    /// If tail is None, then the chain starts from genesis instead.
    pub(crate) fn blocks_from_height_to_newest(
        &self,
        height: BlockHeight,
        limit: u32,
    ) -> Result<Vec<Block>, BlockTreeError> {
        let mut res = Vec::with_capacity(limit as usize);

        // Get committed blocks starting from the specified height.
        let mut cursor = height;
        while let Some(block_hash) = self.0.block_at_height(cursor)? {
            res.push(self.0.block(&block_hash)?.ok_or(
                BlockTreeError::BlockExpectedButNotFound {
                    block: block_hash.clone(),
                },
            )?);
            cursor += 1;

            if res.len() == limit as usize {
                return Ok(res);
            }
        }

        // Get speculative blocks.
        let speculative_blocks = self.blocks_from_newest_to_committed()?.into_iter().rev();
        for block in speculative_blocks {
            res.push(block);

            if res.len() == limit as usize {
                break;
            }
        }

        Ok(res)
    }

    /// Get a chain of blocks from the newest block up to (but not including) the highest committed block,
    /// or genesis.
    ///
    /// The returned chain goes from blocks of higher height (newest block) to blocks of lower height.
    fn blocks_from_newest_to_committed(&self) -> Result<Vec<Block>, BlockTreeError> {
        let mut res = Vec::new();
        if let Some(newest_block) = self.0.newest_block()? {
            let mut cursor = newest_block;
            loop {
                let block =
                    self.0
                        .block(&cursor)?
                        .ok_or(BlockTreeError::BlockExpectedButNotFound {
                            block: cursor.clone(),
                        })?;
                let block_justify = block.justify.clone();
                res.push(block);

                if let Some(highest_committed_block) = self.0.highest_committed_block()? {
                    if block_justify.block == highest_committed_block {
                        break;
                    }
                }

                if block_justify == PhaseCertificate::genesis_pc() {
                    break;
                }

                cursor = block_justify.block;
            }
        }

        Ok(res)
    }

    pub(crate) fn highest_committed_block_height(
        &self,
    ) -> Result<Option<BlockHeight>, BlockTreeError> {
        let highest_committed_block = self.highest_committed_block()?;
        if let Some(block) = highest_committed_block {
            Ok(self.block_height(&block)?)
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ Basic state getters ↓↓↓ */

    pub fn block(&self, block: &CryptoHash) -> Result<Option<Block>, BlockTreeError> {
        Ok(self.0.block(block)?)
    }

    pub fn block_height(&self, block: &CryptoHash) -> Result<Option<BlockHeight>, BlockTreeError> {
        Ok(self.0.block_height(block)?)
    }

    pub fn block_data_hash(
        &self,
        block: &CryptoHash,
    ) -> Result<Option<CryptoHash>, BlockTreeError> {
        Ok(self.0.block_data_hash(block)?)
    }

    pub fn block_justify(&self, block: &CryptoHash) -> Result<PhaseCertificate, BlockTreeError> {
        Ok(self.0.block_justify(block)?)
    }

    pub fn block_data_len(&self, block: &CryptoHash) -> Result<Option<DataLen>, BlockTreeError> {
        Ok(self.0.block_data_len(block)?)
    }

    pub fn block_data(&self, block: &CryptoHash) -> Result<Option<Data>, BlockTreeError> {
        Ok(self.0.block_data(block)?)
    }

    pub fn block_datum(&self, block: &CryptoHash, datum_index: u32) -> Option<Datum> {
        self.0.block_datum(block, datum_index)
    }

    pub fn block_at_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<CryptoHash>, BlockTreeError> {
        Ok(self.0.block_at_height(height)?)
    }

    pub fn children(&self, block: &CryptoHash) -> Result<ChildrenList, BlockTreeError> {
        Ok(self.0.children(block)?)
    }

    pub fn committed_app_state(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.committed_app_state(key)
    }

    pub fn pending_app_state_updates(
        &self,
        block: &CryptoHash,
    ) -> Result<Option<AppStateUpdates>, BlockTreeError> {
        Ok(self.0.pending_app_state_updates(block)?)
    }

    pub fn committed_validator_set(&self) -> Result<ValidatorSet, BlockTreeError> {
        Ok(self.0.committed_validator_set()?)
    }

    pub fn validator_set_updates_status(
        &self,
        block: &CryptoHash,
    ) -> Result<ValidatorSetUpdatesStatus, BlockTreeError> {
        Ok(self.0.validator_set_updates_status(block)?)
    }

    pub fn locked_pc(&self) -> Result<PhaseCertificate, BlockTreeError> {
        Ok(self.0.locked_pc()?)
    }

    pub fn highest_view_entered(&self) -> Result<ViewNumber, BlockTreeError> {
        Ok(self.0.highest_view_entered()?)
    }

    pub fn highest_pc(&self) -> Result<PhaseCertificate, BlockTreeError> {
        Ok(self.0.highest_pc()?)
    }

    pub fn highest_committed_block(&self) -> Result<Option<CryptoHash>, BlockTreeError> {
        Ok(self.0.highest_committed_block()?)
    }

    pub fn newest_block(&self) -> Result<Option<CryptoHash>, BlockTreeError> {
        Ok(self.0.newest_block()?)
    }

    pub fn highest_tc(&self) -> Result<Option<TimeoutCertificate>, BlockTreeError> {
        Ok(self.0.highest_tc()?)
    }

    pub fn previous_validator_set(&self) -> Result<ValidatorSet, BlockTreeError> {
        Ok(self.0.previous_validator_set()?)
    }

    pub fn validator_set_update_block_height(&self) -> Result<Option<BlockHeight>, BlockTreeError> {
        Ok(self.0.validator_set_update_block_height()?)
    }

    pub fn validator_set_update_complete(&self) -> Result<bool, BlockTreeError> {
        Ok(self.0.validator_set_update_complete()?)
    }

    pub fn validator_set_state(&self) -> Result<ValidatorSetState, BlockTreeError> {
        Ok(self.0.validator_set_state()?)
    }

    pub fn highest_view_voted(&self) -> Result<Option<ViewNumber>, BlockTreeError> {
        Ok(self.0.highest_view_phase_voted()?)
    }
}
