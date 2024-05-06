/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! This module contains implementations of rules that collectively guarantee the safety of hotstuff-rs.
//! 
//! In hotstuff-rs, the key events that can trigger state updates are:
//! 1. Receiving a block proposal,
//! 2. Receiving a nudge.
//! 
//! If a block or a nudge is considered safe, then it is safe for it to trigger state updates inside the
//! [BlockTree]. This generally requires that the block or nudge is well-formed, and that the quroum
//! certificate that it references is cryptographically correct and safe. However, since nudges
//! implement a version of the non-pipelined HotStuff consensus for validator-set-updating blocks, a
//! non-decide-phase nudge is only safe if it is broadcasted in the next view relative to the view in
//! which its justify is collected. 
//! 
//! While [safe_block], [safe_nudge], and [safe_qc] define what it means for a block, nudge, or QC,
//! to be safe, and hence be allowed to trigger state updates, [qc_to_lock] and [block_to_commit]
//! can be used to determine which updates can be made on seeing a safe nudge or a safe block.
//! This depends on the commit and locking rules.
//! 
//! ## Safe QC
//! 
//! A safe QC must match the chain id of the replica (to avoid having state updates triggered by a
//! QC from another blockchain). It must also have an appropriate phase depending on whether the
//! block it justifies has associated validator set updates or not, and it must be for a known
//! block, i.e., a block that is already stored in the BlockTree.
//! 
//! Most importantly, it must either:
//! 1. Reference a block that extends the branch of the block referenced by the locked QC, or
//! 2. Have a higher view than the locked QC.
//! 
//! The former condition guarantees safety in case a quorum of replicas is locked on a given block,
//! while the latter ensures liveness in case the quorum has moved to a conflicting branch and
//! there exists evidence for it. The evidence would be a newely received QC with a view higher than the
//! view of the replica's locked QC.
//! 
//! ## Commit rules
//! 
//! To avoid inconsistent state due to Byzantine leaders causing safety-threatening branch switches, the
//! pipelined HotStuff protocol requires that in order to be committed a block must be followed by a
//! 3-chain. Concretely, the three QCs referenced by its child, grandchild, and great-grandchild, 
//! serving as its prepareQC, precommitQC, and commitQC respectively, must have consecutive views.
//! [block_to_commit] enforces this rule for the pipelined protocol for non-validator-set-updating
//! blocks. It is not required in [safe_block] since blocks can be safely added even if their 
//! ancestors don't have consecutive views - but commit of the branch will only be triggered once
//! there is a 3-chain.
//! 
//! To obtain an equivalent guarantee for the non-pipelined protocol for validator-set-updating blocks,
//! we require that the prepareQC, precommitQC, and commitQC for a validator-set-updating block received
//! via nudges have consecutive views. If this flow is interrupted by a temporary loss of synchrony or
//! a Byzantine leader, the validator-set-updating block has to be re-proposed, as specified in
//! [repropose_block]. This rule is enforced through [safe_nudge]. Unlike block insertions, nudges
//! cannot cause state updates unless they satisfy the rule.
//! 
//! ## Locking rules
//! 
//! Locking servers to guarantee the safety of commit. In SMR protocols that follow the lock-commit
//! paradigm, a value has to be locked before it is committed. The idea is that even if only some
//! replicas commit, the others hold a lock to the committed value and should eventually commit it.
//! 
//! The locked value is essentially a block, but we consider any QC that references the block
//! to be valid proxy for it, and hence store a lockedQC instead. This enables keeping track
//! of information on the view in which a quorum of replicas last voted for this block.
//! 
//! As mentioned above, if a block is locked, then any new proposal or nudge must "extend" it,
//! unless there exists evidence that a quorum of replicas do not share this lock and have
//! moved to an alternative branch.
//! 
//! In the pipelined HotStuff protocol, a block is locked if there is a 2-chain supporting it, i.e., 
//! there is a QC for the block and a QC for its child, which serve as the block's prepare QC and
//! precommit QC respectively. In such case it is the prepare QC that is locked. To adapt this rule to
//! the non-pipelined protocol for validator-set-updating blocks, we lock the block on seeing a nudge
//! with a precommitQC for it - it is the precommitQC that becomes locked this time, as by the 
//! consecutive views requirement this is equivalent to locking the prepareQC. Since the block sync 
//! protocol does not involve sending nudges, we must also lock the decideQC when referenced by the
//! block. This is to ensure that subsequently received blocks are not accepted if they conflict with 
//! it. This is enforced through [qc_to_lock].

use crate::hotstuff::{
    messages::Nudge, 
    types::{Phase, QuorumCertificate}
};
use crate::types::{
    basic::{ChainID, CryptoHash, ViewNumber}, 
    block::Block
};

use super::{
    block_tree::{BlockTree, BlockTreeError},
    kv_store::KVStore
};

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
        /* 2 */ block.justify.is_block_justify()
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
/// [QuorumCertificate::is_correct] holds for block.justify.
pub fn safe_qc<K: KVStore>(qc: &QuorumCertificate, block_tree: &BlockTree<K>, chain_id: ChainID) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */ (qc.chain_id == chain_id || qc.is_genesis_qc()) &&
        /* 2 */ (block_tree.contains(&qc.block) || qc.is_genesis_qc()) &&
        /* 3 */ (qc.view > block_tree.locked_qc()?.view || extends_locked_qc_block(qc, block_tree)?) &&
        /* 4 */ (((qc.phase.is_prepare() || qc.phase.is_precommit() || qc.phase.is_commit() || qc.phase.is_decide()) && 
                   block_tree.validator_set_updates_status(&qc.block)?.contains_updates()) ||
        /* 5 */ (qc.phase.is_generic() && !block_tree.validator_set_updates_status(&qc.block)?.contains_updates()))
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
/// 
/// # Precondition
/// [QuorumCertificate::is_correct] holds for nudge.justify.
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
/// 
/// # Precondition
/// The block or nudge with this justify must satisfy [safe_block] or [safe_nudge] respectively.
pub(crate) fn qc_to_lock<K: KVStore>(
    justify: &QuorumCertificate,
    block_tree: &BlockTree<K>) 
    -> Result<Option<QuorumCertificate>, BlockTreeError> 
{   
    if justify.is_genesis_qc() {
        return Ok(None)
    }

    let locked_qc = block_tree.locked_qc()?;
    let new_locked_qc = match justify.phase {
        Phase::Decide | Phase::Precommit => {
            if justify != &locked_qc {
                Some(justify.clone())
            } else {
                None
            }
        },
        Phase::Generic => {
            let parent_justify = block_tree.block_justify(&justify.block)?;
            if parent_justify != locked_qc {
                Some(parent_justify.clone())
            } else {
                None
            }
        },
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
/// blocks by requiring that the three quorum certificates following a block to be committed have 
/// consecutive views.
/// 
/// This method implements the following rule:
/// 1. If a commit or decide qc is seen, then the qc's block should be committed - if not committed yet. 
/// 2. If a generic qc is seen, then the great-grandparent block, i.e., the block referenced by the generic
///    qc's grandparent qc, should be committed - if not committed yet and if the commit rule holds.
/// 
/// Note:
/// - We do not need to consider the blocks referenced by the parent and grandparent qc and check if they
///   are decide qc, since if so, they would have been committed on inserting the parent or grandparent
///   blocks respectively.
/// - Even though validator-set-updating blocks are usually committed on seeing a commit qc,
///   when the missing blocks are received via sync the commit qc is not sent, but rather the block has to
///   be committed on seeing a decide qc.
/// 
/// # Precondition
/// The block or nudge with this justify must satisfy [safe_block] or [safe_nudge] respectively.
pub(crate) fn block_to_commit<K: KVStore>(
    justify: &QuorumCertificate, 
    block_tree: &BlockTree<K>) 
    -> Result<Option<CryptoHash>, BlockTreeError> 
{
    if justify.is_genesis_qc() {
        return Ok(None)
    };

    match justify.phase {
        Phase::Commit | Phase::Decide => {
            let commit_block_height = 
                block_tree.block_height(&justify.block)?
                .ok_or(BlockTreeError::BlockExpectedButNotFound{block: justify.block.clone()})?;

            let highest_committed_block_height = block_tree.highest_committed_block_height()?;

            let not_committed_yet = highest_committed_block_height.is_none() || commit_block_height > highest_committed_block_height.unwrap();

            if not_committed_yet {
                Ok(Some(justify.block))
            } else {
                Ok(None)
            }      
        },
        Phase::Generic => {
            let parent_justify = block_tree.block_justify(&justify.block)?;
            if parent_justify.is_genesis_qc() {return Ok(None)};

            let grandparent_justify = block_tree.block_justify(&parent_justify.block)?;
            if grandparent_justify.is_genesis_qc() {return Ok(None)};

            let commit_rule_satisfied = 
                justify.view == parent_justify.view + 1 && parent_justify.view == grandparent_justify.view + 1;

            let commit_block_height = 
                block_tree.block_height(&grandparent_justify.block)?
                .ok_or(BlockTreeError::BlockExpectedButNotFound{block: grandparent_justify.block.clone()})?;

            let highest_committed_block_height = block_tree.highest_committed_block_height()?;

            let not_committed_yet = highest_committed_block_height.is_none() || commit_block_height > highest_committed_block_height.unwrap();

            if commit_rule_satisfied && not_committed_yet {
                Ok(Some(grandparent_justify.block))
            } else {
                Ok(None)
            }

        },
        _ => Ok(None)
    }
}

/// Returns whether a block needs to be re-proposed or not, and if yes then which block. This method
/// should be called by a leader to determine whether a block should be re-proposed or whether a 
/// proposal or nudge referencing the highest qc should be broadcasted. The former may happen in case
/// the sequence of nudges for a validator-set-updating block has been interrupted and the commit rule
/// cannot be satisfied. In such case proposing or nudging on top of the highest_qc is doomed to fail, 
/// and instead the block that the highest_qc points to should be re-proposed.
pub(crate) fn repropose_block<K: KVStore>(
    cur_view: ViewNumber, 
    block_tree: &BlockTree<K>) 
    -> Result<Option<CryptoHash>, BlockTreeError> 
{
    let highest_qc = block_tree.highest_qc()?;
    match highest_qc.phase {
        Phase::Prepare | Phase::Precommit if cur_view != highest_qc.view + 1 => {
            Ok(Some(highest_qc.block))
        },
        _ => Ok(None)
    }
}

/// Returns whether a given quorum certificate belongs to the branch that extends from the locked block.
/// Since in pipelined HotStuff it is always the grandparent of the newest block that is locked, we only
/// need to check the qc's block, its parent, and its grandparent.
fn extends_locked_qc_block<K: KVStore>(qc: &QuorumCertificate, block_tree: &BlockTree<K>) -> Result<bool, BlockTreeError> {
    let locked_qc = block_tree.locked_qc()?;
    let block = qc.block;
    let block_parent = block_tree.block_justify(&block).ok().map(|qc| qc.block);
    let block_grandparent = block_parent.map(|b| block_tree.block_justify(&b).ok().map(|qc| qc.block)).flatten();
    Ok(
        block == locked_qc.block ||
        block_parent.is_some_and(|b| b == locked_qc.block) ||
        block_grandparent.is_some_and(|b| b == locked_qc.block)
    )
}