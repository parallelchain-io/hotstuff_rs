/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Rules and predicates that help with maintaining the invariant properties of the Block Tree data
//! structure.
//!
//! The rustdoc for this module is divided into three sections:
//! 1. [Invariants](#invariants) clarifies the notion of block tree invariants and groups them into two
//!    categories depending on whether they are "local" or "global" in nature.
//! 2. [Methods](#methods) lists the methods defined in this module and groups them into two categories
//!    depending on whether they deal with "whether" questions about state updates or "what" questions.
//! 3. Finally, [Blockchain Consistency](#blockchain-consistency) discusses HotStuff-rs' overarching
//!    global invariant, "blockchain consistency", and how the methods in this module work together to
//!    enforce it.
//!
//! # Invariants
//!  
//! In the context of this module, invariants are logical properties that are always true about the
//! Block Tree.
//!
//! Block tree invariants can be grouped into two categories depending on their scope:
//! - **Local Invariants**: invariants that pertain to isolated parts of the block tree. For example, the
//!   invariant enforced by `safe_nudge` that `nudge.justify.phase` must be either `Prepare`, `Precommit`, or
//!   `Commit`.
//! - **Global Invariants**: invariants that relate different parts of the block tree. For example, the
//!   invariant enforced by `safe_qc` that either: (i). `qc.block` extends from
//!   `block_tree.locked_qc()?.block`, or (ii). `qc.view` is greater than `block_tree.locked_qc()?.view`.
//!
//! Some simple local invariants can be enforced by the type system at compile-time and therefore
//! do not need to be checked at runtime. For example, the typedef of `Phase` automatically guarantees
//! the invariant `nudge.justify.phase` could only be one of five values--`Generic`, `Prepare`, `Precommit`,
//! `Commit` and `Decide`.
//!
//! More complicated invariants, however (including both global invariants and also more complicated local
//! invariants, as illustrated by the `safe_nudge` and `safe_qc` examples above), can only be enforced by
//! runtime logic. This is where the methods in this module come in.
//!  
//! # Methods
//!
//! TODO: mention the top-level state updaters of the block tree.
//!
//! The methods in this module each help enforce a combination of local and global block tree invariants.
//! Specifically, they do this by ensuring that every block tree *update* is *invariant-preserving*. The idea
//! can be summarized in simple formulaic terms as: block tree that satisfies invariants + invariant-preserving
//! update = an updated block tree that also satisfies invariants.
//!
//! Each method works to ensure that every update is invariant-preserving in one of two different ways:
//! 1. By checking **whether** an event (like receiving a `Proposal` or collecting a `QuorumCertificate`)
//!    can trigger an invariant-preserving update, or
//! 2. By determining **what** invariant-preserving updates should be made in response to an event.
//!
//! These two different ways allow us to group the methods in this module into two different categories:
//!
//! ## Category 1: "whether"
//!
//! Methods in this category: [`safe_qc`], [`safe_block`], [`safe_nudge`], (outlier) [`repropose_block`].
//!
//! These methods check **whether** a `QuorumCertificate`, `Block`, or `Nudge` (respectively) can trigger
//! invariant-preserving state updates. Methods in this category feature in the *preconditions* of the
//! [`insert`](BlockTree::insert) and [`update`](BlockTree::update) block tree mutators.
//!
//! We also include in this category the method called `repropose_block`. This does not fit neatly into
//! this category in terms of name or purpose, but is closely related to `safe_nudge` in that it serves
//! to help proposers choose a block to propose that satisfy the "consecutive views" requirement that
//! `safe_nudge` checks.
//!
//! ## Category 2: "what"
//!
//! Methods in this category: [`qc_to_lock`], [`block_to_commit`].
//!
//! These methods help determine **what** invariant-preserving state updates should be triggered in
//! `update` in response to obtaining a `QuorumCertificate`, whether through receiving a `Proposal`,
//! `Nudge`, or `NewView` message, or by collecting enough `Vote`s. Methods in this category are called
//! *inside* [`update`](BlockTree::update).
//!
//! # Blockchain Consistency
//!
//! The most important global invariant that HotStuff-rs guarantees is called "Blockchain Consistency".
//! Blockchain consistency is the property that the block trees of all honest replicas are **identical**
//! below a certain block height.
//!
//! This "certain block height" is exactly the height of the
//! [Highest Committed Block](super::block_tree#safety). Below this height, the block*tree* (a directed
//! acyclic graph of blocks) is reduced to a block*chain*, a directed acyclic graph of blocks with the
//! restriction that every block has exactly one inward edge (formed by the `justify` of its child) and
//! one outward edge (formed by its `justify` to its parent).
//!
//! The blockchain grows as more and more blocks are *committed*. Committing is a state update whereby
//! a block (and transitively, all of its ancestors) become part of the permanent blockchain.
//! Critically, committing is a one-way update: once a block becomes committed, it can never be
//! un-committed. Because of this, it is essential that the protocol is designed such that a replica only
//! commits when it is sure that all other honest replicas have either also committed the block, or will
//! eventually.
//!
//! This section explains how the methods in this module work together to maintain blockchain
//! consistency. The discussion is split into two parts:
//! 1. First, [Locking](#locking) discusses an intermediate state that blocks enter after being inserted
//! but before being committed, that is, being "Locked".
//! 2. Then, [Committing](#committing) discusses how blocks move between being locked into being committed.
//!
//! ## Locking
//!
//! Before a block is committed, its branch must first be locked.
//!
//! Locking ensures that an honest replica only commits a block when either of the following two
//! conditions hold:
//! 1. All other honest replicas have also committed the block, in which case the commit is trivially
//!    consistent, or
//! 2. If not all honest replicas have committed the block, then a quorum of replicas has at least
//!    *locked* on the block, which makes it impossible for a QC for a conflicting block to be formed.
//!
//! The consequence of condition 2 is that condition 1 will *eventually* hold. Making the block safe to
//! commit.
//!
//! Locking entails keeping track of a block tree variable called
//! ["Locked QC"](super::block_tree#safety) and doing two things with it:
//! 1. **Updating** the Locked QC whenever it is appropriate, according to the logic implemented by
//!    `qc_to_lock`.
//! 2. **Checking** every QC received or collected against the Locked QC. Only QCs that pass this check
//!    and therefore "satisfy the lock" are allowed to cause state updates.
//!
//! Updating the locked QC and checking the locked QC is discussed in turn in the following two
//! subsections:
//!
//! ### Locking on a Block
//!
//! Any time `update` is called, the `locked_qc` should potentially be updated. The [`qc_to_lock`]
//! method in this module decides what locked QC should be updated to.
//!
//! The precise logic used by `qc_to_lock` to decide which QC to lock on is documented in
//! [the doc for `qc_to_lock`](qc_to_lock#qc_to_lock-logic). In short, the basic logic for choosing
//! which QC to lock in HotStuff-rs is the same as the basic logic for choosing which QC to lock in the
//! PODC'19 HotStuff paper, that is, "lock on seeing a 2-Chain".
//!
//! The basic logic is already well-documented in the PODC '19 paper, so for brevity, we do not
//! re-describe it here. Instead, in the rest of this subsection, we describe a small but nuanced
//! difference in the precise logic, and then explain the rationale behind this difference:
//!
//! In both HotStuff-rs and PODC '19 HotStuff, which QC `qc_to_lock` decides to lock on upon receiving
//! `justify` depends on what `justify.phase` is:
//! - If `justify.phase` is `Generic`, `Prepare`, or `Precommit`, `qc_to_lock`'s
//!   decision rule is exactly the same as the decision rule used in the algorithm in the original (PODC'
//!   19) HotStuff paper that corresponds to the [operating mode](crate::hotstuff#operating-mode) that the
//!   `Phase` is part of (recall that the pipelined mode corresponds to Algorithm 1, while the phased mode
//!   corresponds to Algorithm 3).
//! - On the other hand, if `justify.phase` is `Commit` or `Decide`, `qc_to_lock` will decide to lock on
//!   `justify` (as long as the current `locked_qc.block` is different from `justify.block`). This is
//!   **different** from the logic used in Algorithm 1 in the original HotStuff paper, which does not
//!   update `locked_qc` upon receiving a `Commit` QC (there is no phase called `Decide` in the original
//!   HotStuff paper).
//!
//! The reason why the PODC '19 HotStuff does not lock upon receiving a `Commit` or `Decide` QC and
//! HotStuff-rs does becomes clearer when we consider that the original HotStuff makes a simplifying
//! assumption that receiving any proposal implies that we have received every proposal in the chain
//! that precedes the proposal. E.g., receiving a proposal for a block at height 10 means that we (the
//! replica) has previously received a complete set of proposals for the ancestor blocks at heights
//! 0..9, *including for every phase*.
//!
//! This assumption simplifies the specification of the algorithm, and is one that is made by many
//! publications. However, this assumption is difficult to uphold in a production setting, where
//! messages are often dropped. HotStuff-rs' [Block Sync](crate::block_sync) goes some way toward making
//! this assumption hold, but is not perfect: in particular, Sync Servers only send their singular
//! current [`highest_qc`](crate::block_sync::messages::BlockSyncResponse::highest_qc) in their
//! `SyncResponse`s, which could be a QC of any phase: `Generic` up to `Decide`.
//!
//! This means that if we use the same logic as used in Algorithm 1 to decide on which QC to lock on
//! upon receiving a [phased mode](crate::hotstuff#phased-mode) QC, i.e., to lock only if
//! `justify.phase == Precommit`, then we will fail to lock on `justify.block` if `justify.phase` is
//! `Commit` or `Decide`, which can lead to safety violations because the next block may then extend
//! a conflicting branch.
//!
//! Because extending the Block Sync protocol to return multiple QCs in a `SyncResponse` could be
//! complicated (in particular, it would probably require replicas to store extra state), we instead
//! solve this issue by deviating from PODC '19 HotStuff slightly by locking upon receiving a
//! `Commit` or `Decide` QC.
//!
//! ### Checking against the Lock
//!
//! The [3rd predicate](safe_qc#conditional-checks) of `safe_qc` checks whether any received or
//! collected QC satisfies the lock and is therefore allowed to trigger state updates. This predicate
//! is exactly the same as the corresponding predicate in the PODC '19 HotStuff paper, but is simple
//! enough that we describe it and the rationale behind it fully in the rest of this subsection.
//!
//! The 3rd predicate comprises of two clauses, joined by an "or":
//! 1. **Safety clause**: `qc.block` extends from `locked_qc.block`, *or*
//! 2. **Liveness clause**: `qc.view` is greater than `locked_qc.view`.
//!
//! In stable cases--i.e., where in every view, either 1/3rd of replicas lock on the same `locked_qc`,
//! or none lock--the safety clause will always be satisfied. This ensures that the `qc` extends the
//! branch headed by the locked block.
//!
//! In unstable cases, however, where e.g., messages are dropped or a proposer is faulty, less than
//! 1/3rd but more than one replica may lock on the same `locked_qc` in the same view, and therefore a
//! `Block` or `Nudge` that conflicts with `locked_qc` may be proposed in the next view and be accepted
//! by the replicas that didn't lock on `locked_qc` in the previous view. In this case, the replicas that
//! *did* lock on `locked_qc` in the previous view will not be able to accept the new `Block` or `Nudge`,
//! and unless the 3rd predicate of `safe_qc` includes a relaxing clause, these replicas will be stuck,
//! unable to grow their blockchain further.
//!
//! This is where the liveness clause comes in. This clause enables the replicas that did lock on the
//! now "abandoned" QC to eventually accept new `Block`s and `Nudge`s, and does so by relaxing the
//! third predicate to allow `Block`s and `Nudge`s that build on a different branch than the current
//! `locked_qc.block` to cause state updates as long as the QC they contain has a higher view than
//! `locked_qc.view`.
//!
//! ## Committing
//!
//! As is the case with Locking, Committing in HotStuff-rs follows the same basic logic as committing in
//! PODC '19 HotStuff, but with a small and nuanced difference. The following two subsections discuss, in
//! turn:
//! 1. Under what conditions will a block become committed, one of the conditions being a "consecutive
//!    views requirement" that does not exist in PODC '19 HotStuff.
//! 2. How the algorithm requires that replicas *re-propose* existing blocks in certain conditions in order
//!    to satsify the consecutive views requirement while still achieving Immediacy.
//!
//! ### Committing a Block
//!
//! Any time `update` is called, blocks should potentially be committed. The [`block_to_commit`] method
//! in this module decides what blocks should be committed.
//!
//! Like with `qc_to_lock`, the precise logic used by `block_to_commit` is documented in
//! [the doc for `block_to_commit`](block_to_commit#block_to_commit-logic). Again, the logic used for
//! choosing which block to commit in HotStuff-rs is broadly similar as the logic used for choosing
//! which block to commit in the PODC '19 HotStuff paper. In particular, the logic used in HotStuff-rs'
//! Pipelined Mode is the same as the logic used in Algorithm 3 in PODC '19 HotStuff; that is, a block
//! should be committed in the Pipelined Mode when it meets two requirements:
//! 1. **3-Chain**: the block must head a sequence of 3 QCs.
//! 2. **Consecutive views**: the 3 QCs that follow the block must each have *consecutively increasing*
//!    views, i.e., `justify3.view == justify2.view + 1 == justify1.view + 2` where
//!    `justify3.block.justify == justify2`, `justify2.block.justify == justify1`, and `justify1.block
//!    = block`.
//!
//! The nuanced difference between
//!
//! The underlying reason why the consecutive views requirement is needed in HotStuff-rs but not in PODC
//! '19 HotStuff is a specific difference between PODC '19's Algorithm 1 and HotStuff-rs' Phased Mode:
//! - In Algorithm 1, each view is comprised of *3 phases*.
//! - In Phased Mode, each view is comprised of *only 1 phase*.
//!
//! This means that in Phased Mode, the formation of `Prepare`, `Precommit`,
//!
//!
//! ### Ensuring Immediacy
//!

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

/// Check whether `block` can safely cause updates to `block_tree`, given the replica's `chain_id`.
///
/// # Conditional checks
///
/// `safe_block` returns `true` in case all of the following predicates are `true`:
/// 1. `safe_qc(&block.justify, block_tree, chain_id)`.
/// 2. `block.qc.phase` is either `Generic` or `Decide`.
///
/// # Precondition
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

/// Check whether `qc` can safely cause updates to `block_tree`, given the replica's `chain_id`.
///
/// # Conditional checks
///
/// `safe_qc` returns `true` in case all of the following predicates are `true`:
/// 1. Either `qc.chain_id` equals `chain_id`, or `qc` is the Genesis QC.
/// 2. Either `block_tree` contains `qc.block`, or `qc` is the Genesis QC.
/// 3. Either `qc.view` is greater than `block_tree`'s `locked_qc.view`, or `qc.block` extends from
///    `locked_qc.block`.
/// 4. If `qc.phase` is `Prepare`, `Precommit`, `Commit`, or `Decide`, `qc.block` is a validator set
///    updating block. Else, if `qc.phase` is `Generic`, `qc.block` is not a validator set updating
///    block.
///
/// # Precondition
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
        /* 4 (else) */ (qc.phase.is_generic() && !block_tree.validator_set_updates_status(&qc.block)?.contains_updates())),
    )
}

/// Check whether `nudge` can safely cause updates to `block_tree`, given the replica's `current_view`
/// and `chain_id`.
///
/// # Conditional checks
///
/// `safe_nudge` returns `true` in case all of the following predicates are `true`:
/// 1. `safe_qc(&nudge.justify, block_tree, chain_id)`.
/// 2. `nudge.justify.phase` is `Prepare`, `Precommit`, or `Commit`.
/// 3. `nudge.chain_id` equals `chain_id`.
/// 4. `nudge.justify` is either a Commit QC, or `nudge.justify.view = current_view - 1`.
///
/// # Precondition
///
/// [`is_correct`](crate::types::collectors::Certificate::is_correct) is `true` for `nudge.justify`.
pub fn safe_nudge<K: KVStore>(
    nudge: &Nudge,
    current_view: ViewNumber,
    block_tree: &BlockTree<K>,
    chain_id: ChainID,
) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */
        safe_qc(&nudge.justify, block_tree, chain_id)? &&
        /* 2 */ nudge.justify.is_nudge_justify() &&
        /* 3 */ (nudge.chain_id == chain_id) &&
        /* 4 */ (nudge.justify.phase.is_commit() || nudge.justify.view == current_view - 1),
    )
}

/// Get the QC (if any) that should be set as the Locked QC after the replica sees the given `justify`.
///
/// # Precondition
///
/// The block or nudge containing this justify must satisfy [`safe_block`] or [`safe_nudge`]
/// respectively.
///
/// # `qc_to_lock` logic
///
/// `qc_to_lock`'s return value depends on `justify`'s phase and whether or not it is the Genesis QC.
/// What `qc_to_lock` returns in every case is summarized in the below table:
///
/// |`justify` is the/a|QC to lock if not already the current locked QC|
/// |---|---|
/// |Genesis QC|`None`|
/// |`Generic` QC|`justify.block.justify`|
/// |`Prepare` QC|`None`|
/// |`Precommit` QC|`justify`|
/// |`Commit` QC|`justify`|
/// |`Decide` QC|`justify`|
///
/// The rationale behind `qc_to_lock`'s logic is explained in
/// [`safety`'s module-level docs](super::safety#updating-the-lock).
pub(crate) fn qc_to_lock<K: KVStore>(
    justify: &QuorumCertificate,
    block_tree: &BlockTree<K>,
) -> Result<Option<QuorumCertificate>, BlockTreeError> {
    // Special case: if `justify` is the Genesis QC, there is no QC to lock.
    if justify.is_genesis_qc() {
        return Ok(None);
    }

    // Determine the `QuorumCertificate` to lock according to `justify.phase`.
    let locked_qc = block_tree.locked_qc()?;
    let qc_to_lock = match justify.phase {
        // If `justify.phase` is `Generic`, lock on `justify.block.justify`.
        Phase::Generic => {
            let parent_justify = block_tree.block_justify(&justify.block)?;
            Some(parent_justify.clone())
        }

        // If `justify.phase` is `Prepare`, don't lock.
        Phase::Prepare => None,

        // If `justify.phase` is `Precommit`, lock on `justify`.
        Phase::Precommit => Some(justify.clone()),

        // If `justify.phase` is `Commit` or `Decide`, *and* `locked_qc.block != justify.block`, lock on
        // `justify`. The second condition is to prevent redundant locking (e.g., once we've locked on a
        // `Precommit` QC justifying a block, we don't have to lock on a `Commit` or `Decide` QC justifying
        // the same block).
        Phase::Commit | Phase::Decide => {
            if locked_qc.block != justify.block {
                Some(justify.clone())
            } else {
                None
            }
        }
    };

    // Prevent redundant locking by returning Some only if the QC to lock is not already the locked QC.
    if qc_to_lock.as_ref().is_some_and(|qc| qc != &locked_qc) {
        Ok(qc_to_lock)
    } else {
        Ok(None)
    }
}

/// Get the `Block` in `block_tree` (if any) that, along with all of its uncommitted predecessors,
/// should be committed after the replica sees `justify`.
///
/// ## Preconditions
///
/// The `Block` or `Nudge` containing `justify` must satisfy [`safe_block`] or [`safe_nudge`]
/// respectively.
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
/// # `block_to_commit` logic
///
/// |`justify.phase` is a|Block to commit if satisfies the consecutive views rule and not already committed yet|
/// |---|---|
/// |`Generic` QC|`justify.block.justify.block.justify.block`|
/// |`Prepare` QC|`None`|
/// |`Precommit` QC|`None`|
/// |`Commit` QC|`justify.block`|
/// |`Decide` QC|`justify.block`|
pub(crate) fn block_to_commit<K: KVStore>(
    justify: &QuorumCertificate,
    block_tree: &BlockTree<K>,
) -> Result<Option<CryptoHash>, BlockTreeError> {
    // Special case: if `justify` is the Genesis QC, there is no block to commit.
    if justify.is_genesis_qc() {
        return Ok(None);
    };

    // Determine the `Block` to commit according to `justify.phase`.
    match justify.phase {
        // If `justify.phase` is `Generic`, commit the `justify`'s great-grandparent block, if it has one in
        // the block tree.
        Phase::Generic => {
            // Check whether the parent block or the grandparent block is a genesis block. If so, there is no
            // great-grandparent block to commit.
            let parent_justify = block_tree.block_justify(&justify.block)?;
            if parent_justify.is_genesis_qc() {
                return Ok(None);
            };
            let grandparent_justify = block_tree.block_justify(&parent_justify.block)?;
            if grandparent_justify.is_genesis_qc() {
                return Ok(None);
            };

            // Check whether the "consecutive views" requirement of the commit rule is satisfied.
            let commit_rule_satisfied = justify.view == parent_justify.view + 1
                && parent_justify.view == grandparent_justify.view + 1;

            // Check whether the great-grandparent block has been committed already.
            let not_committed_yet = {
                let greatgrandparent_height = block_tree
                    .block_height(&grandparent_justify.block)?
                    .ok_or(BlockTreeError::BlockExpectedButNotFound {
                        block: grandparent_justify.block.clone(),
                    })?;
                let highest_committed_block_height = block_tree.highest_committed_block_height()?;

                highest_committed_block_height.is_none()
                    || greatgrandparent_height > highest_committed_block_height.unwrap()
            };

            // If the commit rule is satisfied and the great-grandparent block has not been committed yet, ask the
            // caller to commit it. Otherwise, return `None` to prevent redundant commits.
            if commit_rule_satisfied && not_committed_yet {
                Ok(Some(grandparent_justify.block))
            } else {
                Ok(None)
            }
        }

        // If `justify.phase` is `Prepare` or `Precommit`, do not commit any block.
        Phase::Prepare | Phase::Precommit => Ok(None),

        // If `justify.phase` is `Commit` or `Decide`, commit `justify.block`.
        Phase::Commit | Phase::Decide => {
            // Check whether the parent block has been committed already.
            let not_committed_yet = {
                let parent_height = block_tree.block_height(&justify.block)?.ok_or(
                    BlockTreeError::BlockExpectedButNotFound {
                        block: justify.block.clone(),
                    },
                )?;
                let highest_committed_block_height = block_tree.highest_committed_block_height()?;

                highest_committed_block_height.is_none()
                    || parent_height > highest_committed_block_height.unwrap()
            };

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

/// Get the `Block` in the `block_tree` which a leader of the `current_view` should re-propose.
///
/// # Usage
///
/// If `Ok(Some(block_hash))` is returned, then the leader should re-propose the block identified by
/// `block_hash`.
///
/// Else if `Ok(None)` is returned, then the leader should either propose a new block, or
/// nudge using the highest qc.
///
/// # Purpose
///
/// The leader needs to re-propose an existing block in scenarios where the Highest QC in the Block Tree
/// indicates that the consecutive-view sequence of nudges required to commit a validator-set updating
/// block has been broken. In these scenarios, nudging the Highest QC will not allow state machine
/// replication to make progress.
pub(crate) fn repropose_block<K: KVStore>(
    current_view: ViewNumber,
    block_tree: &BlockTree<K>,
) -> Result<Option<CryptoHash>, BlockTreeError> {
    let highest_qc = block_tree.highest_qc()?;
    match highest_qc.phase {
        Phase::Prepare | Phase::Precommit if current_view != highest_qc.view + 1 => {
            Ok(Some(highest_qc.block))
        }
        _ => Ok(None),
    }
}

/// Check whether `qc` belongs to the branch that extends from the `block_tree`'s `locked_qc.block`.
///
/// # Procedure
///
/// In the phased mode, it is always the latest block that is locked, while in the pipelined mode, it
/// is the grandparent of the newest block that is locked. Therefore, to see whether `qc` extends from
/// `locked_qc.block`, we only need to check whether or not `locked_qc.block` is `qc.block`, or its
/// parent, or its grandparent. We do not need to check deeper.
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
