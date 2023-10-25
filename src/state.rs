/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Types and methods used to access and mutate the persistent state that a replica keeps track for the operation
//! of the protocol, and for its application.
//!
//! This state may be stored in a key-value store of the library user's own choosing, as long as that KV store
//! can provide a type that implements [KVStore]. This state can be mutated through an instance of [BlockTree],
//! and read through an instance of [BlockTreeSnapshot], which can be created using [BlockTreeCamera].
//!
//! In normal operation, HotStuff-rs code will internally be making all writes to the Block Tree, and users can
//! get a [BlockTreeCamera] using replica's [block_tree_camera](crate::replica::Replica::block_tree_camera) method.
//!
//! Sometimes, however, users may want to manually mutate the Block Tree, for example, to recover from an error
//! that has corrupted its invariants. For this purpose, one can unsafe-ly get an instance of BlockTree using
//! [BlockTree::new_unsafe] and an instance of the corresponding [BlockTreeWriteBatch] using
//! [BlockTreeWriteBatch::new_unsafe].
//!
//! ## State variables
//!
//! HotStuff-rs structures its state into separate conceptual 'variables' which are stored in tuples that sit
//! at a particular key path or prefix in the library user's chosen KV store. These variables are:
//! - **Blocks** ([CryptoHash] -> [Block]).
//! - **Block at Height** ([BlockHeight] -> [CryptoHash]): a mapping between a block's number and a block's hash. This mapping only contains blocks that are committed, because if a block hasn't been committed, there may be multiple blocks at the same height.
//! - **Block to Children** ([CryptoHash] -> [ChildrenList]): a mapping between a block's hash and the children it has in the block tree. A block may have multiple chilren if they have not been committed.
//! - **Committed App State** ([Vec<u8>] -> [Vec<u8>]).
//! - **Pending App State Updates** ([CryptoHash] -> [AppStateUpdates]).
//! - **Committed Validator Set** ([ValidatorSet]).
//! - **Pending Validator Set Updates** ([CryptoHash] -> [ValidatorSetUpdates]).
//! - **Locked View** ([ViewNumber]): the highest view number of a quorum certificate contained in a block that has a child.
//! - **Highest Voted View** ([ViewNumber]): the highest view that this validator has voted in.
//! - **Highest Quorum Certificate** ([QuorumCertificate]): among the quorum certificates this validator has seen and verified the signatures of, the one with the highest view number.
//! - **Highest Committed Block** ([CryptoHash]).
//! - **Newest Block** ([CryptoHash]): the most recent block to be inserted into the block tree.
//!
//! The location of each of these variables in a KV store is defined in [paths]. Note that the fields of a
//! block are itself stored in different tuples. This is so that user code can get a subset of a block's data
//! without loading the entire block from storage (which can be expensive). The key suffixes on which each of
//! block's fields are stored are also defined in paths.
//!
//! ## Initial state
//!
//! All fields in the Block Tree start out empty except 5, which are set to initial values in
//! [BlockTree::initialize]. These are:
//! - **Committed App State** (initial state provided as an argument).
//! - **Committed Validator Set** (initial state provided as an argument).
//! - **Locked View** (initialized to 0).
//! - **Highest View Entered** (initialized to 0).
//! - **Highest Quorum Certificate** (initialized to [the genesis qc](QuorumCertificate::genesis_qc)).

use crate::events::{Event, InsertBlockEvent, CommitBlockEvent, PruneBlockEvent, UpdateHighestQCEvent, UpdateLockedViewEvent, UpdateValidatorSetEvent};
use crate::types::*;
use borsh::{BorshDeserialize, BorshSerialize};
use std::iter::successors;
use std::sync::mpsc::Sender;
use std::time::SystemTime;

/// A read and write handle into the block tree exclusively owned by the algorithm thread.
pub struct BlockTree<K: KVStore>(K);

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
    ) {
        let mut wb = BlockTreeWriteBatch::new();

        wb.apply_app_state_updates(initial_app_state);

        let mut validator_set = ValidatorSet::new();
        validator_set.apply_updates(initial_validator_set);
        wb.set_committed_validator_set(&validator_set);

        wb.set_locked_view(0);

        wb.set_highest_view_entered(0);

        wb.set_highest_qc(&QuorumCertificate::genesis_qc());

        self.write(wb);
    }

    /* ↓↓↓ Methods for growing the Block Tree ↓↓↓ */

    /// Returns whether a block can be safely inserted. For this, it is necessary that:
    /// 1. self.safe_qc(&block.justify).
    /// 2. No block with the same block hash is already in the block tree.
    /// 3. Its qc's must be either a generic qc or a commit qc.
    ///
    /// This function evaluates [Self::safe_qc], then checks 2 and 3.
    ///
    /// # Precondition
    /// [Block::is_correct]
    pub fn safe_block(&self, block: &Block, chain_id: ChainID) -> bool {
        /* 1 */
        self.safe_qc(&block.justify, chain_id) &&
        /* 2 */ !self.contains(&block.hash)  &&
        /* 3 */ (block.justify.phase.is_generic() || block.justify.phase.is_commit())
    }

    /// Returns whether a qc can be 'inserted' into the block tree, whether as part of a block using [BlockTree::insert_block],
    /// or to be set as the highest qc, or, if it is a precommit or commit qc, to have the view of its prepare qc set as the
    /// locked view.
    ///
    /// For this, it is necessary that:
    /// 1. Its chain ID matches the chain ID of the replica
    /// 2. It justifies a known block, or is the genesis qc.
    /// 3. Its view number is greater than or equal to locked view.
    /// 4. If it is a prepare, precommit, or commit qc, the block it justifies has pending validator state updates.
    /// 5. If its qc is a generic qc, the block it justifies *does not* have pending validator set updates.
    ///
    /// # Precondition
    /// [QuorumCertificate::is_correct]
    pub fn safe_qc(&self, qc: &QuorumCertificate, chain_id: ChainID) -> bool {
        /* 1 */
        qc.chain_id == chain_id &&
        /* 2 */
        (self.contains(&qc.block) || qc.is_genesis_qc()) &&
        /* 3 */ qc.view >= self.locked_view() &&
        /* 4 */ (((qc.phase.is_prepare() || qc.phase.is_precommit() || qc.phase.is_commit()) && self.pending_validator_set_updates(&qc.block).is_some()) ||
        /* 5 */ (qc.phase.is_generic() && self.pending_validator_set_updates(&qc.block).is_none()))
    }

    /// Insert a block, causing all of the necessary state changes, including possibly block commit, to happen.
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
    ) -> Option<ValidatorSetUpdates> {   
        let mut wb = BlockTreeWriteBatch::new();
        let mut update_locked_view: Option<ViewNumber> = None;
        let mut update_highest_qc: Option<QuorumCertificate> = None;

        // Insert block.
        wb.set_block(block);
        wb.set_newest_block(&block.hash);
        if let Some(app_state_updates) = app_state_updates {
            wb.set_pending_app_state_updates(&block.hash, app_state_updates);
        }
        if let Some(validator_set_updates) = validator_set_updates {
            wb.set_pending_validator_set_updates(&block.hash, validator_set_updates);
        }

        let mut siblings = self
            .children(&block.justify.block)
            .unwrap_or(ChildrenList::new());
        siblings.push(block.hash);
        wb.set_children(&block.justify.block, &siblings);

        // Consider updating highest qc.
        if block.justify.view > self.highest_qc().view {
            wb.set_highest_qc(&block.justify);
            update_highest_qc = Some(block.justify.clone())
        }

        // If block does not ancestors, return.
        if block.justify.is_genesis_qc() {
            self.write(wb);
            publish_insert_block_events(event_publisher, block.clone(), update_highest_qc, update_locked_view, Vec::new());
            return None;
        }

        // Otherwise, do things to ancestors according to whether the block contains a commit qc or a generic qc.
        let committed_blocks = match block.justify.phase {
            Phase::Generic => {
                // Consider setting locked view to parent.justify.view.
                let parent = block.justify.block;
                let parent_justify_view = self.block_justify(&parent).unwrap().view;
                if parent_justify_view > self.locked_view() {
                    wb.set_locked_view(parent_justify_view);
                    update_locked_view = Some(parent_justify_view);
                }

                // Get great-grandparent.
                let great_grandparent = {
                    let parent_justify = self.block_justify(&parent).unwrap();
                    if parent_justify.is_genesis_qc() {
                        self.write(wb);
                        publish_insert_block_events(event_publisher, block.clone(), update_highest_qc, update_locked_view, Vec::new());
                        return None;
                    }

                    let grandparent = parent_justify.block;
                    let grandparent_justify = self.block_justify(&grandparent).unwrap();
                    if grandparent_justify.is_genesis_qc() {
                        self.write(wb);
                        publish_insert_block_events(event_publisher, block.clone(), update_highest_qc, update_locked_view, Vec::new());
                        return None;
                    }

                    grandparent_justify.block
                };

                // Commit great_grandparent if not committed yet.
                self.commit_block(&mut wb, &great_grandparent)
            }

            Phase::Commit(precommit_qc_view) => {
                // Consider setting locked view to precommit_qc_view.
                if precommit_qc_view > self.locked_view() {
                    wb.set_locked_view(precommit_qc_view);
                    update_locked_view = Some(precommit_qc_view);
                }

                let parent = block.justify.block;

                // Commit parent if not committed yet.
                self.commit_block(&mut wb, &parent)
            }

            _ => panic!(),
        };

        self.write(wb);

        publish_insert_block_events(event_publisher, block.clone(), update_highest_qc, update_locked_view, committed_blocks.clone());

        /// Publish all events resulting from calling insert_block on a block, 
        /// These events change persistent state, and always include InsertBlockEvent, 
        /// possibly include: UpdateHighestQCEvent, UpdatedLockedViewEvent, PruneBlockEvent, CommitBlockEvent, UpdateValidatorSetEvent
        /// Invariant: invoked immediately after the corresponding changes are written to the Block Tree
        fn publish_insert_block_events(
            event_publisher: &Option<Sender<Event>>,
            block: Block,
            update_highest_qc: Option<QuorumCertificate>,
            update_locked_view: Option<ViewNumber>,
            committed_blocks: Vec<(CryptoHash, Option<ValidatorSetUpdates>)>
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
                Event::PruneBlock(PruneBlockEvent { timestamp: SystemTime::now(), block: *b}).publish(event_publisher);
                Event::CommitBlock(CommitBlockEvent { timestamp: SystemTime::now(), block: *b}).publish(event_publisher);
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

        // Safety: a block that updates the validator set must be followed by a block that contains
        // a commit qc. A block becomes committed immediately if followed by a commit qc. Therefore,
        // under normal operation, at most 1 validator-set-updating block can be committed by one
        // insertion.
        committed_blocks.into_iter().rev().find_map(|(_, validator_set_updates_opt)| validator_set_updates_opt)
    }

    pub fn set_highest_qc(&mut self, qc: &QuorumCertificate) {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_qc(qc);
        self.write(wb);
    }

    pub fn set_highest_view_entered(&mut self, view: ViewNumber) {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_view_entered(view);
        self.write(wb);
    }

    /* ↓↓↓ For committing a block in insert_block ↓↓↓ */

    /// Commits a block and its ancestors if they have not been committed already. 
    /// Returns the newly committed blocks, with the updates they caused to the validator set, in sequence (from oldest to newest).
    pub fn commit_block(
        &mut self,
        wb: &mut BlockTreeWriteBatch<K::WriteBatch>,
        block: &CryptoHash,
    ) -> Vec<(CryptoHash, Option<ValidatorSetUpdates>)> {

        // Obtain an iterator over the "block" and its ancestors, all the way until genesis, from newest ("block") to oldest.
        let blocks_iter =
            successors(
            Some(*block), 
            |b| 
                self.block_justify(b)
                .map(|qc| if !qc.is_genesis_qc() {Some(qc.block)} else {None})
                .flatten()
            );

        // Newest committed block height, we do not consider the blocks from this height downwards.
        let min_height = self.highest_committed_block_height();

        // Obtain an iterator over the uncomitted blocks among "block" and its ancestors from oldest to newest, the newest block being "block".
        // This is required because we want to commit blocks in correct order, applying updates from oldest to newest.
        let uncommitted_blocks_iter = blocks_iter.take_while(|b| min_height.is_none() || min_height.is_some_and(|h| self.block_height(b).unwrap() > h));
        let uncommitted_blocks = uncommitted_blocks_iter.collect::<Vec<CryptoHash>>();
        let uncommitted_blocks_ordered_iter = uncommitted_blocks.iter().rev();
        
        // Helper closure that
        // (1) commits block b, applying all related updates to the write batch
        // (2) extends the vector of blocks committed so far (accumulator) with b together with the optional validator set updates associated with b
        // (3) returns the extended vector of blocks committed so far (updated accumulator)
        let commit = |mut committed_blocks: Vec<(CryptoHash, Option<ValidatorSetUpdates>)>, b: &CryptoHash| ->  Vec<(CryptoHash, Option<ValidatorSetUpdates>)>{

            let block_height = self.block_height(b).unwrap();
            // Work steps:

            // Set block at height.
            wb.set_block_at_height(block_height, b);

            // Delete all of block's siblings.
            self.delete_siblings(wb, b);

            // Apply pending app state updates.
            if let Some(pending_app_state_updates) = self.pending_app_state_updates(b) {
                wb.apply_app_state_updates(&pending_app_state_updates);
                wb.delete_pending_app_state_updates(b);
            }

            // Apply pending validator set updates.
            if let Some(pending_validator_set_updates) = self.pending_validator_set_updates(b) {

                let mut committed_validator_set = self.committed_validator_set();
                committed_validator_set.apply_updates(&pending_validator_set_updates);

                wb.set_committed_validator_set(&committed_validator_set);
                wb.delete_pending_validator_set_updates(block);

                committed_blocks.push((*b, Some(pending_validator_set_updates.clone())));
            } else {
                committed_blocks.push((*b, None));
            }

            // Update the highest committed block.
            wb.set_highest_committed_block(b);

            // Return the blocks committed so far together with their corresponding validator set updates.
            committed_blocks
        };

        // Iterate over the uncommitted blocks from oldest to newest, 
        // (1) applying related updates (by mutating the write batch), and
        // (2) building up the vector of committed blocks (by pushing the newely committed blocks to the accumulator vector)
        // Finally, return the accumulator
        uncommitted_blocks_ordered_iter.fold(Vec::new(), commit)

    }

    /* ↓↓↓ For deleting abandoned branches in insert_block ↓↓↓ */

    /// # Precondition
    /// Block is in its parents' (or the genesis) children list.
    ///
    /// # Panics
    /// Panics if the block is not in the block tree, or if the block's parent (or genesis) does not have a children
    /// list.
    pub fn delete_siblings(
        &mut self,
        wb: &mut BlockTreeWriteBatch<K::WriteBatch>,
        block: &CryptoHash,
    ) {
        let parent_or_genesis = self.block_justify(block).unwrap().block;
        let parents_or_genesis_children = self.children(&parent_or_genesis).unwrap();
        let siblings = parents_or_genesis_children
            .iter()
            .filter(|sib| *sib != block);
        for sibling in siblings {
            self.delete_branch(wb, sibling);
        }

        wb.set_children(&parent_or_genesis, &vec![*block]);
    }

    fn delete_branch(&mut self, wb: &mut BlockTreeWriteBatch<K::WriteBatch>, tail: &CryptoHash) {
        if let Some(children) = self.children(tail) {
            for child in children {
                self.delete_branch(wb, &child);
            }
        }

        wb.delete_children(tail);
        wb.delete_pending_app_state_updates(tail);
        wb.delete_pending_validator_set_updates(tail);
    }

    /* ↓↓↓ Extra state getters for convenience ↓↓↓ */

    pub fn contains(&self, block: &CryptoHash) -> bool {
        self.block_height(block).is_some()
    }

    pub(crate) fn highest_committed_block_height(&self) -> Option<BlockHeight> {
        let highest_committed_block = self.highest_committed_block()?;
        self.block_height(&highest_committed_block)
    }

    /* ↓↓↓ WriteBatch commit ↓↓↓ */

    pub fn write(&mut self, write_batch: BlockTreeWriteBatch<K::WriteBatch>) {
        self.0.write(write_batch.0);
    }

    /* ↓↓↓ Snapshot ↓↓↓ */

    pub fn snapshot(&self) -> BlockTreeSnapshot<K::Snapshot<'_>> {
        BlockTreeSnapshot::new(self.0.snapshot())
    }

    /* ↓↓↓ Get AppBlockTreeView for ProposeBlockRequest and ValidateBlockRequest */

    pub(crate) fn app_view<'a>(&'a self, parent: Option<&CryptoHash>) -> AppBlockTreeView<'a, K> {
        if parent.is_none() {
            return AppBlockTreeView {
                block_tree: self,
                parent_app_state_updates: None,
                grandparent_app_state_updates: None,
                great_grandparent_app_state_updates: None,
            };
        }

        let parent_app_state_updates = self.pending_app_state_updates(parent.unwrap());
        let parent_justify = self.block_justify(parent.unwrap()).unwrap();

        if parent_justify.is_genesis_qc() {
            return AppBlockTreeView {
                block_tree: self,
                parent_app_state_updates,
                grandparent_app_state_updates: None,
                great_grandparent_app_state_updates: None,
            };
        }

        let grandparent = parent_justify.block;
        let grandparent_app_state_updates = self.pending_app_state_updates(&grandparent);
        let grandparent_justify = self.block_justify(&grandparent).unwrap();

        if grandparent_justify.is_genesis_qc() {
            return AppBlockTreeView {
                block_tree: self,
                parent_app_state_updates,
                grandparent_app_state_updates,
                great_grandparent_app_state_updates: None,
            };
        }

        let great_grandparent = grandparent_justify.block;
        let great_grandparent_app_state_updates =
            self.pending_app_state_updates(&great_grandparent);

        AppBlockTreeView {
            block_tree: self,
            parent_app_state_updates,
            grandparent_app_state_updates,
            great_grandparent_app_state_updates,
        }
    }
}

pub struct AppBlockTreeView<'a, K: KVStore> {
    block_tree: &'a BlockTree<K>,
    parent_app_state_updates: Option<AppStateUpdates>,
    grandparent_app_state_updates: Option<AppStateUpdates>,
    great_grandparent_app_state_updates: Option<AppStateUpdates>,
}

impl<'a, K: KVStore> AppBlockTreeView<'a, K> {
    pub fn block(&self, block: &CryptoHash) -> Option<Block> {
        self.block_tree.block(block)
    }

    pub fn block_height(&self, block: &CryptoHash) -> Option<BlockHeight> {
        self.block_tree.block_height(block)
    }

    pub fn block_justify(&self, block: &CryptoHash) -> Option<QuorumCertificate> {
        self.block_tree.block_justify(block)
    }

    pub fn block_data_hash(&self, block: &CryptoHash) -> Option<CryptoHash> {
        self.block_tree.block_data_hash(block)
    }

    pub fn block_data_len(&self, block: &CryptoHash) -> Option<DataLen> {
        self.block_tree.block_data_len(block)
    }

    pub fn block_data(&self, block: &CryptoHash) -> Option<Data> {
        self.block_tree.block_data(block)
    }

    pub fn block_datum(&self, block: &CryptoHash, datum_index: u32) -> Option<Datum> {
        self.block_tree.block_datum(block, datum_index)
    }

    pub fn block_at_height(&self, height: BlockHeight) -> Option<CryptoHash> {
        self.block_tree.block_at_height(height)
    }

    pub fn app_state(&'a self, key: &[u8]) -> Option<Vec<u8>> {
        if let Some(parent_app_state_updates) = &self.parent_app_state_updates {
            if parent_app_state_updates.contains_delete(&key.to_vec()) {
                return None;
            } else if let Some(value) = parent_app_state_updates.get_insert(&key.to_vec()) {
                return Some(value.clone());
            }
        }

        if let Some(grandparent_app_state_changes) = &self.grandparent_app_state_updates {
            if grandparent_app_state_changes.contains_delete(&key.to_vec()) {
                return None;
            } else if let Some(value) = grandparent_app_state_changes.get_insert(&key.to_vec()) {
                return Some(value.clone());
            }
        }

        if let Some(great_grandparent_app_state_changes) = &self.great_grandparent_app_state_updates
        {
            if great_grandparent_app_state_changes.contains_delete(&key.to_vec()) {
                return None;
            } else if let Some(value) =
                great_grandparent_app_state_changes.get_insert(&key.to_vec())
            {
                return Some(value.clone());
            }
        }

        self.block_tree.committed_app_state(key)
    }

    pub fn validator_set(&self) -> ValidatorSet {
        self.block_tree.committed_validator_set()
    }
}

pub struct BlockTreeWriteBatch<W: WriteBatch>(W);

use paths::*;
impl<W: WriteBatch> BlockTreeWriteBatch<W> {
    pub(crate) fn new() -> BlockTreeWriteBatch<W> {
        BlockTreeWriteBatch(W::new())
    }

    pub fn new_unsafe() -> BlockTreeWriteBatch<W> {
        Self::new()
    }

    /* ↓↓↓ Block ↓↓↓  */

    pub fn set_block(&mut self, block: &Block) {
        let block_prefix = combine(&BLOCKS, &block.hash);

        self.0.set(
            &combine(&block_prefix, &BLOCK_HEIGHT),
            &block.height.try_to_vec().unwrap(),
        );
        self.0.set(
            &combine(&block_prefix, &BLOCK_JUSTIFY),
            &block.justify.try_to_vec().unwrap(),
        );
        self.0.set(
            &combine(&block_prefix, &BLOCK_DATA_HASH),
            &block.data_hash.try_to_vec().unwrap(),
        );
        self.0.set(
            &combine(&block_prefix, &BLOCK_DATA_LEN),
            &block.data.len().try_to_vec().unwrap(),
        );

        // Insert datums.
        let block_data_prefix = combine(&block_prefix, &BLOCK_DATA);
        for (i, datum) in block.data.iter().enumerate() {
            let datum_key = combine(&block_data_prefix, &(i as u32).try_to_vec().unwrap());
            self.0.set(&datum_key, datum);
        }
    }

    pub fn delete_block(&mut self, block: &CryptoHash, data_len: DataLen) {
        let block_prefix = combine(&BLOCKS, block);

        self.0.delete(&combine(&block_prefix, &BLOCK_HEIGHT));
        self.0.delete(&combine(&block_prefix, &BLOCK_JUSTIFY));
        self.0.delete(&combine(&block_prefix, &BLOCK_DATA_HASH));
        self.0.delete(&combine(&block_prefix, &BLOCK_DATA_LEN));

        let block_data_prefix = combine(&block_prefix, &BLOCK_DATA);
        for i in 0..data_len {
            let datum_key = combine(&block_data_prefix, &i.try_to_vec().unwrap());
            self.0.delete(&datum_key);
        }
    }

    /* ↓↓↓ Block at Height ↓↓↓ */

    pub fn set_block_at_height(&mut self, height: BlockHeight, block: &CryptoHash) {
        self.0.set(
            &combine(&BLOCK_AT_HEIGHT, &height.try_to_vec().unwrap()),
            &block.try_to_vec().unwrap(),
        );
    }

    /* ↓↓↓ Block to Children ↓↓↓ */

    pub fn set_children(&mut self, block: &CryptoHash, children: &ChildrenList) {
        self.0.set(
            &combine(&BLOCK_TO_CHILDREN, block),
            &children.try_to_vec().unwrap(),
        );
    }

    pub fn delete_children(&mut self, block: &CryptoHash) {
        self.0.delete(&combine(&BLOCK_TO_CHILDREN, block));
    }

    /* ↓↓↓ Committed App State ↓↓↓ */

    pub fn set_committed_app_state(&mut self, key: &[u8], value: &[u8]) {
        self.0.set(&combine(&COMMITTED_APP_STATE, key), value);
    }

    pub fn delete_committed_app_state(&mut self, key: &[u8]) {
        self.0.delete(&combine(&COMMITTED_APP_STATE, key));
    }

    /* ↓↓↓ Pending App State Updates ↓↓↓ */

    pub fn set_pending_app_state_updates(
        &mut self,
        block: &CryptoHash,
        app_state_updates: &AppStateUpdates,
    ) {
        self.0.set(
            &combine(&PENDING_APP_STATE_UPDATES, block),
            &app_state_updates.try_to_vec().unwrap(),
        );
    }

    pub fn apply_app_state_updates(&mut self, app_state_updates: &AppStateUpdates) {
        for (key, value) in app_state_updates.inserts() {
            self.set_committed_app_state(key, value);
        }

        for key in app_state_updates.deletions() {
            self.delete_committed_app_state(key);
        }
    }

    pub fn delete_pending_app_state_updates(&mut self, block: &CryptoHash) {
        self.0.delete(&combine(&PENDING_APP_STATE_UPDATES, block));
    }

    /* ↓↓↓ Commmitted Validator Set */

    pub fn set_committed_validator_set(&mut self, validator_set: &ValidatorSet) {
        let validator_set_bytes: ValidatorSetBytes = validator_set.into();
        self.0.set(
            &COMMITTED_VALIDATOR_SET,
            &validator_set_bytes.try_to_vec().unwrap(),
        )
    }

    /* ↓↓↓ Pending Validator Set Updates */

    pub fn set_pending_validator_set_updates(
        &mut self,
        block: &CryptoHash,
        validator_set_updates: &ValidatorSetUpdates,
    ) {
        let validator_set_updates_bytes: ValidatorSetUpdatesBytes = validator_set_updates.into();
        self.0.set(
            &combine(&PENDING_VALIDATOR_SET_UPDATES, block),
            &validator_set_updates_bytes.try_to_vec().unwrap(),
        )
    }

    pub fn delete_pending_validator_set_updates(&mut self, block: &CryptoHash) {
        self.0
            .delete(&combine(&PENDING_VALIDATOR_SET_UPDATES, block))
    }

    /* ↓↓↓ Locked View ↓↓↓ */

    pub fn set_locked_view(&mut self, view: ViewNumber) {
        self.0.set(&LOCKED_VIEW, &view.try_to_vec().unwrap())
    }

    /* ↓↓↓ Highest View Entered ↓↓↓ */

    pub fn set_highest_view_entered(&mut self, view: ViewNumber) {
        self.0
            .set(&HIGHEST_VIEW_ENTERED, &view.try_to_vec().unwrap())
    }

    /* ↓↓↓ Highest Quorum Certificate ↓↓↓ */

    pub fn set_highest_qc(&mut self, qc: &QuorumCertificate) {
        self.0.set(&HIGHEST_QC, &qc.try_to_vec().unwrap())
    }

    /* ↓↓↓ Highest Committed Block ↓↓↓ */

    pub fn set_highest_committed_block(&mut self, block: &CryptoHash) {
        self.0
            .set(&HIGHEST_COMMITTED_BLOCK, &block.try_to_vec().unwrap())
    }

    /* ↓↓↓ Newest Block ↓↓↓ */

    pub fn set_newest_block(&mut self, block: &CryptoHash) {
        self.0.set(&NEWEST_BLOCK, &block.try_to_vec().unwrap())
    }
}

#[derive(Clone)]
pub struct BlockTreeCamera<K: KVStore>(K);

impl<K: KVStore> BlockTreeCamera<K> {
    pub fn new(kv_store: K) -> Self {
        BlockTreeCamera(kv_store)
    }

    pub fn snapshot(&self) -> BlockTreeSnapshot<K::Snapshot<'_>> {
        BlockTreeSnapshot(self.0.snapshot())
    }
}

/// A read view into the block tree that is guaranteed to stay unchanged.
pub struct BlockTreeSnapshot<S: KVGet>(S);

impl<S: KVGet> BlockTreeSnapshot<S> {
    pub(crate) fn new(kv_snapshot: S) -> Self {
        BlockTreeSnapshot(kv_snapshot)
    }

    /* ↓↓↓ Used for syncing ↓↓↓ */

    /// Get a chain of blocks starting from the specified tail block and going towards the newest block, up until the limit.
    ///
    /// If tail is None, then the chain starts from genesis instead.
    pub(crate) fn blocks_from_height_to_newest(
        &self,
        height: BlockHeight,
        limit: u32,
    ) -> Vec<Block> {
        let mut res = Vec::with_capacity(limit as usize);

        // Get committed blocks starting from the specified height.
        let mut cursor = height;
        while let Some(block_hash) = self.block_at_height(cursor) {
            res.push(self.block(&block_hash).unwrap());
            cursor += 1;

            if res.len() == limit as usize {
                return res;
            }
        }

        // Get speculative blocks.
        let speculative_blocks = self.blocks_from_newest_to_committed().into_iter().rev();
        for block in speculative_blocks {
            res.push(block);

            if res.len() == limit as usize {
                break;
            }
        }

        res
    }

    // Get a chain of blocks from the newest block up to (but not including) the highest committed block, or genesis.
    // The returned chain goes from blocks of higher height (newest block) to blocks of lower height.
    fn blocks_from_newest_to_committed(&self) -> Vec<Block> {
        let mut res = Vec::new();
        if let Some(newest_block) = self.newest_block() {
            let mut cursor = newest_block;
            loop {
                let block = self.block(&cursor).unwrap();
                let block_justify = block.justify.clone();
                res.push(block);

                if let Some(highest_committed_block) = self.highest_committed_block() {
                    if block_justify.block == highest_committed_block {
                        break;
                    }
                }

                if block_justify == QuorumCertificate::genesis_qc() {
                    break;
                }

                cursor = block_justify.block;
            }
        }

        res
    }
}

pub trait KVStore: KVGet + Clone + Send + 'static {
    type WriteBatch: WriteBatch;
    type Snapshot<'a>: 'a + KVGet;

    fn write(&mut self, wb: Self::WriteBatch);
    fn clear(&mut self);
    fn snapshot<'b>(&'b self) -> Self::Snapshot<'_>;
}

pub trait WriteBatch {
    fn new() -> Self;
    fn set(&mut self, key: &[u8], value: &[u8]);
    fn delete(&mut self, key: &[u8]);
}

// Causes the getter methods defined by default for implementors of KVGet to also be public methods
// of BlockTree and BlockTreeCamera.
macro_rules! re_export_getters_from_block_tree_and_block_tree_snapshot {
    ($self:ident, pub trait KVGet {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>>;

        $(fn $f_name:ident(&self$(,)? $($param_name:ident: $param_type:ty),*) -> $return_type:ty $body:block)*
    })
    => {
        pub trait KVGet {
            fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
            $(fn $f_name(&$self, $($param_name: $param_type),*) -> $return_type $body)*
        }

        impl<K: KVStore> BlockTree<K> {
            $(pub fn $f_name(&self, $($param_name: $param_type),*) -> $return_type {
                self.0.$f_name($($param_name),*)
            })*
        }

        impl<S: KVGet> BlockTreeSnapshot<S> {
            $(pub fn $f_name(&self, $($param_name: $param_type),*) -> $return_type {
                self.0.$f_name($($param_name),*)
            })*
        }
    }
}

re_export_getters_from_block_tree_and_block_tree_snapshot!(
    self,
    pub trait KVGet {
        fn get(&self, key: &[u8]) -> Option<Vec<u8>>;

        /* ↓↓↓ Block ↓↓↓  */

        fn block(&self, block: &CryptoHash) -> Option<Block> {
            let height = self.block_height(block)?; // Safety: if block height is Some, then all of the following fields are Some too.
            let justify = self.block_justify(block).unwrap();
            let data_hash = self.block_data_hash(block).unwrap();
            let data = self.block_data(block).unwrap();

            Some(Block {
                height,
                hash: *block,
                justify,
                data_hash,
                data,
            })
        }

        fn block_height(&self, block: &CryptoHash) -> Option<BlockHeight> {
            let block_key = combine(&BLOCKS, block);
            let block_height_key = combine(&block_key, &BLOCK_HEIGHT);
            let block_height = {
                let bs = self.get(&block_height_key)?;
                BlockHeight::deserialize(&mut bs.as_slice()).unwrap()
            };
            Some(block_height)
        }

        fn block_justify(&self, block: &CryptoHash) -> Option<QuorumCertificate> {
            Some(
                QuorumCertificate::deserialize(
                    &mut &*self.get(&combine(&BLOCKS, &combine(block, &BLOCK_JUSTIFY)))?,
                )
                .unwrap(),
            )
        }

        fn block_data_hash(&self, block: &CryptoHash) -> Option<CryptoHash> {
            Some(
                CryptoHash::deserialize(
                    &mut &*self.get(&combine(&BLOCKS, &combine(block, &BLOCK_DATA_HASH)))?,
                )
                .unwrap(),
            )
        }

        fn block_data_len(&self, block: &CryptoHash) -> Option<DataLen> {
            Some(
                DataLen::deserialize(
                    &mut &*self.get(&combine(&BLOCKS, &combine(block, &BLOCK_DATA_LEN)))?,
                )
                .unwrap(),
            )
        }

        fn block_data(&self, block: &CryptoHash) -> Option<Data> {
            let data_len = self.block_data_len(block)?;
            let data = (0..data_len)
                .map(|i| self.block_datum(block, i).unwrap())
                .collect();

            Some(data)
        }

        fn block_datum(&self, block: &CryptoHash, datum_index: u32) -> Option<Datum> {
            let block_data_prefix = combine(&BLOCKS, &combine(block, &BLOCK_DATA));
            self.get(&combine(
                &block_data_prefix,
                &datum_index.try_to_vec().unwrap(),
            ))
        }

        /* ↓↓↓ Block Height to Block ↓↓↓ */

        fn block_at_height(&self, height: BlockHeight) -> Option<CryptoHash> {
            let block_hash_key = combine(&BLOCK_AT_HEIGHT, &height.to_le_bytes());
            let block_hash = {
                let bs = self.get(&block_hash_key)?;
                CryptoHash::deserialize(&mut bs.as_slice()).unwrap()
            };

            Some(block_hash)
        }

        /* ↓↓↓ Block to Children ↓↓↓ */

        fn children(&self, block: &CryptoHash) -> Option<ChildrenList> {
            Some(
                ChildrenList::deserialize(&mut &*self.get(&combine(&BLOCK_TO_CHILDREN, block))?)
                    .unwrap(),
            )
        }

        /* ↓↓↓ Committed App State ↓↓↓ */

        fn committed_app_state(&self, key: &[u8]) -> Option<Vec<u8>> {
            self.get(&combine(&COMMITTED_APP_STATE, key))
        }

        /* ↓↓↓ Pending App State Updates ↓↓↓ */

        fn pending_app_state_updates(&self, block: &CryptoHash) -> Option<AppStateUpdates> {
            Some(
                AppStateUpdates::deserialize(
                    &mut &*self.get(&combine(&PENDING_APP_STATE_UPDATES, block))?,
                )
                .unwrap(),
            )
        }

        /* ↓↓↓ Commmitted Validator Set */

        fn committed_validator_set(&self) -> ValidatorSet {
            let validator_set_bytes = ValidatorSetBytes::deserialize(&mut &*self.get(&COMMITTED_VALIDATOR_SET).unwrap()).unwrap();
            ValidatorSet::try_from(validator_set_bytes).unwrap() //error should not happen so we unwrap
        }

        /* ↓↓↓ Pending Validator Set Updates */

        fn pending_validator_set_updates(&self, block: &CryptoHash) -> Option<ValidatorSetUpdates> {
            ValidatorSetUpdatesBytes::deserialize(&mut &*self.get(&combine(&PENDING_VALIDATOR_SET_UPDATES, block))?,)
            .ok()
            .map(|validator_set_updates_bytes| ValidatorSetUpdates::try_from(validator_set_updates_bytes).unwrap())
        }

        /* ↓↓↓ Locked View ↓↓↓ */

        fn locked_view(&self) -> ViewNumber {
            ViewNumber::deserialize(&mut &*self.get(&LOCKED_VIEW).unwrap()).unwrap()
        }

        /* ↓↓↓ Highest View Entered ↓↓↓ */

        fn highest_view_entered(&self) -> ViewNumber {
            ViewNumber::deserialize(&mut &*self.get(&HIGHEST_VIEW_ENTERED).unwrap()).unwrap()
        }

        /* ↓↓↓ Highest Quorum Certificate ↓↓↓ */

        fn highest_qc(&self) -> QuorumCertificate {
            QuorumCertificate::deserialize(&mut &*self.get(&HIGHEST_QC).unwrap()).unwrap()
        }

        /* ↓↓↓ Highest Committed Block ↓↓↓ */

        fn highest_committed_block(&self) -> Option<CryptoHash> {
            Some(CryptoHash::deserialize(&mut &*self.get(&HIGHEST_COMMITTED_BLOCK)?).unwrap())
        }

        /* ↓↓↓ Newest Block ↓↓↓ */

        fn newest_block(&self) -> Option<CryptoHash> {
            Some(CryptoHash::deserialize(&mut &*self.get(&NEWEST_BLOCK)?).unwrap())
        }
    }
);

mod paths {
    // State variables
    pub(super) const BLOCKS: [u8; 1] = [0];
    pub(super) const BLOCK_AT_HEIGHT: [u8; 1] = [1];
    pub(super) const BLOCK_TO_CHILDREN: [u8; 1] = [2];
    pub(super) const COMMITTED_APP_STATE: [u8; 1] = [3];
    pub(super) const PENDING_APP_STATE_UPDATES: [u8; 1] = [4];
    pub(super) const COMMITTED_VALIDATOR_SET: [u8; 1] = [5];
    pub(super) const PENDING_VALIDATOR_SET_UPDATES: [u8; 1] = [6];
    pub(super) const LOCKED_VIEW: [u8; 1] = [7];
    pub(super) const HIGHEST_VIEW_ENTERED: [u8; 1] = [8];
    pub(super) const HIGHEST_QC: [u8; 1] = [9];
    pub(super) const HIGHEST_COMMITTED_BLOCK: [u8; 1] = [10];
    pub(super) const NEWEST_BLOCK: [u8; 1] = [11];

    // Fields of Block
    pub(super) const BLOCK_HEIGHT: [u8; 1] = [0];
    pub(super) const BLOCK_JUSTIFY: [u8; 1] = [1];
    pub(super) const BLOCK_DATA_HASH: [u8; 1] = [2];
    pub(super) const BLOCK_DATA_LEN: [u8; 1] = [3];
    pub(super) const BLOCK_DATA: [u8; 1] = [4];
}

/// Takes references to two byteslices and returns a vector containing the bytes of the first one, and then the bytes of the
/// second one.
fn combine(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut res = Vec::with_capacity(a.len() + b.len());
    res.extend_from_slice(a);
    res.extend_from_slice(b);
    res
}
