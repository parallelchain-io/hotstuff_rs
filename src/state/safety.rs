/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! This module contains implementations of rules that collectively guarantee the safety of hotstuff-rs.
//! TODO: document the rules
//! 
//! ## Safe QC
//! 
//! ## Commit rules
//! 
//! ## Locking rules
//! 

use crate::{hotstuff::{messages::Nudge, types::{Phase, QuorumCertificate}}, types::{basic::{ChainID, CryptoHash, ViewNumber}, block::Block}};

use super::{block_tree::{BlockTree, BlockTreeError}, kv_store::KVStore};

/// Returns whether a block can be considered safe, and thus cause updates to the block tree. 
/// For this, it is necessary that:
/// 1. safe_qc(&block.justify, block_tree, chain_id).
/// 2. Its qc's must be either a generic qc or a decide qc.
/// 
/// Note that before inserting to the block tree, the caller should also check if
/// no block with the same block hash is already in the block tree.
///
/// This function evaluates [safe_qc], then checks 2.
///
/// # Precondition
/// [Block::is_correct]
pub fn safe_block<K: KVStore>(block: &Block, block_tree: &BlockTree<K>, chain_id: ChainID) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */ safe_qc(&block.justify, block_tree, chain_id)? &&
        /* 2 */ (block.justify.is_block_justify())
    )
}

/// Returns whether a qc can cause updates of the block tree, whether as part of a block using
/// [BlockTree::insert_block], or to be set as the highest qc, or, if it is a precommit or commit qc,
/// to have the view of its prepare qc set as the locked view.
///
/// For this, it is necessary that:
/// 1. Its chain ID matches the chain ID of the replica, or is the genesis qc.
/// 2. It justifies a known block, or is the genesis qc.
/// 3. Its view number is greater than the locked qc view or its block extends from the locked block.
/// 4. If it is a prepare, precommit, or commit qc, the block it justifies has pending validator state
///    updates.
/// 5. If its qc is a generic qc, the block it justifies *does not* have pending validator set updates.
///
/// # Precondition
/// [QuorumCertificate::is_correct]
pub fn safe_qc<K: KVStore>(qc: &QuorumCertificate, block_tree: &BlockTree<K>, chain_id: ChainID) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */ (qc.chain_id == chain_id || qc.is_genesis_qc()) &&
        /* 2 */ (block_tree.contains(&qc.block) || qc.is_genesis_qc()) &&
        /* 3 */ (qc.view > block_tree.locked_qc()?.view || extends_locked_qc_block(qc, block_tree)?) &&
        /* 4 */ (((qc.phase.is_prepare() || qc.phase.is_precommit() || qc.phase.is_commit() || qc.phase.is_decide()) && block_tree.block_validator_set_updates(&qc.block)?.is_some()) ||
        /* 5 */ (qc.phase.is_generic() && block_tree.block_validator_set_updates(&qc.block)?.is_none()))
    )
}

/// Returns whether a nudge can be considered safe, and hence cause updates to the block tree.
/// For this, it is necessary that:
/// 1. safe_qc(&nudge.justify, block_tree, chain_id).
/// 2. nudge.justify is a prepare, precommit, or commit qc.
/// 3. Its chain ID matches the chain ID of the replica.
/// 4. &nudge.justify is either a commit qc, or &nudge.justify.view = cur_view - 1.
/// 
/// This method enforces the commit rule for validator-set-updating blocks.
pub fn safe_nudge<K: KVStore>(nudge: &Nudge, cur_view: ViewNumber, block_tree: &BlockTree<K>, chain_id: ChainID) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */ safe_qc(&nudge.justify, block_tree, chain_id)? &&
        /* 2 */ nudge.justify.is_nudge_justify() &&
        /* 3 */ (nudge.chain_id == chain_id) &&
        /* 4 */ (nudge.justify.phase.is_commit() || nudge.justify.view == cur_view - 1)
    )
}

/// Returns an optional qc that should be set as the new locked qc after seeing a given justify. This
/// should be called whenever a new, correct and safe justify qc is seen, whether through a block
/// proposal or through a nudge. This method implements the following rule:
/// 1. If a block justify is seen,
///     - if it is a decide qc, then locked qc should be updated to the block justify.
///     - else the locked qc should be updated to the parent qc of the block justify.
/// 2. If a nudge justify is seen,
///     - if it is a precommit qc, then locked qc should be updated to the precommit qc.
///     - else the locked qc should not be updated.
pub fn qc_to_lock<K: KVStore>(justify: &QuorumCertificate, block_tree: &BlockTree<K>) -> Result<Option<QuorumCertificate>, BlockTreeError> {
    let locked_qc = block_tree.locked_qc()?;
    let new_locked_qc = match justify.phase {
        Phase::Decide => {
            Some(justify.clone())
        },
        Phase::Generic => {
            let parent_qc = block_tree.block_justify(&justify.block)?;
            Some(parent_qc)
        },
        Phase::Precommit => Some(justify.clone()),
        _ => None
    };
    if new_locked_qc.as_ref().is_some_and(|qc| qc != &locked_qc) {
        Ok(new_locked_qc)
    } else { 
        Ok(None) 
    }
}

/// Returns an optional block that should be committed (along with its uncommitted predecessors) after seeing
/// a given justify. The method should be called whenever a new, correct and safe justify qc is seen, whether 
/// through a block proposal or through a nudge. It enforces the commit rule for non-validator-set-updating 
/// blocks.
pub fn block_to_commit<K: KVStore>(justify: &QuorumCertificate, block_tree: &BlockTree<K>) -> Option<CryptoHash> {
    todo!()
}

/// Returns whether a given quorum certificate belongs to the branch that extends from the locked block.
/// Since in pipelined HotStuff it is always the grandparent of the newest block that is locked, we only
/// need to check the qc's block, its parent, and its grandparent.
fn extends_locked_qc_block<K: KVStore>(qc: &QuorumCertificate, block_tree: &BlockTree<K>) -> Result<bool, BlockTreeError> {
    let locked_qc = block_tree.locked_qc()?;
    Ok(
        qc.block == locked_qc.block ||
        block_tree.block_justify(&qc.block)?.block == locked_qc.block ||
        block_tree.block_justify(&block_tree.block_justify(&qc.block)?.block)?.block == locked_qc.block
    )
}