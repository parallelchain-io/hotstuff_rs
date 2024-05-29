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
//! [BlockTreeCamera](crate::state::block_tree_camera::BlockTreeCamera).
//!
//! In normal operation, HotStuff-rs code will internally be making all writes to the 
//! [Block Tree](crate::state::block_tree::BlockTree), and users can get a 
//! [BlockTreeCamera](crate::state::block_tree_camera::BlockTreeCamera) using replica's 
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
//! |Block to Children|[CryptoHash] -> [ChildrenList]|A mapping between a block's hash and the children it has in the block tree. A block may have multiple children if they have not been committed.|
//! |Committed App State|[Vec<u8>] -> [Vec<u8>]| Current app state as per last committed block.|
//! |Pending App State Updates|[CryptoHash] -> [AppStateUpdates]||
//! |Committed Validator Set|[ValidatorSet]|The acting validator set.|
//! |Previous Validator Set|[ValidatorSet]|The previous acting validator set, possibly still active if the update has not been completed yet.|
//! |Validator Set Update Completed|[bool]|Whether the most recently initiated validator set update has been completed. A validator set update is initiated when a commit QC for the corresponding validator-set-updating block is seen.|
//! |Validator Set Update Block Height|The height of the block associated with the most recently initiated validator set update.|
//! |Validator Set Updates Status|[CryptoHash] -> [ValidatorSetUpdatesStatus]||
//! |Locked QC|[QuorumCertificate]| QC of a block that is about to be committed, unless there is evidence for a quorum switching to a conflicting branch. Refer to the HotStuff paper for details.|
//! |Highest Voted View|[ViewNumber]|The highest view that this validator has voted in.|
//! |Highest View Entered|[ViewNumber]|The highest view that this validator has entered.|
//! |Highest Quorum Certificate|[QuorumCertificate]|Among the quorum certificates this validator has seen and verified the signatures of, the one with the highest view number.|
//! |Highest Timeout Certificate|[TimeoutCertificate]|Among the timeout certificates this validator has seen and verified the signatures of, the one with the highest view number.|
//! |Highest Committed Block|[CryptoHash]|The hash of the committed block that has the highest height.|
//! |Newest BlocK|[CryptoHash]The hash of the most recent block to be inserted into the block tree.|
//! 
//! ### Persistence in KV Store
//! 
//! The location of each of these variables in a KV store is defined in [paths](crate::state::paths). 
//! Note that the fields of a block are itself stored in different tuples. This is so that user code 
//! can get a subset of a block's data without loading the entire block from storage (which can be 
//! expensive). The key suffixes on which each of block's fields are stored are also defined in paths.
//!
//! ## Initial state
//!
//! All variables in the Block Tree start out empty except eight. These eight variables are:
//! 
//! |Variable|Initial value|
//! |---|---|
//! |Committed App State|Provided to [`Replica::initialize`](crate::replica::Replica::initialize).|
//! |Committed Validator Set|Provided to [`Replica::initialize`](crate::replica::Replica::initialize).|
//! |Previous Validator Set| Provided to [`Replica::initialize`](crate::replica::Replica::initialize).|
//! |Validator Set Update Block Height|Provided to [`Replica::initialize`](crate::replica::Replica::initialize).|
//! |Validator Set Update Complete|Provided to [`Replica::initialize`](crate::replica::Replica::initialize).|
//! |Locked QC|The [genesis QC](crate::hotstuff::types::QuorumCertificate::genesis_qc)|
//! |Highest View Entered|0|
//! |Highest Quorum Certificate|The [genesis QC](crate::hotstuff::types::QuorumCertificate::genesis_qc)|

use std::cmp::max;
use std::iter::successors;
use crate::pacemaker::types::TimeoutCertificate;
use crate::types::basic::{ChildrenList, CryptoHash, ViewNumber};
use crate::types::*;
use crate::hotstuff::types::QuorumCertificate;

use self::basic::{AppStateUpdates, BlockHeight, Data, DataLen, Datum};
use self::block::Block;
use self::validators::{ValidatorSetUpdatesStatus, ValidatorSet, ValidatorSetState, ValidatorSetUpdates};

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
        initial_validator_set_state: &ValidatorSetState,
    ) -> Result<(), BlockTreeError>{
        let mut wb = BlockTreeWriteBatch::new();

        wb.apply_app_state_updates(initial_app_state);

        let committed_validator_set = initial_validator_set_state.committed_validator_set();
        let previous_validator_set = initial_validator_set_state.previous_validator_set();
        let update_height = initial_validator_set_state.update_height();
        let update_completed = initial_validator_set_state.update_completed();

        wb.set_committed_validator_set(&committed_validator_set)?;
        wb.set_previous_validator_set(&previous_validator_set)?;
        if let Some(height) = *update_height {
            wb.set_validator_set_update_block_height(height)?
        }
        wb.set_validator_set_update_completed(update_completed)?;

        wb.set_locked_qc(&QuorumCertificate::genesis_qc())?;

        wb.set_highest_view_entered(ViewNumber::init())?;

        wb.set_highest_qc(&QuorumCertificate::genesis_qc())?;

        self.write(wb);

        Ok(())
    }

    /// Insert a block into the block tree. This includes:
    /// 1. Setting pending app states and validator set updates associated with the block,
    /// 2. Updating all relevant mappings from the block.
    ///
    /// # Precondition
    /// [super::safety::safe_block]
    pub fn insert_block(
        &mut self,
        block: &Block,
        app_state_updates: Option<&AppStateUpdates>,
        validator_set_updates: Option<&ValidatorSetUpdates>,
    ) -> Result<(), BlockTreeError> {   
        let mut wb = BlockTreeWriteBatch::new();

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

        self.write(wb);


        Ok(())

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

    pub fn set_highest_view_voted(&mut self, view: ViewNumber) -> Result<(), BlockTreeError> {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_view_voted(view)?;
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
            if let ValidatorSetUpdatesStatus::Pending(validator_set_updates) = self.validator_set_updates_status(b)? {

                let mut committed_validator_set = self.committed_validator_set()?;
                let previous_validator_set = committed_validator_set.clone();
                committed_validator_set.apply_updates(&validator_set_updates);

                wb.set_committed_validator_set(&committed_validator_set)?;
                wb.set_previous_validator_set(&previous_validator_set)?;
                wb.set_validator_set_update_block_height(block_height)?;
                wb.set_validator_set_update_completed(false)?;
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

    /* ↓↓↓ For deleting abandoned branches in insert_block ↓↓↓ */

    /// Delete the "siblings" of the specified block, along with all of its associated data (e.g., pending
    /// app state updates). Siblings
    /// here refer to other blocks that share the same parent as the specified block.
    /// 
    ///  # Precondition
    /// Block is in its parents' (or the genesis) children list.
    ///
    /// # Error
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
    fn delete_branch(&mut self, wb: &mut BlockTreeWriteBatch<K::WriteBatch>, root: &CryptoHash) {
        for block in self.blocks_in_branch(*root) {
            wb.delete_children(&block);
            wb.delete_pending_app_state_updates(&block);
            wb.delete_block_validator_set_updates(&block);

            if let Ok(Some(data_len)) = self.block_data_len(&block) {
                wb.delete_block(&block, data_len)
            }
        }
    }

    /// Performs depth-first search to collect all blocks in a branch into a single iterator.
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

    /* ↓↓↓ Extra state getters for convenience ↓↓↓ */

    pub fn contains(&self, block: &CryptoHash) -> bool {
        self.block(block).is_ok_and(|block_opt| block_opt.is_some())
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
        let highest_committed_block_height = self.highest_committed_block_height()?;
        let parent = 
            match parent {
                None => None,
                Some(&b) => Some(b)
            };

        // Obtain an iterator over the ancestors starting from the parent, all the way until genesis, 
        // from newest (parent) to oldest.
        let ancestors_iter =
            successors(
            parent, 
            |b| 
                self.block_justify(b).ok()
                .map(|qc| if !qc.is_genesis_qc() {Some(qc.block)} else {None})
                .flatten()
            );
        
        let ancestors_heights_iter = 
            ancestors_iter
            .clone()
            .map(|block| 
                self.block_height(&block)
                .map(|res| if res.is_none() {Err(BlockTreeError::BlockExpectedButNotFound{block: block.clone()})} else {Ok(res.unwrap())})
            )
            .flatten()
            .flatten();

        // Obtain an iterator over the uncomitted ancestors starting from the parent,
        // ending at the lowest uncommitted ancestor.
        let uncommitted_ancestors_iter =
            ancestors_iter
            .zip(ancestors_heights_iter)
            .take_while(|(_, height)| 
                highest_committed_block_height.is_none() || 
                highest_committed_block_height.is_some_and(|h| height > &h)
            )
            .map(|(b, _)| b);

        // Obtain a vector of optional app state updates associated with ancestors
        // starting from the parent, ending at the oldest uncommitted ancestor.
        let pending_ancestors_app_state_updates: Vec<Option<AppStateUpdates>> = 
            uncommitted_ancestors_iter
            .map(|block| self.pending_app_state_updates(&block))
            .flatten()
            .collect();

        Ok(
            AppBlockTreeView {
                block_tree: self,
                pending_ancestors_app_state_updates
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

    pub(crate) fn validator_set_updates_status(&self, block: &CryptoHash) -> Result<ValidatorSetUpdatesStatus, BlockTreeError> {
        Ok(self.0.validator_set_updates_status(block)?)
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

    pub(crate) fn highest_tc(&self) -> Result<Option<TimeoutCertificate>, BlockTreeError> {
        Ok(self.0.highest_tc()?)
    }

    pub(crate) fn validator_set_state(&self) -> Result<ValidatorSetState, BlockTreeError> {
        Ok(self.0.validator_set_state()?)
    }

    pub(crate) fn highest_view_voted(&self) -> Result<Option<ViewNumber>, BlockTreeError> {
        Ok(self.0.highest_view_voted()?)
    }

}

/// Error when reading or writing to the [BlockTree]. Three kinds of errors may be encountered:
/// 1. Error when trying to get a value from the underlying [key value store][KVStore],
/// 2. Error when trying to set a value for a given key in the underlying [key value store][KVStore],
/// 3. Unable to find a block that matches a given block tree query, even though the block should be 
///    present in the block tree.
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
