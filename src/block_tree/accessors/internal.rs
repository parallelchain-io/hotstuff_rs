//! Internal read-and-write handle used by the algorithm thread to mutate the Block Tree.
//!
//! # Initializing the Block Tree
//!
//! All variables in the Block Tree start out empty except eight. These eight variables, which must be
//! initialized using the [`initialize`](BlockTree::initialize) function before doing anything else with
//! the Block Tree, are:
//!
//! |Variable|Initial value|
//! |---|---|
//! |Committed App State|Provided to [`initialize`](crate::replica::Replica::initialize).|
//! |Committed Validator Set|Provided to [`initialize`](crate::replica::Replica::initialize).|
//! |Previous Validator Set|Provided to [`initialize`](crate::replica::Replica::initialize).|
//! |Validator Set Update Block Height|Provided to [`initialize`](crate::replica::Replica::initialize).|
//! |Validator Set Update Complete|Provided to [`initialize`](crate::replica::Replica::initialize).|
//! |Locked PC|The [Genesis PC](crate::hotstuff::types::PhaseCertificate::genesis_pc)|
//! |Highest View Entered|0|
//! |Highest Phase Certificate|The [Genesis PC](crate::hotstuff::types::PhaseCertificate::genesis_pc)|
//!
//! # Mutating the Block Tree directly from user code
//!
//! In normal operation, HotStuff-rs code will internally be making all writes to the
//! `BlockTreeSingleton`, while users can get a `BlockTreeCamera` through which they can read from the
//! block tree by calling `Replica`'s [`block_tree_camera`](crate::replica::Replica::block_tree_camera)
//! method.
//!
//! Sometimes, however, users may want to manually mutate the Block Tree, for example, to recover from
//! an error that has corrupted some of its invariants. For this purpose, one can unsafe-ly get an
//! instance of BlockTree using [`BlockTree::new_unsafe`] and an instance of the corresponding
//! [`BlockTreeWriteBatch`] using [`BlockTreeWriteBatch::new_unsafe`].

use std::{cmp::max, iter::successors, sync::mpsc::Sender, time::SystemTime};

use borsh::BorshSerialize;

use crate::{
    events::{
        CommitBlockEvent, Event, PruneBlockEvent, UpdateHighestPCEvent, UpdateLockedPCEvent,
        UpdateValidatorSetEvent,
    },
    hotstuff::types::PhaseCertificate,
    pacemaker::types::TimeoutCertificate,
    types::{
        block::Block,
        data_types::{BlockHeight, ChildrenList, CryptoHash, Data, DataLen, Datum, ViewNumber},
        update_sets::{AppStateUpdates, ValidatorSetUpdates},
        validator_set::{
            ValidatorSet, ValidatorSetBytes, ValidatorSetState, ValidatorSetUpdatesStatus,
            ValidatorSetUpdatesStatusBytes,
        },
    },
};

use super::super::{
    invariants,
    pluggables::{KVGetError, KVStore, Key, WriteBatch},
    variables::{self, concat},
};

use super::{app::AppBlockTreeView, public::BlockTreeSnapshot};

/// Read and write handle into the block tree that should be owned exclusively by the algorithm thread.
///
/// ## Categories of methods
///
/// `BlockTreeSingleton` has a large number of methods. To improve understandability, these methods are grouped
/// into five categories, with methods in each separate category being defined in a separate `impl`
/// block. These five categories are:
/// 1. [Lifecycle methods](#impl-BlockTreeSingleton<K>).
/// 2. [Top-level state updaters](#impl-BlockTreeSingleton<K>-1).
/// 3. [Helper functions called by `BlockTree::update`](#impl-BlockTreeSingleton<K>-2).
/// 4. [Basic state getters](#impl-BlockTreeSingleton<K>-3).
/// 5. [Extra state getters](#impl-BlockTreeSingleton<K>-4).
pub struct BlockTreeSingleton<K: KVStore>(K);

/// Lifecycle methods.
///
/// These are methods for creating and initializing a `BlockTreeSingleton`, as well as for using it to create and
/// consume other block tree-related types, namely, [`BlockTreeSnapshot`], [`BlockTreeWriteBatch`], and
/// [`AppBlockTreeView`].
impl<K: KVStore> BlockTreeSingleton<K> {
    /// Create a new instance of `BlockTreeSingleton` on top of `kv_store`.
    ///
    /// This constructor is private (`pub(crate)`). To create an instance of `BlockTreeSingleton` as a
    /// library user, use [`new_unsafe`](Self::new_unsafe).
    pub(crate) fn new(kv_store: K) -> Self {
        BlockTreeSingleton(kv_store)
    }

    /// Create a new instance of `BlockTreeSingleton` on top of `kv_store`.
    ///
    /// ## Safety
    ///
    /// Read
    /// [mutating the block tree directly from user code](#mutating-the-block-tree-directly-from-user-code).
    pub unsafe fn new_unsafe(kv_store: K) -> Self {
        Self::new(kv_store)
    }

    /// Initialize the block tree variables listed in [initial state](#initial-state).
    ///
    /// This function must be called exactly once on a `BlockTreeSingleton` with an empty backing
    /// `kv_store`, before any of the other functions (except the constructors `new` or `new_unsafe`) are
    /// called.
    pub fn initialize(
        &mut self,
        initial_app_state: &AppStateUpdates,
        initial_validator_set_state: &ValidatorSetState,
    ) -> Result<(), BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();

        wb.apply_app_state_updates(initial_app_state);

        let committed_validator_set = initial_validator_set_state.committed_validator_set();
        let previous_validator_set = initial_validator_set_state.previous_validator_set();
        let update_height = initial_validator_set_state.update_height();
        let update_decided = initial_validator_set_state.update_decided();

        wb.set_committed_validator_set(&committed_validator_set)?;
        wb.set_previous_validator_set(&previous_validator_set)?;
        if let Some(height) = *update_height {
            wb.set_validator_set_update_block_height(height)?
        }
        wb.set_validator_set_update_decided(update_decided)?;

        wb.set_locked_pc(&PhaseCertificate::genesis_pc())?;

        wb.set_highest_view_entered(ViewNumber::init())?;

        wb.set_highest_pc(&PhaseCertificate::genesis_pc())?;

        self.write(wb);

        Ok(())
    }

    /// Create a `BlockTreeSnapshot`.
    pub fn snapshot(&self) -> BlockTreeSnapshot<K::Snapshot<'_>> {
        BlockTreeSnapshot::new(self.0.snapshot())
    }

    /// Atomically write the changes in `write_batch` into the `BlockTreeSingleton`.
    pub fn write(&mut self, write_batch: BlockTreeWriteBatch<K::WriteBatch>) {
        self.0.write(write_batch.0)
    }

    /// Create an `AppBlockTreeView` which sees the app state as it will be right after `parent` becomes
    /// committed.
    pub fn app_view<'a>(
        &'a self,
        parent: Option<&CryptoHash>,
    ) -> Result<AppBlockTreeView<'a, K>, BlockTreeError> {
        let highest_committed_block_height = self.highest_committed_block_height()?;
        let parent = match parent {
            None => None,
            Some(&b) => Some(b),
        };

        // Obtain an iterator over the ancestors starting from the parent, all the way until genesis,
        // from newest (parent) to oldest.
        let ancestors_iter = successors(parent, |b| {
            self.block_justify(b)
                .ok()
                .map(|pc| {
                    if !pc.is_genesis_pc() {
                        Some(pc.block)
                    } else {
                        None
                    }
                })
                .flatten()
        });

        let ancestors_heights_iter = ancestors_iter
            .clone()
            .map(|block| {
                self.block_height(&block).map(|res| {
                    if res.is_none() {
                        Err(BlockTreeError::BlockExpectedButNotFound {
                            block: block.clone(),
                        })
                    } else {
                        Ok(res.unwrap())
                    }
                })
            })
            .flatten()
            .flatten();

        // Obtain an iterator over the uncomitted ancestors starting from the parent,
        // ending at the lowest uncommitted ancestor.
        let uncommitted_ancestors_iter = ancestors_iter
            .zip(ancestors_heights_iter)
            .take_while(|(_, height)| {
                highest_committed_block_height.is_none()
                    || highest_committed_block_height.is_some_and(|h| height > &h)
            })
            .map(|(b, _)| b);

        // Obtain a vector of optional app state updates associated with ancestors
        // starting from the parent, ending at the oldest uncommitted ancestor.
        let pending_ancestors_app_state_updates: Vec<Option<AppStateUpdates>> =
            uncommitted_ancestors_iter
                .map(|block| self.pending_app_state_updates(&block))
                .flatten()
                .collect();

        Ok(AppBlockTreeView {
            block_tree: self,
            pending_ancestors_app_state_updates,
        })
    }
}

/// Top-level state updaters.
///
/// These are the methods that mutate the block tree that are called directly by code in the
/// subprotocols (i.e., [`hotstuff`](crate::hotstuff), [`block_sync`](crate::block_sync), and
/// [`pacemaker`](crate::pacemaker)). Mutating methods outside of this `impl` and the lifecycle methods
/// `impl` above are only used internally in this module.
impl<K: KVStore> BlockTreeSingleton<K> {
    /// Insert into the block tree a `block` that will cause the provided `app_state_updates` and
    /// `validator_set_updates` to be applied when it is committed in the future.
    ///
    /// ## Relationship with `update`
    ///
    /// `insert` does not internally call [`update`](Self::update). Calling code is responsible for
    /// calling `update` on `block.justify` after calling `insert`.
    ///
    /// ## Precondition
    ///
    /// [`safe_block`](invariants::safe_block) is `true` for `block`.
    pub fn insert(
        &mut self,
        block: &Block,
        app_state_updates: Option<&AppStateUpdates>,
        validator_set_updates: Option<&ValidatorSetUpdates>,
    ) -> Result<(), BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();

        // Set block, which entails setting block's fields in separate key-value pairs.
        wb.set_block(block)?;

        // Set the block as the newest inserted block.
        wb.set_newest_block(&block.hash)?;

        // Insert the block's pending app state updates and validator set updates.
        if let Some(app_state_updates) = app_state_updates {
            wb.set_pending_app_state_updates(&block.hash, app_state_updates)?;
        }
        if let Some(validator_set_updates) = validator_set_updates {
            wb.set_pending_validator_set_updates(&block.hash, validator_set_updates)?;
        }

        // Mark the block as a child of its parent block.
        let mut siblings = self
            .children(&block.justify.block)
            .unwrap_or(ChildrenList::default());
        siblings.push(block.hash);
        wb.set_children(&block.justify.block, &siblings)?;

        // Atomically write the above changes to persistent storage.
        self.write(wb);

        Ok(())
    }

    /// Update the block tree upon seeing a safe `justify` in a [`Nudge`](crate::hotstuff::messages::Nudge)
    /// or a [`Block`].
    ///
    /// ## Updates
    ///
    /// Depending on the specific Phase Certificate received and the state of the Block Tree, the updates
    /// that this function performs will include:
    /// 1. Updating the Highest PC if `justify.view > highest_pc.view`.
    /// 2. Updating the Locked PC if appropriate, as determined by the [`pc_to_lock`](invariants::pc_to_lock)
    ///    helper.
    /// 3. Committing a block and all of its ancestors if appropriate, as determined by the
    ///    [`block_to_commit`](invariants::block_to_commit) helper.
    /// 4. Marking the latest validator set updates as decided if `justify` is a Decide PC.
    ///
    /// ## Preconditions
    ///
    /// The `Block` or `Nudge` containing `justify` must satisfy [`safe_block`](invariants::safe_block) or
    /// [`safe_nudge`](invariants::safe_nudge), respectively.
    pub(crate) fn update(
        &mut self,
        justify: &PhaseCertificate,
        event_publisher: &Option<Sender<Event>>,
    ) -> Result<Option<ValidatorSetUpdates>, BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();

        let mut update_locked_pc: Option<PhaseCertificate> = None;
        let mut update_highest_pc: Option<PhaseCertificate> = None;
        let mut committed_blocks: Vec<(CryptoHash, Option<ValidatorSetUpdates>)> = Vec::new();

        // 1. Update highestPC if needed.
        if justify.view > self.highest_pc()?.view {
            wb.set_highest_pc(justify)?;
            update_highest_pc = Some(justify.clone())
        }

        // 2. Update lockedPC if needed.
        if let Some(new_locked_pc) = invariants::pc_to_lock(justify, &self)? {
            wb.set_locked_pc(&new_locked_pc)?;
            update_locked_pc = Some(new_locked_pc)
        }

        // 3. Commit block(s) if needed.
        if let Some(block) = invariants::block_to_commit(justify, &self)? {
            committed_blocks = self.commit(&mut wb, &block)?;
        }

        // 4. Set validator set updates as decided if needed.
        if justify.phase.is_decide() {
            wb.set_validator_set_update_decided(true)?
        }

        self.write(wb);

        Self::publish_update_block_tree_events(
            event_publisher,
            update_highest_pc,
            update_locked_pc,
            &committed_blocks,
        );

        // Safety: a block that updates the validator set must be followed by a block that contains a decide
        // pc. A block becomes committed immediately if its commitPC or decidePC is seen. Therefore, under normal
        // operation, at most 1 validator-set-updating block can be committed at a time.
        let resulting_vs_update = committed_blocks
            .into_iter()
            .rev()
            .find_map(|(_, validator_set_updates_opt)| validator_set_updates_opt);

        Ok(resulting_vs_update)
    }

    /// Set the highest `TimeoutCertificate` to be `tc`.
    ///
    /// ## Preconditions
    ///
    /// TODO.
    pub fn set_highest_tc(&mut self, tc: &TimeoutCertificate) -> Result<(), BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_tc(tc)?;
        self.write(wb);
        Ok(())
    }

    /// Set the highest view entered to be `view`.
    ///
    /// ## Preconditions
    ///
    /// `view >= self.highest_view_entered()`.
    pub fn set_highest_view_entered(&mut self, view: ViewNumber) -> Result<(), BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_view_entered(view)?;
        self.write(wb);
        Ok(())
    }

    /// Set the highest view phase-voted to be `view`.
    ///
    /// ## Preconditions
    ///
    /// `view >= self.highest_view_voted()`.
    pub fn set_highest_view_phase_voted(&mut self, view: ViewNumber) -> Result<(), BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_view_phase_voted(view)?;
        self.write(wb);
        Ok(())
    }
}

/// Helper functions called by [BlockTree::update].
impl<K: KVStore> BlockTreeSingleton<K> {
    /// Commit `block` and all of its ancestors, if they have not already been committed.
    ///
    /// ## Return value
    ///
    /// Returns the hashes of the newly committed blocks, along with the updates each caused to the
    /// validator set, in order from the newly committed block with the lowest height to the newly committed
    /// block with the highest height.
    ///
    /// ## Preconditions
    ///
    /// [`block_to_commit`](invariants::block_to_commit) returns `block`.
    pub fn commit(
        &mut self,
        wb: &mut BlockTreeWriteBatch<K::WriteBatch>,
        block: &CryptoHash,
    ) -> Result<Vec<(CryptoHash, Option<ValidatorSetUpdates>)>, BlockTreeError> {
        // Obtain an iterator over the "block" and its ancestors, all the way until genesis, from newest ("block") to oldest.
        let blocks_iter = successors(Some(*block), |b| {
            self.block_justify(b)
                .ok()
                .map(|pc| {
                    if !pc.is_genesis_pc() {
                        Some(pc.block)
                    } else {
                        None
                    }
                })
                .flatten()
        });

        // Newest committed block height, we do not consider the blocks from this height downwards.
        let min_height = self.highest_committed_block_height()?;

        // Obtain an iterator over the uncomitted blocks among "block" and its ancestors from oldest to newest,
        // the newest block being "block".
        // This is required because we want to commit blocks in correct order, applying updates from oldest to
        // newest.
        let uncommitted_blocks_iter = blocks_iter.take_while(|b| {
            min_height.is_none()
                || min_height.is_some_and(|h| self.block_height(b).unwrap().unwrap() > h)
        });
        let uncommitted_blocks = uncommitted_blocks_iter.collect::<Vec<CryptoHash>>();
        let uncommitted_blocks_ordered_iter = uncommitted_blocks.iter().rev();

        // Helper closure that
        // (1) commits block b, applying all related updates to the write batch,
        // (2) extends the vector of blocks committed so far (accumulator) with b together with the optional
        //     validator set updates associated with b,
        // (3) returns the extended vector of blocks committed so far (updated accumulator).
        let commit =
            |committed_blocks_res: Result<
                Vec<(CryptoHash, Option<ValidatorSetUpdates>)>,
                BlockTreeError,
            >,
             b: &CryptoHash|
             -> Result<Vec<(CryptoHash, Option<ValidatorSetUpdates>)>, BlockTreeError> {
                let mut committed_blocks = committed_blocks_res?;

                let block_height = self
                    .block_height(b)?
                    .ok_or(BlockTreeError::BlockExpectedButNotFound { block: b.clone() })?;
                // Work steps:

                // Set block at height.
                wb.set_block_at_height(block_height, b)?;

                // Delete all of block's siblings.
                self.delete_siblings(wb, b)?;

                // Apply pending app state updates.
                if let Some(pending_app_state_updates) = self.pending_app_state_updates(b)? {
                    wb.apply_app_state_updates(&pending_app_state_updates);
                    wb.delete_pending_app_state_updates(b);
                }

                // Apply pending validator set updates.
                if let ValidatorSetUpdatesStatus::Pending(validator_set_updates) =
                    self.validator_set_updates_status(b)?
                {
                    let mut committed_validator_set = self.committed_validator_set()?;
                    let previous_validator_set = committed_validator_set.clone();
                    committed_validator_set.apply_updates(&validator_set_updates);

                    wb.set_committed_validator_set(&committed_validator_set)?;
                    wb.set_previous_validator_set(&previous_validator_set)?;
                    wb.set_validator_set_update_block_height(block_height)?;
                    wb.set_validator_set_update_decided(false)?;
                    wb.set_committed_validator_set_updates(block)?;

                    committed_blocks.push((*b, Some(validator_set_updates.clone())));
                } else {
                    committed_blocks.push((*b, None));
                }

                // Update the highest committed block.
                wb.set_highest_committed_block(b)?;

                // Return the blocks committed so far together with their corresponding validator set updates.
                Ok(committed_blocks)
            };

        // Iterate over the uncommitted blocks from oldest to newest,
        // (1) applying related updates (by mutating the write batch), and
        // (2) building up the vector of committed blocks (by pushing the newely committed blocks to
        //     the accumulator vector).
        // Finally, return the accumulator.
        uncommitted_blocks_ordered_iter.fold(Ok(Vec::new()), commit)
    }

    /// Delete the "siblings" of the specified block, along with all of its associated data (e.g., pending
    /// app state updates, validator set updates).
    ///
    /// "Siblings" refer to other blocks that share the same parent as the specified block.
    ///
    /// ## Precondition
    ///
    /// `block` is in its parents' (or the genesis) children list.
    ///
    /// ## Error
    ///
    /// Returns an error if the block is not in the block tree, or if the block's parent (or genesis) does not have a
    /// children list.
    pub fn delete_siblings(
        &mut self,
        wb: &mut BlockTreeWriteBatch<K::WriteBatch>,
        block: &CryptoHash,
    ) -> Result<(), BlockTreeError> {
        let parent_or_genesis = self.block_justify(block)?.block;
        let parents_or_genesis_children = self.children(&parent_or_genesis)?;
        let siblings = parents_or_genesis_children
            .iter()
            .filter(|sib| *sib != block);
        for sibling in siblings {
            self.delete_branch(wb, sibling);
        }

        wb.set_children(&parent_or_genesis, &ChildrenList::new(vec![*block]))?;
        Ok(())
    }

    /// Deletes all data of blocks in a branch starting from (and including) a given root block.
    pub fn delete_branch(
        &mut self,
        wb: &mut BlockTreeWriteBatch<K::WriteBatch>,
        root: &CryptoHash,
    ) {
        for block in self.blocks_in_branch(*root) {
            wb.delete_children(&block);
            wb.delete_pending_app_state_updates(&block);
            wb.delete_block_validator_set_updates(&block);

            if let Ok(Some(data_len)) = self.block_data_len(&block) {
                wb.delete_block(&block, data_len)
            }
        }
    }

    /// Perform depth-first search to collect the hashes of all blocks in the branch rooted at `root` into
    /// a single iterator.
    pub fn blocks_in_branch(&self, root: CryptoHash) -> impl Iterator<Item = CryptoHash> {
        let mut stack: Vec<CryptoHash> = vec![root];
        let mut branch: Vec<CryptoHash> = vec![];

        while let Some(block) = stack.pop() {
            if let Ok(children) = self.children(&block) {
                for child in children.iter() {
                    stack.push(*child)
                }
            };
            branch.push(block)
        }
        branch.into_iter()
    }

    /// Publish all events resulting from calling [`update`](Self::update). These events have to do with
    /// changing persistent state, and  possibly include: [`UpdateHighestPCEvent`], [`UpdateLockedPCEvent`],
    /// [`PruneBlockEvent`], [`CommitBlockEvent`], [`UpdateValidatorSetEvent`].
    ///
    /// Invariant: this method must only be invoked after the associated changes are persistently written to
    /// the [`BlockTreeSingleton`].
    fn publish_update_block_tree_events(
        event_publisher: &Option<Sender<Event>>,
        update_highest_pc: Option<PhaseCertificate>,
        update_locked_pc: Option<PhaseCertificate>,
        committed_blocks: &Vec<(CryptoHash, Option<ValidatorSetUpdates>)>,
    ) {
        if let Some(highest_pc) = update_highest_pc {
            Event::UpdateHighestPC(UpdateHighestPCEvent {
                timestamp: SystemTime::now(),
                highest_pc: highest_pc,
            })
            .publish(event_publisher)
        };

        if let Some(locked_pc) = update_locked_pc {
            Event::UpdateLockedPC(UpdateLockedPCEvent {
                timestamp: SystemTime::now(),
                locked_pc,
            })
            .publish(event_publisher)
        };

        committed_blocks
            .iter()
            .for_each(|(b, validator_set_updates_opt)| {
                Event::PruneBlock(PruneBlockEvent {
                    timestamp: SystemTime::now(),
                    block: b.clone(),
                })
                .publish(event_publisher);
                Event::CommitBlock(CommitBlockEvent {
                    timestamp: SystemTime::now(),
                    block: b.clone(),
                })
                .publish(event_publisher);
                if let Some(validator_set_updates) = validator_set_updates_opt {
                    Event::UpdateValidatorSet(UpdateValidatorSetEvent {
                        timestamp: SystemTime::now(),
                        cause_block: *b,
                        validator_set_updates: validator_set_updates.clone(),
                    })
                    .publish(event_publisher);
                }
            });
    }
}

/// "Basic" state getters.
///
/// Each basic state getter calls a corresponding provided method of [`KVGet`](super::kv_store::KVGet) and
/// return whatever they return.
///
/// The exact same set of basic state getters are also defined on `BlockTreeSnapshot`.
impl<K: KVStore> BlockTreeSingleton<K> {
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

    pub fn highest_tc(&self) -> Result<Option<TimeoutCertificate>, BlockTreeError> {
        Ok(self.0.highest_tc()?)
    }

    pub fn validator_set_state(&self) -> Result<ValidatorSetState, BlockTreeError> {
        Ok(self.0.validator_set_state()?)
    }

    pub fn highest_view_voted(&self) -> Result<Option<ViewNumber>, BlockTreeError> {
        Ok(self.0.highest_view_phase_voted()?)
    }
}

/// "Extra" state getters.
///
/// Extra state getters call [basic state getters](#impl-BlockTree<K>-3) and aggregate or modify what
/// they return into forms that are more convenient to use.
///
/// Unlike basic state getters, these functions are not defined on `BlockTreeSnapshot`.
impl<K: KVStore> BlockTreeSingleton<K> {
    /// Check whether `block` exists on the block tree.
    pub fn contains(&self, block: &CryptoHash) -> bool {
        self.block(block).is_ok_and(|block_opt| block_opt.is_some())
    }

    /// Get the maximum of:
    /// - [`self.highest_view_entered()`](Self::highest_view_entered).
    /// - [`self.highest_pc()`](Self::highest_pc).
    /// - [`self.highest_tc()`](Self::highest_tc).
    ///
    /// This is useful for deciding which view to initially enter after starting or restarting a replica.
    pub fn highest_view_with_progress(&self) -> Result<ViewNumber, BlockTreeError> {
        Ok(max(
            self.highest_view_entered()?,
            max(
                self.highest_pc()?.view,
                self.highest_tc()?
                    .map(|tc| tc.view)
                    .unwrap_or(ViewNumber::init()),
            ),
        ))
    }

    /// Get the height of the highest committed block.
    pub fn highest_committed_block_height(&self) -> Result<Option<BlockHeight>, BlockTreeError> {
        let highest_committed_block = self.highest_committed_block()?;
        if let Some(block) = highest_committed_block {
            Ok(self.block_height(&block)?)
        } else {
            Ok(None)
        }
    }
}

/// Errors that may be encountered when reading or writing to the [`BlockTreeSingleton`].
#[derive(Debug)]
pub enum BlockTreeError {
    /// Error when trying to get a value from the block tree's underlying [key value store][KVStore].
    KVGetError(KVGetError),

    /// Error when trying set a value into block tree's underlying key value store.
    KVSetError(KVSetError),

    /// Unable to find a block with the specific `CryptoHash`, even though an invariant that the block tree
    /// expects to be maintained suggests that the block should exist.
    BlockExpectedButNotFound { block: CryptoHash },
}

impl From<KVGetError> for BlockTreeError {
    fn from(value: KVGetError) -> Self {
        BlockTreeError::KVGetError(value)
    }
}

impl From<KVSetError> for BlockTreeError {
    fn from(value: KVSetError) -> Self {
        BlockTreeError::KVSetError(value)
    }
}

pub struct BlockTreeWriteBatch<W: WriteBatch>(pub(super) W);

impl<W: WriteBatch> BlockTreeWriteBatch<W> {
    pub(crate) fn new() -> BlockTreeWriteBatch<W> {
        BlockTreeWriteBatch(W::new())
    }

    pub fn new_unsafe() -> BlockTreeWriteBatch<W> {
        Self::new()
    }

    /* ↓↓↓ Block ↓↓↓  */

    pub fn set_block(&mut self, block: &Block) -> Result<(), BlockTreeError> {
        let block_prefix = concat(&variables::BLOCKS, &block.hash.bytes());

        self.0.set(
            &concat(&block_prefix, &variables::BLOCK_HEIGHT),
            &block
                .height
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::BlockHeight {
                        block: block.hash.clone(),
                    },
                    source: err,
                })?,
        );
        self.0.set(
            &concat(&block_prefix, &variables::BLOCK_JUSTIFY),
            &block
                .justify
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::BlockJustify {
                        block: block.hash.clone(),
                    },
                    source: err,
                })?,
        );
        self.0.set(
            &concat(&block_prefix, &variables::BLOCK_DATA_HASH),
            &block
                .data_hash
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::BlockDataHash {
                        block: block.hash.clone(),
                    },
                    source: err,
                })?,
        );
        self.0.set(
            &concat(&block_prefix, &variables::BLOCK_DATA_LEN),
            &block
                .data
                .len()
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::BlockDataLength {
                        block: block.hash.clone(),
                    },
                    source: err,
                })?,
        );

        // Insert datums.
        let block_data_prefix = concat(&block_prefix, &variables::BLOCK_DATA);
        for (i, datum) in block.data.iter().enumerate() {
            let datum_key = concat(
                &block_data_prefix,
                &(i as u32)
                    .try_to_vec()
                    .map_err(|err| KVSetError::SerializeValueError {
                        key: Key::BlockData {
                            block: block.hash.clone(),
                        },
                        source: err,
                    })?,
            );
            self.0.set(&datum_key, datum.bytes());
        }

        Ok(())
    }

    pub fn delete_block(&mut self, block: &CryptoHash, data_len: DataLen) {
        let block_prefix = concat(&variables::BLOCKS, &block.bytes());

        self.0
            .delete(&concat(&block_prefix, &variables::BLOCK_HEIGHT));
        self.0
            .delete(&concat(&block_prefix, &variables::BLOCK_JUSTIFY));
        self.0
            .delete(&concat(&block_prefix, &variables::BLOCK_DATA_HASH));
        self.0
            .delete(&concat(&block_prefix, &variables::BLOCK_DATA_LEN));

        let block_data_prefix = concat(&block_prefix, &variables::BLOCK_DATA);
        for i in 0..data_len.int() {
            let datum_key = concat(&block_data_prefix, &i.try_to_vec().unwrap());
            self.0.delete(&datum_key);
        }
    }

    /* ↓↓↓ Block at Height ↓↓↓ */

    pub fn set_block_at_height(
        &mut self,
        height: BlockHeight,
        block: &CryptoHash,
    ) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &concat(&variables::BLOCK_AT_HEIGHT, &height.try_to_vec().unwrap()),
            &block
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::BlockAtHeight { height },
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Block to Children ↓↓↓ */

    pub fn set_children(
        &mut self,
        block: &CryptoHash,
        children: &ChildrenList,
    ) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &concat(&variables::BLOCK_TO_CHILDREN, &block.bytes()),
            &children
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::BlockChildren {
                        block: block.clone(),
                    },
                    source: err,
                })?,
        ))
    }

    pub fn delete_children(&mut self, block: &CryptoHash) {
        self.0
            .delete(&concat(&variables::BLOCK_TO_CHILDREN, &block.bytes()));
    }

    /* ↓↓↓ Committed App State ↓↓↓ */

    pub fn set_committed_app_state(&mut self, key: &[u8], value: &[u8]) {
        self.0
            .set(&concat(&variables::COMMITTED_APP_STATE, key), value);
    }

    pub fn delete_committed_app_state(&mut self, key: &[u8]) {
        self.0.delete(&concat(&variables::COMMITTED_APP_STATE, key));
    }

    /* ↓↓↓ Pending App State Updates ↓↓↓ */

    pub fn set_pending_app_state_updates(
        &mut self,
        block: &CryptoHash,
        app_state_updates: &AppStateUpdates,
    ) -> Result<(), KVSetError> {
        Ok(self.0.set(
            &concat(&variables::PENDING_APP_STATE_UPDATES, &block.bytes()),
            &app_state_updates
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::PendingAppStateUpdates {
                        block: block.clone(),
                    },
                    source: err,
                })?,
        ))
    }

    pub fn apply_app_state_updates(&mut self, app_state_updates: &AppStateUpdates) {
        for (key, value) in app_state_updates.inserts() {
            self.set_committed_app_state(key, value);
        }

        for key in app_state_updates.deletes() {
            self.delete_committed_app_state(key);
        }
    }

    pub fn delete_pending_app_state_updates(&mut self, block: &CryptoHash) {
        self.0.delete(&concat(
            &variables::PENDING_APP_STATE_UPDATES,
            &block.bytes(),
        ));
    }

    /* ↓↓↓ Commmitted Validator Set */

    pub fn set_committed_validator_set(
        &mut self,
        validator_set: &ValidatorSet,
    ) -> Result<(), BlockTreeError> {
        let validator_set_bytes: ValidatorSetBytes = validator_set.into();
        Ok(self.0.set(
            &variables::COMMITTED_VALIDATOR_SET,
            &validator_set_bytes
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::CommittedValidatorSet,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Pending Validator Set Updates */

    pub fn set_pending_validator_set_updates(
        &mut self,
        block: &CryptoHash,
        validator_set_updates: &ValidatorSetUpdates,
    ) -> Result<(), BlockTreeError> {
        let block_vs_updates_bytes =
            ValidatorSetUpdatesStatusBytes::Pending(validator_set_updates.into());
        Ok(self.0.set(
            &concat(&variables::VALIDATOR_SET_UPDATES_STATUS, &block.bytes()),
            &block_vs_updates_bytes.try_to_vec().map_err(|err| {
                KVSetError::SerializeValueError {
                    key: Key::ValidatorSetUpdatesStatus {
                        block: block.clone(),
                    },
                    source: err,
                }
            })?,
        ))
    }

    pub fn set_committed_validator_set_updates(
        &mut self,
        block: &CryptoHash,
    ) -> Result<(), BlockTreeError> {
        let block_vs_updates_bytes = ValidatorSetUpdatesStatusBytes::Committed;
        Ok(self.0.set(
            &concat(&variables::VALIDATOR_SET_UPDATES_STATUS, &block.bytes()),
            &block_vs_updates_bytes.try_to_vec().map_err(|err| {
                KVSetError::SerializeValueError {
                    key: Key::ValidatorSetUpdatesStatus {
                        block: block.clone(),
                    },
                    source: err,
                }
            })?,
        ))
    }

    pub fn delete_block_validator_set_updates(&mut self, block: &CryptoHash) {
        self.0.delete(&concat(
            &variables::VALIDATOR_SET_UPDATES_STATUS,
            &block.bytes(),
        ))
    }

    /* ↓↓↓ Locked PC ↓↓↓ */

    pub fn set_locked_pc(&mut self, pc: &PhaseCertificate) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &variables::LOCKED_PC,
            &pc.try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::LockedPC,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Highest View Entered ↓↓↓ */

    pub fn set_highest_view_entered(&mut self, view: ViewNumber) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &variables::HIGHEST_VIEW_ENTERED,
            &view
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::HighestTC,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Highest Phase Certificate ↓↓↓ */

    pub fn set_highest_pc(&mut self, pc: &PhaseCertificate) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &variables::HIGHEST_PC,
            &pc.try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::HighestTC,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Highest Committed Block ↓↓↓ */

    pub fn set_highest_committed_block(
        &mut self,
        block: &CryptoHash,
    ) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &variables::HIGHEST_COMMITTED_BLOCK,
            &block
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::HighestCommittedBlock,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Newest Block ↓↓↓ */

    pub fn set_newest_block(&mut self, block: &CryptoHash) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &variables::NEWEST_BLOCK,
            &block
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::NewestBlock,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Highest Timeout Certificate ↓↓↓ */

    pub fn set_highest_tc(&mut self, tc: &TimeoutCertificate) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &variables::HIGHEST_TC,
            &tc.try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::HighestTC,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Previous Validator Set  ↓↓↓ */
    pub fn set_previous_validator_set(
        &mut self,
        validator_set: &ValidatorSet,
    ) -> Result<(), BlockTreeError> {
        let validator_set_bytes: ValidatorSetBytes = validator_set.into();
        Ok(self.0.set(
            &variables::PREVIOUS_VALIDATOR_SET,
            &validator_set_bytes
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::PreviousValidatorSet,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Validator Set Update Block Height ↓↓↓ */
    pub fn set_validator_set_update_block_height(
        &mut self,
        height: BlockHeight,
    ) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &variables::VALIDATOR_SET_UPDATE_BLOCK_HEIGHT,
            &height
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::ValidatorSetUpdateHeight,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Validator Set Update Decided ↓↓↓ */

    pub fn set_validator_set_update_decided(
        &mut self,
        update_complete: bool,
    ) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &variables::VALIDATOR_SET_UPDATE_DECIDED,
            &update_complete
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::ValidatorSetUpdateDecided,
                    source: err,
                })?,
        ))
    }

    /* ↓↓↓ Highest View Phase-Voted ↓↓↓ */

    pub fn set_highest_view_phase_voted(&mut self, view: ViewNumber) -> Result<(), BlockTreeError> {
        Ok(self.0.set(
            &variables::HIGHEST_VIEW_PHASE_VOTED,
            &view
                .try_to_vec()
                .map_err(|err| KVSetError::SerializeValueError {
                    key: Key::HighestViewPhaseVoted,
                    source: err,
                })?,
        ))
    }
}

/// Error when writing a key-value pair to the [write batch][BlockTreeWriteBatch].
/// The error may arise when the value cannot be serialized, and hence cannot be
/// written to the write batch.
#[derive(Debug)]
pub enum KVSetError {
    SerializeValueError { key: Key, source: std::io::Error },
}
