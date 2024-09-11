/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Rules and predicates that collectively ensure that all state updates made to the block tree are
//! safe.
//!
//! ## Block Tree state updates
//!
//! A state machine replication algorithm like HotStuff-rs is considered "safe" if it is impossible for
//! two replicas to commit conflicting state updates as long as less than a threshold of validators are
//! Byzantine.
//!
//! The top-level state updates that a HotStuff-rs replica can perform are performed by the
//! [top-level state updaters](BlockTree#impl-BlockTree<K>-1) category of methods defined in the
//! `block_tree` module on the `BlockTree` struct.
//!
//! The crate-public methods in this (`safety`) module are only relevant to the
//! [`insert`](BlockTree::insert), [`update`](BlockTree::update) top-level state updaters. The remaining
//! three state updaters have much simpler preconditions and perform much simpler state mutations, and
//! thus do not need helper functions like those defined here.
//!
//! First we list these methods, then we discuss one of the big picture invariants that these methods
//! maintain to keep HotStuff-rs SMR safe: Locking. (TODO Alice: expand).
//!
//! ## Two categories of methods
//!
//! The crate-public methods defined in module `safety` fall into two categories. Each category of
//! methods follow a consistent naming convention and play a distinct part in ensuring that all updates
//! made to the block tree are safe. Simply put, the first category of methods check **whether** state
//! updates can safely be triggered, while the second category of methods help determine **what** state
//! updates should be triggered.
//!
//! ### Category 1: safe_{type}
//!
//! Methods in this category: [`safe_qc`], [`safe_block`], [`safe_nudge`].
//!
//! These methods check **whether** a `QuorumCertificate`, `Block`, or `Nudge` (respectively) can safely
//! trigger state updates. Methods in this category are part of the preconditions of `insert` and `update`.
//!
//! Methods in this category check two kinds of properties to determine safety:
//! 1. Internal properties: these properties are simple and can be checked without querying the block
//!    tree. For example: checking in `safe_nudge` that `nudge.justify` is a `Prepare`, `Precommit`, or
//!    `Commit` QC.
//! 2. External properties: these properties are more complicated and checking them entail querying the
//!    block tree. For example: checking in `safe_qc` that either: (i) `qc.view` is greater than
//!    `block_tree.locked_qc()?.view`, or (ii). `qc.block` extends (directly or transitively) from
//!    `block_tree.locked_qc()?.block`.
//!
//! ### Category 2: {type}\_to\_{lock/commit}
//!
//! Methods in this category: [`qc_to_lock`], [`block_to_commit`].
//!
//! These methods help determine **what** state updates should be triggered in `update` in response to
//! obtaining a `QuorumCertificate`, whether through receiving a `Proposal`, `Nudge`, or `NewView`
//! message, or by collecting enough `Vote`s. Methods in this category are called inside `update`.
//!
//! ### Outlier: repropose_block
//!
//! This module also defines a method called [`repropose_block`]. This does not fit neatly into either
//! of the above two categories in terms of name or function, but is closely related to `safe_nudge`
//! in that it serves to help proposers choose a block to propose that satisfy the "consecutive views"
//! property that `safe_nudge` checks.
//!
//! ## Locking
//!
//! The most integral, and yet least obvious invariant that the methods in this module maintains has
//! to do with *locking*.
//!
//! Locking entails keeping track of a block tree variable called
//! ["Locked QC"](super::block_tree#safety), and doing two things with it:
//! 1. **Checking** every QC received or collected against it in `safe_qc`. Only QCs that pass this check
//!    and therefore "satisfy the lock" are allowed to cause state updates.
//! 2. **Updating** the Locked QC whenever it is appropriate, according to the logic implemented by
//!    `qc_to_lock`. `qc_to_lock` is called in [`BlockTree::update`] every time the latter is called.
//!
//! Locking serves to guarantee the uniqueness of commits. The idea is that if any single honest replica
//! *commits* on a block, then either: (i). All other honest replicas have also committed the block, in
//! which case the commit is trivially unique, or (ii). If not all honest replicas have committed the
//! block, then a quorum of replicas has at least *locked* on the block, which ensures that (i) will
//! eventually be true.
//!
//! ### Checking against the lock
//!
//! The 3rd predicate of `safe_qc` checks whether any received or collected QC satisfies the lock and is
//! therefore allowed to trigger state updates.
//!
//! Note how this predicate has two clauses, joined by an "or":
//! 1. `qc.block` extends from `locked_qc.block`, *or*
//! 2. `qc.view` is greater than `locked_qc.view`.
//!
//! Both clauses serve distinct purposes which are discussed at length in the original HotStuff paper.
//!
//! In short, the first clause (the "safety clause") will always be satisfied in the stable case, where
//! in every view, either a 1/3rd of replicas lock on the same `locked_qc`, or none lock.
//!
//! In unstable cases, however, where e.g., messages are dropped or a proposer is faulty, less than
//! 1/3rd but more than one replica may lock on the same `locked_qc` in the same view, and therefore a
//! `Block` or `Nudge` that conflicts with `locked_qc` may be proposed in the next view and be accepted
//! by the replicas that didn't lock on `locked_qc` in the previous view. In this case, the replicas that
//! *did* lock on `locked_qc` in the previous view will not be able to accept the new `Block` or `Nudge`,
//! and unless the 3rd predicate of `safe_qc` includes a relaxing clause, these replicas will be stuck,
//! unable to grow their blockchain further.
//!
//! This is where the second clause (the "liveness clause") comes in. It allows the replicas that did
//! lock on the now "abandoned" QC to eventually accept new `Block`s and `Nudge`s, and does so by
//! relaxing the third predicate to allow `Block`s and `Nudge`s that build on a different branch than
//! the current `locked_qc.block` to cause state updates as long as the QC they contain has a higher
//! view than `locked_qc.view`.
//!
//! ### Updating the lock
//!
//! Any time [`BlockTree::update`] is called, Locked QC should potentially be updated, and `qc_to_lock`
//! decides what Locked QC should be updated *to*.
//!
//! The logic used by `qc_to_lock` to decide which QC to lock on is documented in
//! [the doc for `qc_to_lock`](qc_to_lock#qc_to_lock-logic). Below, we explain the rationale behind
//! this logic, including how and why the implemented logic is slightly different from the logic used
//! in the original HotStuff (PODC '19) paper.
//!
//! Which QC `qc_to_lock` decides to lock on upon receiving `justify` partly depends on what
//! `justify.phase` is:
//! - If `justify.phase` is `Generic`, `Prepare`, or `Precommit`, `qc_to_lock`'s
//!   decision rule is exactly the same as the decision rule used in the algorithm in the original (PODC'
//!   19) HotStuff paper that corresponds to the [operating mode](crate::hotstuff#operating-mode) that the
//!   `Phase` is part of (recall that the pipelined mode corresponds to Algorithm 1, while the phased mode
//!   corresponds to Algorithm 3).
//! - On the other hand, if `justify.phase` is `Commit` or `Decide`, `qc_to_lock` will decide to lock on
//!   `justify` (as long as the current `locked_qc.block` is different from `justify.block`). This is in
//!   contrast to the logic used in Algorithm 1 in the original HotStuff paper, which does not update
//!   `locked_qc` upon receiving a `Commit` QC (there is no phase called `Decide` in the original HotStuff
//!   paper).
//!
//! This difference is necessary so that replicas can reliably update their `locked_qc`s during
//! [Block Sync](crate::block_sync). During Block Sync, Sync Servers only send their `highest_qc` in their
//! [`SyncResponse`](crate::block_sync::messages::BlockSyncResponse). This means that sometimes, replicas
//! may not receive a `Precommit` QC during Block Sync. We still want `locked_qc` to be updated if this
//! happens, which is why we update `locked_qc` if `justify.phase` is `Commit` or `Decide`. (TODO Alice: improve language)

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
/// 2. `block.qc` is either a Generic QC or a Decide QC.
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
/// 1. `qc.chain_id` either matches the Chain ID of the replica, or `qc` is the genesis qc.
/// 2. Either `qc.block` is in the block tree, or `qc` is the Genesis QC.
/// 3. Either `qc`'s view number is greater than the Locked QC's view, or its block extends from the locked
///    block.
/// 4. If `qc` is a Prepare, Precommit, or Commit QC, the block it justifies has pending validator state
///    updates.
/// 5. If `qc` is a Generic QC, the block it justifies *does not* have pending validator set updates.
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
/// 2. `nudge.justify` is a Prepare, Precommit, or Commit QC.
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
/// ## `qc_to_lock` logic
///
/// |`justify` is the/a|QC to lock if `justify.block != locked_qc.block`|
/// |---|---|
/// |Genesis QC|`None`|
/// |`Generic` QC|`justify.block.justify`|
/// |`Prepare` QC|`None`|
/// |`Precommit` QC|`justify`|
/// |`Commit` QC|`justify`|
/// |`Decide` QC|`justify`|
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
/// ## Commit Rules
///
/// To prevent different replicas from entering inconsistent states due to Byzantine leaders causing
/// safety-threatening branch switches, the pipelined HotStuff protocol requires that a block must be
/// followed by a "3-chain" in order to be committed. This means that the block must be followed by
/// a child, a grandchild, and a great-grandchild block.
///
/// In addition, the three QCs contained in its child, grandchild, and great-grandchild blocks, serving
/// as its "virtual" Prepare QC, Precommit QC, and Commit QC respectively, must have **consecutive
/// views**. Note however that while consecutive views are required for a block to be safely committed,
/// they are not required for a block to be inserted to the block tree, and therefore `safe_block` does
/// not enforce it. Consecutive views are also not required for a block to be locked.
///
/// To obtain an equivalent guarantee for the phased HotStuff protocol for validator-set-updating
/// blocks, we require that the Prepare QC, Precommit QC, and Commit QC for a validator-set-updating
/// block received via nudges have consecutive views. If this flow is interrupted by a temporary loss of
/// synchrony or a Byzantine leader, the validator-set-updating block has to be re-proposed, as
/// specified in [`repropose_block`]. This rule is enforced through [`safe_nudge`]. Unlike block
/// insertions, nudges cannot cause state updates unless they satisfy the consecutive views rule.
///
/// ## Commit Rules
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
