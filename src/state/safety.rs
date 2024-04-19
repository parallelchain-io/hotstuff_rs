/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/


 /* ↓↓↓ Methods for growing the Block Tree ↓↓↓ */

use crate::{hotstuff::types::QuorumCertificate, types::{basic::ChainID, block::Block}};

use super::{block_tree::{BlockTree, BlockTreeError}, kv_store::KVStore};

    /// Returns whether a block can be safely inserted. For this, it is necessary that:
    /// 1. self.safe_qc(&block.justify).
    /// 2. No block with the same block hash is already in the block tree.
    /// 3. Its qc's must be either a generic qc or a commit qc.
    ///
    /// This function evaluates [Self::safe_qc], then checks 2 and 3.
    ///
    /// # Precondition
    /// [Block::is_correct]
    pub fn safe_block<K: KVStore>(block_tree: &BlockTree<K>, block: &Block, chain_id: ChainID) -> Result<bool, BlockTreeError> {
        Ok(
            /* 1 */
            safe_qc(block_tree, &block.justify, chain_id)? &&
            /* 2 */ !block_tree.contains(&block.hash)  &&
            /* 3 */ (block.justify.phase.is_generic() || block.justify.phase.is_commit())
        )
    }

    /// Returns whether a qc can be 'inserted' into the block tree, whether as part of a block using
    /// [BlockTree::insert_block], or to be set as the highest qc, or, if it is a precommit or commit qc,
    /// to have the view of its prepare qc set as the locked view.
    ///
    /// For this, it is necessary that:
    /// 1. Its chain ID matches the chain ID of the replica, or is the genesis qc.
    /// 2. It justifies a known block, or is the genesis qc.
    /// 3. Its view number is greater than or equal to locked view.
    /// 4. If it is a prepare, precommit, or commit qc, the block it justifies has pending validator state
    ///    updates.
    /// 5. If its qc is a generic qc, the block it justifies *does not* have pending validator set updates.
    ///
    /// # Precondition
    /// [QuorumCertificate::is_correct]
    pub fn safe_qc<K: KVStore>(block_tree: &BlockTree<K>, qc: &QuorumCertificate, chain_id: ChainID) -> Result<bool, BlockTreeError> {
        Ok(
            /* 1 */
            (qc.chain_id == chain_id || qc.is_genesis_qc()) &&
            /* 2 */
            (block_tree.contains(&qc.block) || qc.is_genesis_qc()) &&
            /* 3 */ qc.view >= block_tree.locked_qc()?.view &&
            /* 4 */ (((qc.phase.is_prepare() || qc.phase.is_precommit() || qc.phase.is_commit()) && block_tree.pending_validator_set_updates(&qc.block)?.is_some()) ||
            /* 5 */ (qc.phase.is_generic() && block_tree.pending_validator_set_updates(&qc.block)?.is_none()))
        )
    }