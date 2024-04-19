/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Types and methods used to access and mutate the persistent state that a replica keeps track for the
//! operation of the protocol, and for its application.
//!
//! This state may be stored in any key-value store of the library user's own choosing, as long as that
//! KV store can provide a type that implements [KVStore]. This state can be mutated through an instance
//! of [BlockTree], and read through an instance of [BlockTreeSnapshot], which can be created using
//! [BlockTreeCamera].
//!
//! In normal operation, HotStuff-rs code will internally be making all writes to the 
//! [Block Tree](crate::state::BlockTree), and users can get a [BlockTreeCamera] using replica's 
//! [block_tree_camera](crate::replica::Replica::block_tree_camera) method.
//!
//! Sometimes, however, users may want to manually mutate the Block Tree, for example, to recover from
//! an error that has corrupted its invariants. For this purpose, one can unsafe-ly get an instance of 
//! BlockTree using [BlockTree::new_unsafe] and an instance of the corresponding [BlockTreeWriteBatch]
//! using [BlockTreeWriteBatch::new_unsafe].
//!
//! ## State variables
//!
//! HotStuff-rs structures its state into separate conceptual 'variables' which are stored in tuples
//! that sit at a particular key path or prefix in the library user's chosen KV store. These variables are:
//! 
//! |Variable|"Type"|Description|
//! |---|---|---|
//! |Blocks|[CryptoHash] -> [Block]||
//! |Block at Height|[BlockHeight] -> [CryptoHash]|A mapping between a block's number and a block's hash. This mapping only contains blocks that are committed, because if a block hasn't been committed, there may be multiple blocks at the same height.|
//! |Block to Children|[CryptoHash] -> [ChildrenList]|A mapping between a block's hash and the children it has in the block tree. A block may have multiple chilren if they have not been committed.|
//! |Committed App State|[Vec<u8>] -> [Vec<u8>]||
//! |Pending App State Updates|[CryptoHash] -> [AppStateUpdates]||
//! |Locked View|[ViewNumber]|The highest view number of a quorum certificate contained in a block that has a child.|
//! |Highest Voted View|[ViewNumber]|The highest view that this validator has voted in.|
//! |Highest Quorum Certificate|[QuorumCertificate]|Among the quorum certificates this validator has seen and verified the signatures of, the one with the highest view number.|
//! |Highest Committed Block|[CryptoHash]|The hash of the committed block that has the highest height.|
//! |Newest BlocK|[CryptoHash]The hash of the most recent block to be inserted into the block tree.|
//!
//! The location of each of these variables in a KV store is defined in [paths]. Note that the fields of a
//! block are itself stored in different tuples. This is so that user code can get a subset of a block's data
//! without loading the entire block from storage (which can be expensive). The key suffixes on which each of
//! block's fields are stored are also defined in paths.
//!
//! ## Initial state
//!
//! All variables in the Block Tree start out empty except five. These five variables are:
//! 
//! |Variable|Initial value|
//! |---|---|
//! |Committed App State|Provided to [`Replica::initialize`](crate::replica::Replica::initialize).|
//! |Committed Validator Set|Provided to [`Replica::initialize`](crate::replica::Replica::initialize).|
//! |Locked View|0|
//! |Highest View Entered|0|
//! |Highest Quorum Certificate|The [genesis QC](crate::types::QuorumCertificate::genesis_qc)|

use std::cmp::max;
use std::iter::successors;
use std::sync::mpsc::Sender;
use std::time::SystemTime;

use crate::events::{Event, InsertBlockEvent, CommitBlockEvent, PruneBlockEvent, UpdateHighestQCEvent, UpdateLockedViewEvent, UpdateValidatorSetEvent};
use crate::pacemaker::types::TimeoutCertificate;
use crate::types::basic::{ChildrenList, CryptoHash, ViewNumber};
use crate::types::*;
use crate::hotstuff::types::{QuorumCertificate, Phase};

use self::basic::{AppStateUpdates, BlockHeight, Data, DataLen, Datum};
use self::block::Block;
use self::validators::{ValidatorSet, ValidatorSetState, ValidatorSetUpdates};

use super::app_block_tree_view::AppBlockTreeView;
use super::block_tree_camera::BlockTreeSnapshot;
use super::kv_store::{KVGetError, KVStore};
use super::write_batch::{BlockTreeWriteBatch, KVSetError};

/// A read and write handle into the block tree exclusively owned by the algorithm thread.
pub struct BlockTree<K: KVStore>(pub(super) K);

impl<K: KVStore> BlockTree<K> {
    pub(crate) fn new(kv_store: K) -> Self {
        BlockTree(kv_store)
    }

    pub unsafe fn new_unsafe(kv_store: K) -> Self {
        Self::new(kv_store)
    }

    /* ↓↓↓ Initialize ↓↓↓ */

    pub fn initialize(
        &mut self,
        initial_app_state: &AppStateUpdates,
        initial_validator_set: &ValidatorSetUpdates,
    ) -> Result<(), BlockTreeError>{
        let mut wb = BlockTreeWriteBatch::new();

        wb.apply_app_state_updates(initial_app_state);

        let mut validator_set = ValidatorSet::new();
        validator_set.apply_updates(initial_validator_set);
        wb.set_committed_validator_set(&validator_set)?;

        wb.set_locked_qc(&QuorumCertificate::genesis_qc())?;

        wb.set_highest_view_entered(ViewNumber::init())?;

        wb.set_highest_qc(&QuorumCertificate::genesis_qc())?;

        Ok(self.write(wb))
    }

    /// Insert a block, causing all of the necessary state changes, including possibly block commit, to
    /// happen.
    ///
    /// If the insertion causes a block/blocks to be committed, returns the updates that this causes to the
    /// validator set, if any.
    ///
    /// # Precondition
    /// [Self::safe_block]
    pub fn insert_block(
        &mut self,
        block: &Block,
        app_state_updates: Option<&AppStateUpdates>,
        validator_set_updates: Option<&ValidatorSetUpdates>,
        event_publisher: &Option<Sender<Event>>,
    ) -> Result<Option<ValidatorSetUpdates>, BlockTreeError> {   
        let mut wb = BlockTreeWriteBatch::new();
        let mut update_locked_view: Option<ViewNumber> = None;
        let mut update_highest_qc: Option<QuorumCertificate> = None;

        // Insert block.
        wb.set_block(block)?;
        wb.set_newest_block(&block.hash)?;
        if let Some(app_state_updates) = app_state_updates {
            wb.set_pending_app_state_updates(&block.hash, app_state_updates)?;
        }
        if let Some(validator_set_updates) = validator_set_updates {
            wb.set_pending_validator_set_updates(&block.hash, validator_set_updates)?;
        }

        let mut siblings = self
            .children(&block.justify.block)
            .unwrap_or(ChildrenList::default());
        siblings.push(block.hash);
        wb.set_children(&block.justify.block, &siblings)?;

        // Consider updating highest qc.
        if block.justify.view > self.highest_qc()?.view {
            wb.set_highest_qc(&block.justify)?;
            update_highest_qc = Some(block.justify.clone())
        }

        // If block does not ancestors, return.
        if block.justify.is_genesis_qc() {
            self.write(wb);
            publish_insert_block_events(event_publisher, block.clone(), update_highest_qc, update_locked_view, &Vec::new());
            return Ok(None);
        }

        // Otherwise, do things to ancestors according to whether the block contains a commit qc or a generic qc.
        let committed_blocks_res = match block.justify.phase {
            Phase::Generic => {
                // Consider setting locked view to parent.justify.view.
                let parent = block.justify.block;
                let parent_justify = self.block_justify(&parent)?;
                if parent_justify.view > self.locked_qc()?.view {
                    wb.set_locked_qc(&parent_justify)?;
                    update_locked_view = Some(parent_justify.view);
                }

                // Get great-grandparent.
                let great_grandparent = {
                    let parent_justify = self.block_justify(&parent).unwrap();
                    if parent_justify.is_genesis_qc() {
                        self.write(wb);
                        publish_insert_block_events(event_publisher, block.clone(), update_highest_qc, update_locked_view, &Vec::new());
                        return Ok(None);
                    }

                    let grandparent = parent_justify.block;
                    let grandparent_justify = self.block_justify(&grandparent).unwrap();
                    if grandparent_justify.is_genesis_qc() {
                        self.write(wb);
                        publish_insert_block_events(event_publisher, block.clone(), update_highest_qc, update_locked_view, &Vec::new());
                        return Ok(None);
                    }

                    grandparent_justify.block
                };

                // Commit great_grandparent if not committed yet.
                self.commit_block(&mut wb, &great_grandparent)
            }

            Phase::Commit(_) => {

                let parent = block.justify.block;

                // Commit parent if not committed yet.
                self.commit_block(&mut wb, &parent)
            }

            _ => panic!(),
        };

        self.write(wb);

        let committed_blocks = committed_blocks_res?;

        publish_insert_block_events(event_publisher, block.clone(), update_highest_qc, update_locked_view, &committed_blocks);

        /// Publish all events resulting from calling [self::insert_block] on a block, 
        /// These events change persistent state, and always include [InsertBlockEvent], 
        /// possibly include: [UpdateHighestQCEvent], [UpdateLockedViewEvent], [PruneBlockEvent], 
        /// [CommitBlockEvent], [UpdateValidatorSetEvent].
        /// 
        /// Invariant: this method is invoked immediately after the corresponding changes are written to the [BlockTree].
        fn publish_insert_block_events(
            event_publisher: &Option<Sender<Event>>,
            block: Block,
            update_highest_qc: Option<QuorumCertificate>,
            update_locked_view: Option<ViewNumber>,
            committed_blocks: &Vec<(CryptoHash, Option<ValidatorSetUpdates>)>
        ) {
            Event::InsertBlock(InsertBlockEvent { timestamp: SystemTime::now(), block}).publish(event_publisher);
    
            if let Some(highest_qc) = update_highest_qc {
                Event::UpdateHighestQC(UpdateHighestQCEvent { timestamp: SystemTime::now(), highest_qc}).publish(event_publisher)
            };
    
            if let Some(locked_view) = update_locked_view {
                Event::UpdateLockedView(UpdateLockedViewEvent { timestamp: SystemTime::now(), locked_view}).publish(event_publisher)
            };
    
            committed_blocks
            .iter()
            .for_each(|(b, validator_set_updates_opt)| {
                Event::PruneBlock(PruneBlockEvent { timestamp: SystemTime::now(), block: b.clone()}).publish(event_publisher);
                Event::CommitBlock(CommitBlockEvent { timestamp: SystemTime::now(), block: b.clone()}).publish(event_publisher);
                if let Some(validator_set_updates) = validator_set_updates_opt {
                    Event::UpdateValidatorSet(UpdateValidatorSetEvent 
                        { 
                          timestamp: SystemTime::now(),
                          cause_block: *b,
                          validator_set_updates: validator_set_updates.clone()
                        }
                    )
                    .publish(event_publisher);
                }
            });
        }

        // Safety: a block that updates the validator set must be followed by a block that contains a commit
        // qc. A block becomes committed immediately if followed by a commit qc. Therefore, under normal
        // operation, at most 1 validator-set-updating block can be committed by on insertion.
        Ok(committed_blocks.into_iter().rev().find_map(|(_, validator_set_updates_opt)| validator_set_updates_opt))
    }

    pub fn set_highest_qc(&mut self, qc: &QuorumCertificate) -> Result<(), BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_qc(qc)?;
        self.write(wb);
        Ok(())
    }

    pub fn set_highest_tc(&mut self, tc: &TimeoutCertificate) -> Result<(), BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_tc(tc)?;
        self.write(wb);
        Ok(())
    }

    pub fn set_highest_view_entered(&mut self, view: ViewNumber) -> Result<(), BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_view_entered(view)?;
        self.write(wb);
        Ok(())
    }

    /* ↓↓↓ For committing a block in insert_block ↓↓↓ */

    /// Commits a block and its ancestors if they have not been committed already. 
    /// 
    /// Returns the hashes of the newly committed blocks, with the updates they caused to the validator set,
    /// in sequence (from lowest height to highest height).
    pub fn commit_block(
        &mut self,
        wb: &mut BlockTreeWriteBatch<K::WriteBatch>,
        block: &CryptoHash,
    ) -> Result<Vec<(CryptoHash, Option<ValidatorSetUpdates>)>, BlockTreeError> {

        // Obtain an iterator over the "block" and its ancestors, all the way until genesis, from newest ("block") to oldest.
        let blocks_iter =
            successors(
            Some(*block), 
            |b| 
                self.block_justify(b).ok()
                .map(|qc| if !qc.is_genesis_qc() {Some(qc.block)} else {None})
                .flatten()
            );

        // Newest committed block height, we do not consider the blocks from this height downwards.
        let min_height = self.highest_committed_block_height()?;

        // Obtain an iterator over the uncomitted blocks among "block" and its ancestors from oldest to newest,
        // the newest block being "block".
        // This is required because we want to commit blocks in correct order, applying updates from oldest to
        // newest.
        let uncommitted_blocks_iter = 
            blocks_iter.take_while(|b| min_height.is_none() || min_height.is_some_and(|h| self.block_height(b).unwrap().unwrap() > h));
        let uncommitted_blocks = uncommitted_blocks_iter.collect::<Vec<CryptoHash>>();
        let uncommitted_blocks_ordered_iter = uncommitted_blocks.iter().rev();
        
        // Helper closure that
        // (1) commits block b, applying all related updates to the write batch,
        // (2) extends the vector of blocks committed so far (accumulator) with b together with the optional
        //     validator set updates associated with b,
        // (3) returns the extended vector of blocks committed so far (updated accumulator).
        let commit = 
            |committed_blocks_res: Result<Vec<(CryptoHash, Option<ValidatorSetUpdates>)>, BlockTreeError>, b: &CryptoHash| 
            ->  Result<Vec<(CryptoHash, Option<ValidatorSetUpdates>)>, BlockTreeError> 
        {   
            let mut committed_blocks = committed_blocks_res?;

            let block_height = self.block_height(b)?.ok_or(BlockTreeError::BlockExpectedButNotFound{block: b.clone()})?;
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
            if let Some(pending_validator_set_updates) = self.pending_validator_set_updates(b)? {

                let mut committed_validator_set = self.committed_validator_set()?;
                committed_validator_set.apply_updates(&pending_validator_set_updates);

                wb.set_committed_validator_set(&committed_validator_set)?;
                wb.delete_pending_validator_set_updates(block);

                committed_blocks.push((*b, Some(pending_validator_set_updates.clone())));
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

    /* ↓↓↓ For deleting abandoned branches in insert_block ↓↓↓ */

    /// Delete the "siblings" of the specified block, along with all of its associated data (e.g., pending
    /// app state updates). Siblings
    /// here refer to other blocks that share the same parent as the specified block.
    /// 
    ///  # Precondition
    /// Block is in its parents' (or the genesis) children list.
    ///
    /// # Panics
    /// Panics if the block is not in the block tree, or if the block's parent (or genesis) does not have a
    /// children list.
    pub fn delete_siblings(
        &mut self,
        wb: &mut BlockTreeWriteBatch<K::WriteBatch>,
        block: &CryptoHash,
    ) -> Result<(), BlockTreeError> {
        let parent_or_genesis = self.block_justify(block).unwrap().block;
        let parents_or_genesis_children = self.children(&parent_or_genesis).unwrap();
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
    fn delete_branch(&mut self, wb: &mut BlockTreeWriteBatch<K::WriteBatch>, root: &CryptoHash) {
        for block in self.blocks_in_branch(*root) {
            wb.delete_children(&block);
            wb.delete_pending_app_state_updates(&block);
            wb.delete_pending_validator_set_updates(&block);

            if let Ok(Some(data_len)) = self.block_data_len(&block) {
                wb.delete_block(&block, data_len)
            }
        }
    }

    /// Performs depth-first search to collect all blocks in a branch into a single iterator.
    fn blocks_in_branch(&self, root: CryptoHash) -> impl Iterator<Item = CryptoHash> {
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

    /* ↓↓↓ Extra state getters for convenience ↓↓↓ */

    pub fn contains(&self, block: &CryptoHash) -> bool {
        self.block(block).is_ok() && self.block(block).unwrap().is_some()
    }

    pub(crate) fn highest_view_with_progress(&self) -> Result<ViewNumber, BlockTreeError> {
        Ok(
            max(
                self.highest_view_entered()?,
                max(
                    self.highest_qc()?.view,
                    self.highest_tc()?.map(|tc| tc.view).unwrap_or(ViewNumber::init()),
                )
            )
        )
    }

    pub(crate) fn highest_committed_block_height(&self) -> Result<Option<BlockHeight>, BlockTreeError> {
        let highest_committed_block = self.highest_committed_block()?;
        if let Some(block) = highest_committed_block {
            Ok(self.block_height(&block)?)
        } else {
            Ok(None)
        }
    }

    /* ↓↓↓ WriteBatch commit ↓↓↓ */

    pub fn write(&mut self, write_batch: BlockTreeWriteBatch<K::WriteBatch>) {
        self.0.write(write_batch.0)
    }

    /* ↓↓↓ Snapshot ↓↓↓ */

    pub fn snapshot(&self) -> BlockTreeSnapshot<K::Snapshot<'_>> {
        BlockTreeSnapshot::new(self.0.snapshot())
    }

    /* ↓↓↓ Get AppBlockTreeView for ProposeBlockRequest and ValidateBlockRequest */

    pub(crate) fn app_view<'a>(&'a self, parent: Option<&CryptoHash>) -> Result<AppBlockTreeView<'a, K>, BlockTreeError> {
        if parent.is_none() {
            return Ok(
                AppBlockTreeView {
                    block_tree: self,
                    parent_app_state_updates: None,
                    grandparent_app_state_updates: None,
                    great_grandparent_app_state_updates: None,
                }
            )
        }

        let parent_app_state_updates = self.pending_app_state_updates(parent.unwrap())?;
        let parent_justify = self.block_justify(parent.unwrap()).unwrap();

        if parent_justify.is_genesis_qc() {
            return Ok(
                AppBlockTreeView {
                    block_tree: self,
                    parent_app_state_updates,
                    grandparent_app_state_updates: None,
                    great_grandparent_app_state_updates: None,
                }
            )
        }

        let grandparent = parent_justify.block;
        let grandparent_app_state_updates = self.pending_app_state_updates(&grandparent)?;
        let grandparent_justify = self.block_justify(&grandparent).unwrap();

        if grandparent_justify.is_genesis_qc() {
            return Ok(
                AppBlockTreeView {
                    block_tree: self,
                    parent_app_state_updates,
                    grandparent_app_state_updates,
                    great_grandparent_app_state_updates: None,
                }
            )
        }

        let great_grandparent = grandparent_justify.block;
        let great_grandparent_app_state_updates =
            self.pending_app_state_updates(&great_grandparent)?;

        Ok(
            AppBlockTreeView {
                block_tree: self,
                parent_app_state_updates,
                grandparent_app_state_updates,
                great_grandparent_app_state_updates,
            }
        )
    }

    /* ↓↓↓ Basic state getters ↓↓↓ */

    pub(crate) fn block(&self, block: &CryptoHash) -> Result<Option<Block>, BlockTreeError> {
        Ok(self.0.block(block)?)
    }

    pub(crate) fn block_height(&self, block: &CryptoHash) -> Result<Option<BlockHeight>, BlockTreeError> {
        Ok(self.0.block_height(block)?)
    }

    pub(crate) fn block_data_hash(&self, block: &CryptoHash) -> Result<Option<CryptoHash>, BlockTreeError> {
        Ok(self.0.block_data_hash(block)?)
    }

    pub(crate) fn block_justify(&self, block: &CryptoHash) -> Result<QuorumCertificate, BlockTreeError> {
        Ok(self.0.block_justify(block)?)
    }

    pub(crate) fn block_data_len(&self, block: &CryptoHash) -> Result<Option<DataLen>, BlockTreeError> {
        Ok(self.0.block_data_len(block)?)
    }

    pub(crate) fn block_data(&self, block: &CryptoHash) -> Result<Option<Data>, BlockTreeError> {
        Ok(self.0.block_data(block)?)
    }

    pub(crate) fn block_datum(&self, block: &CryptoHash, datum_index: u32) -> Option<Datum> {
        self.0.block_datum(block, datum_index)
    }

    pub(crate) fn block_at_height(&self, height: BlockHeight) -> Result<Option<CryptoHash>, BlockTreeError> {
        Ok(self.0.block_at_height(height)?)
    }

    pub(crate) fn children(&self, block: &CryptoHash) -> Result<ChildrenList, BlockTreeError> {
        Ok(self.0.children(block)?)
    }

    pub(crate) fn committed_app_state(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.committed_app_state(key)
    }

    pub(crate) fn pending_app_state_updates(&self, block: &CryptoHash) -> Result<Option<AppStateUpdates>, BlockTreeError> {
        Ok(self.0.pending_app_state_updates(block)?)
    }

    pub(crate) fn committed_validator_set(&self) -> Result<ValidatorSet, BlockTreeError> {
        Ok(self.0.committed_validator_set()?)
    }

    pub(crate) fn pending_validator_set_updates(&self, block: &CryptoHash) -> Result<Option<ValidatorSetUpdates>, BlockTreeError> {
        Ok(self.0.pending_validator_set_updates(block)?)
    }

    pub(crate) fn locked_qc(&self) -> Result<QuorumCertificate, BlockTreeError> {
        Ok(self.0.locked_qc()?)
    }

    pub(crate) fn highest_view_entered(&self) -> Result<ViewNumber, BlockTreeError> {
        Ok(self.0.highest_view_entered()?)
    }

    pub(crate) fn highest_qc(&self) -> Result<QuorumCertificate, BlockTreeError> {
        Ok(self.0.highest_qc()?)
    }

    pub(crate) fn highest_committed_block(&self) -> Result<Option<CryptoHash>, BlockTreeError> {
        Ok(self.0.highest_committed_block()?)
    }

    pub(crate) fn newest_block(&self) -> Result<Option<CryptoHash>, BlockTreeError> {
        Ok(self.0.newest_block()?)
    }

    pub(crate) fn highest_tc(&self) -> Result<Option<TimeoutCertificate>, BlockTreeError> {
        Ok(self.0.highest_tc()?)
    }

    pub(crate) fn previous_validator_set(&self) -> Result<ValidatorSet, BlockTreeError> {
        Ok(self.0.previous_validator_set()?)
    }

    pub(crate) fn validator_set_update_block_height(&self) -> Result<BlockHeight, BlockTreeError> {
        Ok(self.validator_set_update_block_height()?)
    }

    pub(crate) fn validator_set_update_complete(&self) -> Result<bool, BlockTreeError> {
        Ok(self.0.validator_set_update_complete()?)
    }

    pub(crate) fn validator_set_state(&self) -> Result<ValidatorSetState, BlockTreeError> {
        Ok(self.0.validator_set_state()?)
    }

}

#[derive(Debug)]
pub enum BlockTreeError {
    KVGetError(KVGetError),
    KVSetError(KVSetError),
    BlockExpectedButNotFound{block: CryptoHash}
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
