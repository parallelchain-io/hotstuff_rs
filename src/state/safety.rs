/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Rules and predicates that collectively guarantee the safety of HotStuff-rs SMR.
//!
//! ## Safety and state updates
//!
//! In HotStuff-rs, the key events that can trigger state updates are:
//! 1. Receiving a [`Proposal`](crate::hotstuff::messages::Proposal).
//! 2. Receiving a [`Nudge`].
//!
//! A `Proposal` or `Nudge` is considered **safe** if it is safe for it to trigger state updates inside
//! the [`BlockTree`]. This generally requires that the block or nudge is well-formed, and that the
//! quorum certificate that it references is cryptographically correct and safe. In addition, since
//! nudges implement a version of the "phased" HotStuff consensus for validator-set-updating blocks, a
//! non-Decide-phase nudge is only safe if it is broadcasted in the next view relative to the view in
//! which its justify is collected.
//!
//! This module defines two primary sets of functions, each implementing a specific aspect of safety and
//! state updates:
//!
//! The crate-public functions that this module defines fall into two main categories:
//! 1. [`safe_block`], [`safe_nudge`], and [`safe_qc`] checks **whether** a Block, Nudge, or QC is safe,
//!    respectively. One extra function, [`repropose_block`], is related to `safe_nudge`.
//! 2. [`qc_to_lock`], and [`block_to_commit`] determine **what** state updates should be made upon seeing
//!    a safe Block or a safe Nudge.
//!
//! ## Safe QC
//!
//! A safe QC must match the Chain ID of the replica (to avoid having state updates triggered by a
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
//! there exists evidence for it. The evidence would be a newly received QC with a view higher than the
//! view of the replica's locked QC.
//!
//! ## Commit Rules
//!
//! To prevent different replicas from entering inconsistent states due to Byzantine leaders causing
//! safety-threatening branch switches, the pipelined HotStuff protocol requires that a block must be
//! followed by a "3-chain" in order to be committed. This means that the block must be followed by
//! a child, a grandchild, and a great-grandchild block.
//!
//! In addition, the three QCs contained in its child, grandchild, and great-grandchild blocks, serving
//! as its "virtual" Prepare QC, Precommit QC, and Commit QC respectively, must have **consecutive
//! views**. Note however that while consecutive views are required for a block to be safely committed,
//! they are not required for a block to be inserted to the block tree, and therefore `safe_block` does
//! not enforce it. Consecutive views are also not required for a block to be locked.
//!
//! To obtain an equivalent guarantee for the phased HotStuff protocol for validator-set-updating
//! blocks, we require that the Prepare QC, Precommit QC, and Commit QC for a validator-set-updating
//! block received via nudges have consecutive views. If this flow is interrupted by a temporary loss of
//! synchrony or a Byzantine leader, the validator-set-updating block has to be re-proposed, as
//! specified in [`repropose_block`]. This rule is enforced through [`safe_nudge`]. Unlike block
//! insertions, nudges cannot cause state updates unless they satisfy the consecutive views rule.
//!
//! ## Locking Rules
//!
//! Locking serves to guarantee the safety of commit. In SMR protocols that follow the lock-commit
//! paradigm, a value has to be locked before it is committed. If a block is locked, then any new
//! proposal or nudge must "extend" it unless there exists evidence that a quorum of replicas do not
//! share this lock and have moved to an alternative branch. The idea is that even if only some replicas
//! commit, the others hold a lock to the committed value and should eventually commit it.
//!
//! The locked value is essentially a block, but we consider any QC that references the block to be
//! valid proxy for it, and hence store a locked QC instead. This enables keeping track of information on
//! the view in which a quorum of replicas last voted for this block.
//!
//! In the pipelined HotStuff protocol, a block is locked if there is a 2-chain supporting it, i.e.,
//! there is a QC for the block and a QC for its child, which serve as the block's prepare QC and
//! precommit QC respectively. In such case it is the prepare QC that is locked.
//!
//! To adapt this rule to the phased HotStuff protocol for validator-set-updating blocks, we lock
//! the block on seeing a nudge with a precommit QC for it - it is the precommit QC that becomes locked
//! this time, as by the consecutive views requirement this is equivalent to locking the prepareQC.
//! Since the block sync protocol does not involve sending nudges, we must also lock the decideQC when
//! referenced by the block. This is to ensure that subsequently received blocks are not accepted if
//! they conflict with it.

use crate::hotstuff::{
    messages::Nudge,
    types::{Phase, QuorumCertificate},
};
use crate::types::{
    basic::{ChainID, CryptoHash, ViewNumber},
    block::Block,
};

use super::{
    block_tree::{BlockTree, BlockTreeError},
    kv_store::KVStore,
};

/// Check whether `block` can safely cause updates to the block tree.
///
/// ## Conditional checks
///
/// `safe_block` returns `true` in case all of the following predicates are `true`:
/// 1. `safe_qc(&block.justify, block_tree, chain_id)`.
/// 2. `block.qc` is either a generic qc or a decide qc.
///
/// ## Precondition
///
/// [`is_correct`](Block::is_correct) is `true` for `block`.
pub(crate) fn safe_block<K: KVStore>(
    block: &Block,
    block_tree: &BlockTree<K>,
    chain_id: ChainID,
) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */
        safe_qc(&block.justify, block_tree, chain_id)? &&
        /* 2 */ block.justify.is_block_justify(),
    )
}

/// Check whether a QC can safely cause updates to the block tree, whether as part of a block through
/// [`insert_block`](BlockTree::insert_block), or through being set as the Highest QC, or, if it is a
/// Precommit or Commit QC, through being set as the Locked QC.
///
/// ## Conditional checks
///
/// `safe_qc` returns `true` in case all of the following predicates are `true`:
/// 1. `qc.chain_id` either matches the chain ID of the replica, or `qc` is the genesis qc.
/// 2. Either `qc.block` is in the block tree, or `qc` is the Genesis QC.
/// 3. Either `qc`'s view number is greater than the Locked QC's view, or its block extends from the locked
///    block.
/// 4. If `qc` is a Prepare, Precommit, or Commit QC, the block it justifies has pending validator state
///    updates.
/// 5. If `qc` is a Generic qc, the block it justifies *does not* have pending validator set updates.
///
/// ## Precondition
///
/// [`is_correct`](crate::types::collectors::Certificate::is_correct) is `true` for `block.justify`.
pub(crate) fn safe_qc<K: KVStore>(
    qc: &QuorumCertificate,
    block_tree: &BlockTree<K>,
    chain_id: ChainID,
) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */
        (qc.chain_id == chain_id || qc.is_genesis_qc()) &&
        /* 2 */ (block_tree.contains(&qc.block) || qc.is_genesis_qc()) &&
        /* 3 */ (qc.view > block_tree.locked_qc()?.view || extends_locked_qc_block(qc, block_tree)?) &&
        /* 4 */ (((qc.phase.is_prepare() || qc.phase.is_precommit() || qc.phase.is_commit() || qc.phase.is_decide()) &&
                   block_tree.validator_set_updates_status(&qc.block)?.contains_updates()) ||
        /* 5 */ (qc.phase.is_generic() && !block_tree.validator_set_updates_status(&qc.block)?.contains_updates())),
    )
}

/// Check whether a `Nudge` can safely cause updates to the Block Tree. This method enforces the
/// commit rule for validator-set-updating blocks.
///
/// ## Conditional checks
///
/// `safe_nudge` returns `true` in case all of the following predicates are `true`:
/// 1. `safe_qc(&nudge.justify, block_tree, chain_id)`.
/// 2. `nudge.justify` is a Prepare, Precommit, or Commit qc.
/// 3. `nudge.chain_id` matches the Chain ID configured for the replica.
/// 4. `nudge.justify` is either a Commit QC, or `nudge.justify.view = cur_view - 1`.
///
/// ## Precondition
///
/// [`is_correct`](crate::types::collectors::Certificate::is_correct) is `true` for `nudge.justify`.
pub fn safe_nudge<K: KVStore>(
    nudge: &Nudge,
    cur_view: ViewNumber,
    block_tree: &BlockTree<K>,
    chain_id: ChainID,
) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */
        safe_qc(&nudge.justify, block_tree, chain_id)? &&
        /* 2 */ nudge.justify.is_nudge_justify() &&
        /* 3 */ (nudge.chain_id == chain_id) &&
        /* 4 */ (nudge.justify.phase.is_commit() || nudge.justify.view == cur_view - 1),
    )
}

/// Get the QC (if any) that should be set as the Locked QC after the replica sees the given `justify`.
///
/// Like [`block_to_commit`], this function should be called whenever a new, correct, and
/// [safe](`safe_qc`) `justify` QC is seen, whether received in a `Proposal`, or a `Nudge`.
///
/// ## Precondition
///
/// The block or nudge containing this justify must satisfy [`safe_block`] or [`safe_nudge`]
/// respectively.
///
/// ## Lower-level Locking Rules
///
/// This function implements the "high-level" [Locking Rules](super::safety#locking-rules) by evaluating
/// the following "lower-level" Locking Rules:
///
/// |`justify` is a|QC to lock|Reasoning|
/// |---|---|---|
/// |`Generic` QC|`Some(justify.block.justify)`|In the pipelined mode of the HotStuff subprotocol, Generic `justify`s serve as the Precommit QC for the parent of `justify.block`.|
/// |`Prepare` QC|`None`|This is for the sake of consistency with the decision not to commit ancestor blocks on seeing a Prepare QC or a Precommit QC in [`block_to_commit`] ([discussion on GitHub](https://github.com/parallelchain-io/hotstuff_rs/pull/36#discussion_r1598193117)).|
/// |`Precommit` QC|`Some(justify)`|This is equivalent to locking the Prepare QC for the same block on seeing the Precommit QC, as is done in the original HotStuff algorithm, since by the `safe_nudge` precondition, Prepare QC and Precommit QC must have consecutive views.|
/// |`Commit` QC or `Decide` QC|If `justify.block == locked_qc.block`, then `None`, else `Some(justify)`|If `justify` is a Commit or Decide QC, then under "normal" conditions `justify.block` should have already been locked on seeing a Precommit QC for the block, and hence there is no need to lock. <br/><br/> However, in case the current locked QC is not for `justify.block` (this can happen if the replica didn't receive the Precommit QC for the block, for example if the replica is receiving the Decide QC `justify` via block sync), we should still lock the `justify`. This is equivalent to locking the Precommit QC, as we do under normal operation, since the Precommit QC and the Commit QC/Decide QC have consecutive views and are for the same block.|
///
/// ### Preventing redundant locking
///
/// If the QC to lock as determined by the rules described in the table above is equal to the current
/// `locked_qc`, then `qc_to_lock` will return `None` so that the caller does not redundantly lock on
/// the same QC again.
pub(crate) fn qc_to_lock<K: KVStore>(
    justify: &QuorumCertificate,
    block_tree: &BlockTree<K>,
) -> Result<Option<QuorumCertificate>, BlockTreeError> {
    // Special case: if `justify` is the Genesis QC, there is no QC to lock.
    if justify.is_genesis_qc() {
        return Ok(None);
    }

    // Determine the "QC to lock" according to the Lower-level Locking Rules.
    let locked_qc = block_tree.locked_qc()?;
    let new_locked_qc = match justify.phase {
        Phase::Generic => {
            let parent_justify = block_tree.block_justify(&justify.block)?;
            Some(parent_justify.clone())
        }
        Phase::Prepare => None,
        Phase::Precommit => Some(justify.clone()),
        Phase::Commit | Phase::Decide => {
            if locked_qc.block != justify.block {
                Some(justify.clone())
            } else {
                None
            }
        }
    };

    // Prevent redundant locking by returning Some only if the QC to lock is not already the locked QC.
    if new_locked_qc.as_ref().is_some_and(|qc| qc != &locked_qc) {
        Ok(new_locked_qc)
    } else {
        Ok(None)
    }
}

/// Get the Block (if any) that, along with its uncommitted predecessors, should be committed after the
/// replica sees the given `justify`.
///
/// Like [`qc_to_lock`], this function should be called whenever a new, correct, and safe `justify` QC
/// is seen, whether received in a `Proposal`, or a `Nudge`.
///
/// ## Preconditions
///
/// The block or nudge containing `justify` must satisfy [`safe_block`] or [`safe_nudge`] respectively.
///
/// ## Lower-level Commit Rules
///
/// This function implements the "high-level" [Commit Rules](super::safety#commit-rules) by evaluating
/// the following "lower-level" Commit Rules:
///
/// |`justify` is a|Block to commit|Reasoning|
/// |---|---|---|
/// |`Generic` QC|If the "consecutive views" requirement is met, then `Some(great_grandparent_block)`, else, `None`|Implements the commit rule for non-validator-set-updating blocks.|
/// |`Prepare` QC or `Precommit` QC|`None`|Theoretically, we could commit the `grandparent_block` on receiving a `Prepare` QC and the `parent_block` on receiving `Precommit` QC, but we choose not to do so.|
/// |`Commit` QC|`Some(justify.block)`|Implements the commit rule for validator-set-updating blocks.|
/// |`Decide` QC|`Some(justify.block)`|Validator-set-updating blocks are usually committed on seeing `Commit` QC. However, during sync `Commit` QCs are not sent, so blocks have to be committed on seeing `Decide` QCs instead.|
///
/// ### Preventing redundant commits
///
/// If the block to commit as determined by the rules described in the table above has already been
/// committed, then `block_to_commit` will return `None` so that the caller does not commit the same
/// block multiple times.
pub(crate) fn block_to_commit<K: KVStore>(
    justify: &QuorumCertificate,
    block_tree: &BlockTree<K>,
) -> Result<Option<CryptoHash>, BlockTreeError> {
    // Special case: if `justify` is the Genesis QC, there is no block to commit.
    if justify.is_genesis_qc() {
        return Ok(None);
    };

    match justify.phase {
        Phase::Generic => {
            let parent_justify = block_tree.block_justify(&justify.block)?;
            if parent_justify.is_genesis_qc() {
                return Ok(None);
            };

            let grandparent_justify = block_tree.block_justify(&parent_justify.block)?;
            if grandparent_justify.is_genesis_qc() {
                return Ok(None);
            };

            // Check the "consecutive views" requirement of the commit rule.
            let commit_rule_satisfied = justify.view == parent_justify.view + 1
                && parent_justify.view == grandparent_justify.view + 1;

            // Determine whether the great-grandparent block has been committed already.
            let commit_block_height = block_tree.block_height(&grandparent_justify.block)?.ok_or(
                BlockTreeError::BlockExpectedButNotFound {
                    block: grandparent_justify.block.clone(),
                },
            )?;
            let highest_committed_block_height = block_tree.highest_committed_block_height()?;
            let not_committed_yet = highest_committed_block_height.is_none()
                || commit_block_height > highest_committed_block_height.unwrap();

            // If the commit rule is satisfied and the great-grandparent block has not been committed yet, ask the
            // caller to commit it. Otherwise, return `None` to prevent redundant commits.
            if commit_rule_satisfied && not_committed_yet {
                Ok(Some(grandparent_justify.block))
            } else {
                Ok(None)
            }
        }
        Phase::Prepare | Phase::Precommit => Ok(None),
        Phase::Commit | Phase::Decide => {
            // Determine whether `justify.block` has been committed already.
            let commit_block_height = block_tree.block_height(&justify.block)?.ok_or(
                BlockTreeError::BlockExpectedButNotFound {
                    block: justify.block.clone(),
                },
            )?;
            let highest_committed_block_height = block_tree.highest_committed_block_height()?;
            let not_committed_yet = highest_committed_block_height.is_none()
                || commit_block_height > highest_committed_block_height.unwrap();

            // If `justify.block` has not been committed yet, ask the caller to commit it. Otherwise, return `None`
            // to prevent redundant commits.
            if not_committed_yet {
                Ok(Some(justify.block))
            } else {
                Ok(None)
            }
        }
    }
}

/// Get the block which the leader of the current view should re-propose (if any).
///
/// If `Ok(Some(block_hash))` is returned, then the leader should re-propose the block identified by
/// `block_hash`. Else if `Ok(None)` is returned, then the leader should either propose a new block, or
/// nudge using the highest qc.
///
/// ## Purpose
///
/// The leader needs to re-propose an existing block in scenarios where the Highest QC in the Block Tree
/// indicates that the consecutive-view sequence of nudges required to commit a validator-set updating
/// block has been broken. In these scenarios, nudging the Highest QC will not allow state machine
/// replication to make progress.
pub(crate) fn repropose_block<K: KVStore>(
    cur_view: ViewNumber,
    block_tree: &BlockTree<K>,
) -> Result<Option<CryptoHash>, BlockTreeError> {
    let highest_qc = block_tree.highest_qc()?;
    match highest_qc.phase {
        Phase::Prepare | Phase::Precommit if cur_view != highest_qc.view + 1 => {
            Ok(Some(highest_qc.block))
        }
        _ => Ok(None),
    }
}

/// Check whether a given quorum certificate belongs to the branch that extends from the locked block.
///
/// Since in pipelined HotStuff it is always the grandparent of the newest block that is locked, we only
/// need to check the QC's block, its parent, and its grandparent.
fn extends_locked_qc_block<K: KVStore>(
    qc: &QuorumCertificate,
    block_tree: &BlockTree<K>,
) -> Result<bool, BlockTreeError> {
    let locked_qc = block_tree.locked_qc()?;
    let block = qc.block;
    let block_parent = block_tree.block_justify(&block).ok().map(|qc| qc.block);
    let block_grandparent = block_parent
        .map(|b| block_tree.block_justify(&b).ok().map(|qc| qc.block))
        .flatten();
    Ok(block == locked_qc.block
        || block_parent.is_some_and(|b| b == locked_qc.block)
        || block_grandparent.is_some_and(|b| b == locked_qc.block))
}
