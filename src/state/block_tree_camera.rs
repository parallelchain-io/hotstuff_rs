/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use crate::hotstuff::types::QuorumCertificate;
use crate::types::{basic::BlockHeight, block::Block};

use super::block_tree::BlockTreeError;
use super::kv_store::{KVGet, KVGetError, KVStore};

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

/// A read view into the block tree that is guaranteed to stay unchanged.
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
            res.push(self.0.block(&block_hash)?.ok_or(BlockTreeError::BlockExpectedButNotFound{block: block_hash.clone()})?);
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
                let block = self.0.block(&cursor)?.ok_or(BlockTreeError::BlockExpectedButNotFound{block: cursor.clone()})?;
                let block_justify = block.justify.clone();
                res.push(block);

                if let Some(highest_committed_block) = self.0.highest_committed_block()? {
                    if block_justify.block == highest_committed_block {
                        break;
                    }
                }

                if block_justify == QuorumCertificate::genesis_qc() {
                    break;
                }

                cursor = block_justify.block;
            }
        }

        Ok(res)
    }
}
