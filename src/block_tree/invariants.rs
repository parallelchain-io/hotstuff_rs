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
//! - **Local Invariants**: invariants that pertain to isolated parts of the block tree. An example of a
//!   local invariant is the invariant enforced by `safe_nudge` that `nudge.justify.phase` must be either
//!   `Prepare`, `Precommit`, or `Commit`.
//! - **Global Invariants**: invariants that relate different parts of the block tree. An example of a
//!   global invariant is the invariant enforced by `safe_pc` that either: (i). `pc.block` extends from
//!   `block_tree.locked_pc()?.block`, or (ii). `pc.view` is greater than `block_tree.locked_pc()?.view`.
//!
//! Some simple local invariants can be enforced by the type system at compile-time and therefore
//! do not need to be checked at runtime. For example, the typedef of `Phase` automatically guarantees
//! the invariant `nudge.justify.phase` could only be one of five values--`Generic`, `Prepare`, `Precommit`,
//! `Commit` and `Decide`.
//!
//! More complicated invariants, however (including both global invariants and also more complicated local
//! invariants, as illustrated by the `safe_nudge` and `safe_pc` examples above), can only be enforced by
//! runtime logic. This is where the methods in this module come in.
//!  
//! # Methods
//!
//! The methods in this module each help enforce a combination of local and global block tree
//! invariants. Specifically, they do this by ensuring that every block tree *update*, i.e., set of
//! state mutations done by the
//! [top-level updater methods](BlockTreeSingleton#impl-BlockTreeSingleton<K>-1) defined on the
//! `BlockTreeSingleton` struct, is invariant-preserving. This idea can be summarized in simple
//! formulaic terms as: a block tree that satisfies invariants + a invariant-preserving update = an
//! updated block tree that also satisfies invariants.
//!
//! Each method works to ensure that every update is invariant-preserving in one of two different ways:
//! 1. By checking **whether** an event (like receiving a `Proposal` or collecting a `PhaseCertificate`)
//!    can trigger an invariant-preserving update, or
//! 2. By determining **what** invariant-preserving updates should be made in response to an event.
//!
//! These two different ways allow us to group the methods in this module into the two different
//! categories discussed in the following subsections.
//!
//! Before reading the following subsections, please first note that not every top-level updater method
//! directly uses or is related to the methods in this module. In particular, `set_highest_tc`,
//! `set_highest_view_entered`, and `set_highest_view_voted` have simple enough preconditions that they
//! do not need to have functions of the "whether" category in this module, and do state updates that are
//! simple enough that they do not need functions of the "what" class either. The methods in this module
//! only directly relate to the [`insert`](BlockTree::insert) and [`update`](BlockTree::update) top-level
//! state mutators.
//!
//! ## Category 1: "whether"
//!
//! Methods in this category: [`safe_pc`], [`safe_block`], [`safe_nudge`], (outlier) [`repropose_block`].
//!
//! These methods check **whether** a `PhaseCertificate`, `Block`, or `Nudge` (respectively) can
//! trigger invariant-preserving state updates. Methods in this category feature in the *preconditions*
//! of the `insert` and `update`.
//!
//! We also include in this category the method called `repropose_block`. This method does not fit
//! neatly into this category in terms of name or purpose, but is closely related to `safe_nudge` in
//! that it serves to help proposers choose a block to propose that satisfy the "consecutive views"
//! requirement that `safe_nudge` checks.
//!
//! ## Category 2: "what"
//!
//! Methods in this category: [`pc_to_lock`], [`block_to_commit`].
//!
//! These methods help determine **what** invariant-preserving state updates should be triggered in
//! `update` in response to obtaining a `PhaseCertificate`, whether through receiving a `Proposal`,
//! `Nudge`, or `NewView` message, or by collecting enough `PhaseVote`s. Methods in this category are
//! called *inside* [`update`](BlockTree::update).
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
//! 2. If not all honest replicas have committed the block, then a quorum of replicas is currently
//!    *locked* on the block, which makes it impossible for a PC for a conflicting block to be formed.
//!
//! The consequence of condition 2 is that condition 1 will *eventually* hold, making the block safe to
//! commit.
//!
//! Locking entails keeping track of a block tree variable called
//! ["Locked PC"](super::block_tree#safety) and doing two things with it:
//! 1. **Updating** the Locked PC whenever it is appropriate, according to the logic implemented by
//!    `pc_to_lock`, and
//! 2. **Checking** every PC received or collected against the Locked PC. Only PCs that pass this check
//!    and therefore "satisfy the lock" are allowed to cause state updates.
//!
//! Updating the locked PC and checking the locked PC is discussed in turn in the following two
//! subsections:
//!
//! ### Locking on a Block
//!
//! Any time `update` is called, the `locked_pc` should potentially be updated. The [`pc_to_lock`]
//! method in this module decides what locked PC should be *updated to*.
//!
//! The precise logic used by `pc_to_lock` to decide which PC to lock on is documented in
//! [the doc for `pc_to_lock`](pc_to_lock#pc_to_lock-logic). In short, the basic logic for choosing
//! which PC to lock in HotStuff-rs is the same as the basic logic for choosing which PC to lock in the
//! PODC'19 HotStuff paper, that is, "lock on seeing a 2-Chain".
//!
//! The basic logic is already well-documented in the PODC '19 paper, so for brevity, we do not
//! re-describe it here. Instead, in the rest of this subsection, we describe a small but nuanced
//! difference in the precise logic, and then explain the rationale behind this difference:
//!
//! In both HotStuff-rs and PODC '19 HotStuff, which PC `pc_to_lock` decides to lock on upon receiving
//! `justify` depends on what `justify.phase` is:
//! - If `justify.phase` is `Generic`, `Prepare`, or `Precommit`, `pc_to_lock`'s
//!   decision rule is exactly the same as the decision rule used in the algorithm in the original (PODC'
//!   19) HotStuff paper that corresponds to the [operating mode](crate::hotstuff#operating-mode) that the
//!   `Phase` is part of (recall that the Pipelined Mode corresponds to Algorithm 1, while the Phased Mode
//!   corresponds to Algorithm 3).
//! - On the other hand, if `justify.phase` is `Commit` or `Decide`, `pc_to_lock` will decide to lock on
//!   `justify` (as long as the current `locked_pc.block` is different from `justify.block`). This is
//!   **different** from the logic used in Algorithm 1 in the original HotStuff paper, which does not
//!   update `locked_pc` upon receiving a `Commit` PC (there is no phase called `Decide` in the original
//!   HotStuff paper).
//!
//! The reason why the PODC '19 HotStuff does not lock upon receiving a `Commit` or `Decide` PC while
//! HotStuff-rs does becomes clearer when we consider that the original HotStuff makes the simplifying
//! assumption that receiving any proposal implies that we have received every proposal in the chain
//! that precedes the proposal. E.g., receiving a proposal for a block at height 10 means that we (the
//! replica) has previously received a complete set of proposals for the ancestor blocks at heights
//! 0..9, *including for every phase*.
//!
//! This assumption simplifies the specification of the algorithm, and is one that is made by many
//! publications. However, this assumption is difficult to uphold in a production setting, where
//! messages are often dropped. HotStuff-rs' [Block Sync](crate::block_sync) goes some way toward making
//! this assumption hold, but is not perfect: in particular, Sync Servers only send their singular
//! current [`highest_pc`](crate::block_sync::messages::BlockSyncResponse::highest_pc) in their
//! `SyncResponse`s, which could be a PC of any phase: `Generic` up to `Decide`.
//!
//! This means that if we use the same logic as used in Algorithm 1 to decide on which PC to lock on
//! upon receiving a [phased mode](crate::hotstuff#phased-mode) PC, i.e., to lock only if
//! `justify.phase == Precommit`, then we will fail to lock on `justify.block` if `justify.phase` is
//! `Commit` or `Decide`, which can lead to safety violations because the next block may then extend
//! a conflicting branch.
//!
//! Because extending the Block Sync protocol to return multiple PCs in a `SyncResponse` could be
//! complicated (in particular, it would probably require replicas to store extra state), we instead
//! solve this issue by deviating from PODC '19 HotStuff slightly by locking upon receiving a
//! `Commit` or `Decide` PC.
//!
//! ### Checking against the Lock
//!
//! The [3rd predicate of `safe_pc`](safe_pc#conditional-checks) checks whether any received or
//! collected PC satisfies the lock and therefore is allowed to trigger state updates. This predicate
//! is exactly the same as the corresponding predicate in the PODC '19 HotStuff paper, but is simple
//! enough that we describe it and the rationale behind it fully in the rest of this subsection.
//!
//! The 3rd predicate comprises of two clauses, joined by an "or":
//! 1. **Safety clause**: `pc.block` extends from `locked_pc.block`, *or*
//! 2. **Liveness clause**: `pc.view` is greater than `locked_pc.view`.
//!
//! In stable cases--i.e., where in every view, either 1/3rd of replicas lock on the same `locked_pc`,
//! or none lock--the safety clause will always be satisfied. This ensures that the `pc` extends the
//! branch headed by the locked block.
//!
//! In unstable cases, however, where e.g., messages are dropped or a proposer is faulty, less than
//! 1/3rd but more than zero replicas may lock on the same `locked_pc`. If, in this scenario, `safe_pc`
//! only comprises of the safety clause and a `Block` or `Nudge` that conflicts with `locked_pc` is
//! proposed in the next view, only replicas that didn't lock on `locked_pc` in the previous view will
//! be able to accept the new `Block` or `Nudge` and make progress, while the replicas that did lock
//! will be stuck, unable to grow their blockchain further.
//!
//! This is where the liveness clause comes in. This clause enables the replicas that did lock on the
//! now "abandoned" PC to eventually accept new `Block`s and `Nudge`s, and does so by relaxing the
//! third predicate to allow `Block`s and `Nudge`s that build on a different branch than the current
//! `locked_pc.block` to cause state updates as long as the PC they contain has a higher view than
//! `locked_pc.view`.
//!
//! ## Committing
//!
//! As is the case with Locking, Committing in HotStuff-rs follows the same basic logic as committing in
//! PODC '19 HotStuff, but with a small and nuanced difference. The following two subsections discuss, in
//! turn:
//! 1. The conditions in which a block becomes committed, one of the conditions being a "consecutive views
//!    requirement" that is more relaxed than the "same views requirement" used in Algorithm 1 of PODC '19
//!    HotStuff.
//! 2. How the algorithm requires that replicas *re-propose* existing blocks in certain conditions in order
//!    to satisfy the consecutive views requirement while still achieving Immediacy.
//!
//! ### Committing a Block
//!
//! Any time `update` is called, blocks should potentially be committed. The [`block_to_commit`] method
//! in this module decides what blocks should be committed.
//!
//! Like with `pc_to_lock`, the precise logic used by `block_to_commit` is documented in
//! [the doc for `block_to_commit`](block_to_commit#block_to_commit-logic). Again, the logic used for
//! choosing which block to commit in HotStuff-rs is broadly similar as the logic used for choosing
//! which block to commit in the PODC '19 HotStuff paper.
//!
//! In particular, the logic used in HotStuff-rs' Pipelined Mode is the same as the logic used in
//! Algorithm 3 in PODC '19 HotStuff; that is, a block should be committed in the Pipelined Mode when it
//! meets two requirements:
//! 1. **3-Chain**: the block must head a sequence of 3 PCs.
//! 2. **Consecutive views**: the 3 PCs that follow the block must each have *consecutively increasing*
//!    views, i.e., `justify3.view == justify2.view + 1 == justify1.view + 2` where
//!    `justify3.block.justify == justify2`, `justify2.block.justify == justify1`, and `justify1.block
//!    = block`.
//!
//! The nuanced difference between HotStuff-rs and PODC '19 HotStuff with regards to `block_to_commit`
//! logic has to do with the *Phased Mode*. Specifically, the difference is that PODC '19's Algorithm 1
//! requires that `Prepare`, `Precommit`, and `Commit` PCs that follow a block have the **same view**
//! number in order for this sequence of PCs to commit the block, whereas on the other hand,
//! HotStuff-rs' Phased Mode requires *only* that these PCs have **consecutive view** numbers, just
//! like Pipelined Mode and Algorithm 3.
//!
//! The underlying reason why the same view requirement is used in PODC '19's Algorithm 1 but the
//! strictly less stringent consecutive views requirement is used in Phased Mode is one specific
//! difference between these two algorithms:
//! - In Algorithm 1, each view is comprised of *3 phases*.
//! - In Phased Mode, each view is comprised of only *1 phase*.
//!
//! The result is that in Phased Mode, `Prepare`, `Precommit`, and `Commit` PCs can *never* have the
//! same view number, so if "same view" is a requirement to commit a block in Phased Mode, no block can
//! ever be committed.
//!
//! The consecutive views requirement relaxes `block_to_commit` enough in order for blocks to be
//! committed, but does not relax it *too* far that it would obviate the uniqueness guarantee provided
//! by locking.
//!
//! Consider what could happen if we had instead, for example, relaxed the requirement further to just
//! "increasing views", and a replica commits a block upon receiving `Prepare`, `Precommit`, and
//! `Commit` PCs for the block with views 4, 5 and 7. Because 5 and 7 are not contiguous, it could be
//! the case that in view 6, a quorum of replicas have locked on a conflicting block, so it would be
//! incorrect to assume that a quorum of replicas is currently locked on the block, and therefore it is
//! unsafe in this situation to commit the block.
//!
//! ### Ensuring Immediacy
//!
//! Recall that Immediacy requires validator set updating blocks to be committed by a `Commit` PC
//! before a direct child can be inserted. This requirement, combined with the consecutive views
//! requirement, creates a challenge for proposers.
//!
//! Normally, proposers query the `highest_pc` and broadcast a `Proposal` or `Nudge` containing it
//! to all replicas. When views fail to make progress, however, the `current_view` of live replicas may
//! grow to significantly greater than `highest_pc.view`. If in this situation, more than 1/3rd of
//! replicas have locked on a validator set updating block, proposers must not propose a `Nudge`
//! containing the highest PC, since the [4th predicate of `safe_nudge`](safe_nudge#conditional-checks)
//! wil prevent honest replicas from voting on it, and hence prevent a quorum for the `Nudge` from being
//! formed.
//!
//! To make progress in this situation, a proposer must re-propose either the locked block, or a
//! (possibly new) sibling of the locked block. The implementation in HotStuff-rs chooses to do the
//! former: the [`repropose_block`] method in this module helps determine whether a proposer should
//! re-propose a block by considering its `current_view` and the local block tree's `highest_view.pc`,
//! and if it finds that it *should* re-propose a block, returns the hash of the block that should
//! be re-proposed so that the proposer can get it from the block tree.

use crate::{
    hotstuff::{
        messages::Nudge,
        types::{Phase, PhaseCertificate},
    },
    types::{
        block::Block,
        data_types::{ChainID, CryptoHash, ViewNumber},
    },
};

use super::{
    accessors::internal::{BlockTreeError, BlockTreeSingleton},
    pluggables::KVStore,
};

/// Check whether `block` can safely cause updates to `block_tree`, given the replica's `chain_id`.
///
/// # Conditional checks
///
/// `safe_block` returns `true` in case all of the following predicates are `true`:
/// 1. `safe_pc(&block.justify, block_tree, chain_id)`.
/// 2. `block.pc.phase` is either `Generic` or `Decide`.
///
/// # Precondition
///
/// [`is_correct`](Block::is_correct) is `true` for `block`.
pub(crate) fn safe_block<K: KVStore>(
    block: &Block,
    block_tree: &BlockTreeSingleton<K>,
    chain_id: ChainID,
) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */
        safe_pc(&block.justify, block_tree, chain_id)? &&
        /* 2 */ block.justify.is_block_justify(),
    )
}

/// Check whether `pc` can safely cause updates to `block_tree`, given the replica's `chain_id`.
///
/// # Conditional checks
///
/// `safe_pc` returns `true` in case all of the following predicates are `true`:
/// 1. Either `pc.chain_id` equals `chain_id`, or `pc` is the Genesis PC.
/// 2. Either `block_tree` contains `pc.block`, or `pc` is the Genesis PC.
/// 3. Either `pc.view` is (strictly) greater than `block_tree`'s `locked_pc.view`, or `pc.block` extends
///    from `locked_pc.block`.
/// 4. If `pc.phase` is `Prepare`, `Precommit`, `Commit`, or `Decide`, `pc.block` is a validator set
///    updating block. Else, if `pc.phase` is `Generic`, `pc.block` is not a validator set updating
///    block.
///
/// # Precondition
///
/// [`is_correct`](crate::types::signed_messages::Certificate::is_correct) is `true` for `block.justify`.
pub(crate) fn safe_pc<K: KVStore>(
    pc: &PhaseCertificate,
    block_tree: &BlockTreeSingleton<K>,
    chain_id: ChainID,
) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */
        (pc.chain_id == chain_id || pc.is_genesis_pc()) &&
        /* 2 */ (block_tree.contains(&pc.block) || pc.is_genesis_pc()) &&
        /* 3 */ (pc.view > block_tree.locked_pc()?.view || extends_locked_pc_block(pc, block_tree)?) &&
        /* 4 */ (((pc.phase.is_prepare() || pc.phase.is_precommit() || pc.phase.is_commit() || pc.phase.is_decide()) &&
                   block_tree.validator_set_updates_status(&pc.block)?.contains_updates()) ||
        /* 4 (else) */ (pc.phase.is_generic() && !block_tree.validator_set_updates_status(&pc.block)?.contains_updates())),
    )
}

/// Check whether `nudge` can safely cause updates to `block_tree`, given the replica's `current_view`
/// and `chain_id`.
///
/// # Conditional checks
///
/// `safe_nudge` returns `true` in case all of the following predicates are `true`:
/// 1. `safe_pc(&nudge.justify, block_tree, chain_id)`.
/// 2. `nudge.justify.phase` is `Prepare`, `Precommit`, or `Commit`.
/// 3. `nudge.chain_id` equals `chain_id`.
/// 4. `nudge.justify.phase` is either `Commit`, or `nudge.justify.view = current_view - 1`.
///
/// # Precondition
///
/// [`is_correct`](crate::types::signed_messages::Certificate::is_correct) is `true` for `nudge.justify`.
pub fn safe_nudge<K: KVStore>(
    nudge: &Nudge,
    current_view: ViewNumber,
    block_tree: &BlockTreeSingleton<K>,
    chain_id: ChainID,
) -> Result<bool, BlockTreeError> {
    Ok(
        /* 1 */
        safe_pc(&nudge.justify, block_tree, chain_id)? &&
        /* 2 */ nudge.justify.is_nudge_justify() &&
        /* 3 */ (nudge.chain_id == chain_id) &&
        /* 4 */ (nudge.justify.phase.is_commit() || nudge.justify.view == current_view - 1),
    )
}

/// Get the PC (if any) that should be set as the Locked PC after the replica sees the given `justify`.
///
/// # Precondition
///
/// The block or nudge containing this justify must satisfy [`safe_block`] or [`safe_nudge`]
/// respectively.
///
/// # `pc_to_lock` logic
///
/// `pc_to_lock`'s return value depends on `justify`'s phase and whether or not it is the Genesis PC.
/// What `pc_to_lock` returns in every case is summarized in the below table:
///
/// |`justify` is the/a|PC to lock if not already the current locked PC|
/// |---|---|
/// |Genesis PC|`None`|
/// |`Generic` PC|`justify.block.justify`|
/// |`Prepare` PC|`None`|
/// |`Precommit` PC|`justify`|
/// |`Commit` PC|`justify`|
/// |`Decide` PC|`justify`|
///
/// # Rationale
///
/// The rationale behind `pc_to_lock`'s logic is explained in the ["Locking"](super::invariants#locking)
/// module-level docs.
pub(crate) fn pc_to_lock<K: KVStore>(
    justify: &PhaseCertificate,
    block_tree: &BlockTreeSingleton<K>,
) -> Result<Option<PhaseCertificate>, BlockTreeError> {
    // Special case: if `justify` is the Genesis PC, there is no PC to lock.
    if justify.is_genesis_pc() {
        return Ok(None);
    }

    // Determine the `PhaseCertificate` to lock according to `justify.phase`.
    let locked_pc = block_tree.locked_pc()?;
    let pc_to_lock = match justify.phase {
        // If `justify.phase` is `Generic`, lock on `justify.block.justify`.
        Phase::Generic => {
            let parent_justify = block_tree.block_justify(&justify.block)?;
            Some(parent_justify.clone())
        }

        // If `justify.phase` is `Prepare`, don't lock.
        Phase::Prepare => None,

        // If `justify.phase` is `Precommit`, lock on `justify`.
        Phase::Precommit => Some(justify.clone()),

        // If `justify.phase` is `Commit` or `Decide`, *and* `locked_pc.block != justify.block`, lock on
        // `justify`. The second condition is to prevent redundant locking (e.g., once we've locked on a
        // `Precommit` PC justifying a block, we don't have to lock on a `Commit` or `Decide` PC justifying
        // the same block).
        Phase::Commit | Phase::Decide => {
            if locked_pc.block != justify.block {
                Some(justify.clone())
            } else {
                None
            }
        }
    };

    // Prevent redundant locking by returning Some only if the PC to lock is not already the locked PC.
    if pc_to_lock.as_ref().is_some_and(|pc| pc != &locked_pc) {
        Ok(pc_to_lock)
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
/// # `block_to_commit` logic
///
/// `block_to_commit`'s return value depends on `justify`'s phase and whether or not it is the Genesis
/// PC. What `block_to_commit` returns in every case is summarized in the below table:
///
/// |`justify.phase` is a|Block to commit if it satisfies the consecutive views rule *and* is not already committed yet|
/// |---|---|
/// |`Generic` PC|`justify.block.justify.block.justify.block`|
/// |`Prepare` PC|`None`|
/// |`Precommit` PC|`None`|
/// |`Commit` PC|`justify.block`|
/// |`Decide` PC|`justify.block`|
///
/// # Rationale
///
/// The rationale behind `block_to_commit`'s logic is explained in the
/// ["Committing"](super::invariants#committing) section of `safety`'s module-level docs.
pub(crate) fn block_to_commit<K: KVStore>(
    justify: &PhaseCertificate,
    block_tree: &BlockTreeSingleton<K>,
) -> Result<Option<CryptoHash>, BlockTreeError> {
    // Special case: if `justify` is the Genesis PC, there is no block to commit.
    if justify.is_genesis_pc() {
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
            if parent_justify.is_genesis_pc() {
                return Ok(None);
            };
            let grandparent_justify = block_tree.block_justify(&parent_justify.block)?;
            if grandparent_justify.is_genesis_pc() {
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

/// Get the `Block` in the `block_tree` which a leader of the `current_view` should re-propose in order
/// to satisfy the Consecutive Views Rule and make progress in the view.
///
/// # Usage
///
/// If `Ok(Some(block_hash))` is returned, then the leader should re-propose the block identified by
/// `block_hash`.
///
/// Else if `Ok(None)` is returned, then the leader should either propose a new block, or
/// nudge using the highest pc.
///
/// # Rationale
///
/// The Consecutive Views Rule and the purpose of `repropose_block` is explained in the
/// ["Committing"](super::invariants#committing) section of `safety`'s module-level docs.
pub(crate) fn repropose_block<K: KVStore>(
    current_view: ViewNumber,
    block_tree: &BlockTreeSingleton<K>,
) -> Result<Option<CryptoHash>, BlockTreeError> {
    let highest_pc = block_tree.highest_pc()?;
    match highest_pc.phase {
        Phase::Prepare | Phase::Precommit if current_view != highest_pc.view + 1 => {
            Ok(Some(highest_pc.block))
        }
        _ => Ok(None),
    }
}

/// Check whether `pc` belongs to the branch that extends from the `block_tree`'s `locked_pc.block`.
///
/// # Procedure
///
/// In the phased mode, it is always the latest block that is locked, while in the pipelined mode, it
/// is the grandparent of the newest block that is locked. Therefore, to see whether `pc` extends from
/// `locked_pc.block`, we only need to check whether or not `locked_pc.block` is `pc.block`, or its
/// parent, or its grandparent. We do not need to check deeper.
fn extends_locked_pc_block<K: KVStore>(
    pc: &PhaseCertificate,
    block_tree: &BlockTreeSingleton<K>,
) -> Result<bool, BlockTreeError> {
    let locked_pc = block_tree.locked_pc()?;
    let block = pc.block;
    let block_parent = block_tree.block_justify(&block).ok().map(|pc| pc.block);
    let block_grandparent = block_parent
        .map(|b| block_tree.block_justify(&b).ok().map(|pc| pc.block))
        .flatten();
    Ok(block == locked_pc.block
        || block_parent.is_some_and(|b| b == locked_pc.block)
        || block_grandparent.is_some_and(|b| b == locked_pc.block))
}
