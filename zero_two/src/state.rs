/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! This module defines types and methods used to access and mutate the persistent state that a HotStuff-rs
//! validator keeps track for the operation of the protocol, and for its application.
//! 
//! This state may be stored in a key-value store of the library user's own choosing, as long as that KV store
//! can provide a type that implements [KVStore]. This state can be mutated through an instance of [BlockTree],
//! and read through an instance of [BlockTreeSnapshot], which can be created using [BlockTreeCamera]. 
//! 
//! In normal operation, HotStuff-rs code will internally be making all writes to the Block Tree, and users can
//! get a [BlockTreeCamera] using [crate::replica::Replica]'s [crate::replica::Replica::block_tree_camera] method.
//! However, users may sometimes want to manually mutate the Block Tree, for example, to recover from an error
//! that has corrupted its invariants. For this purpose, one can unsafe-ly get an instance of BlockTree using
//! [BlockTree::new_unsafe] and an instance of the corresponding [BlockTreeWriteBatch] using 
//! [BlockTreeWriteBatch::new_unsafe].
//! 
//! ## State variables
//! 
//! HotStuff-rs structures its state into separate conceptual 'variables' which are stored in tuples that sit
//! at a particular key path or prefix in the library user's chosen KV store. These variables are:
//! - **Blocks** ([CryptoHash] -> [Block]).
//! - **Block Height to Block** ([BlockHeight] -> [CryptoHash]): a mapping between a block's number and a block's hash. This mapping only contains blocks that are committed, because if a block hasn't been committed, there may be multiple blocks at the same height.
//! - **Block to Children** ([CryptoHash] -> [ChildrenList]): a mapping between a block's hash and the children it has in the block tree. A block may have multiple chilren if they have not been committed.
//! - **Committed App State** ([Key] -> [Value]).
//! - **Pending App State Updates** ([CryptoHash] -> [AppStateUpdates]).
//! - **Committed Validator Set** ([ValidatorSet]).
//! - **Pending Validator Set Updates** ([CryptoHash] -> [ValidatorSetUpdates]).
//! - **Locked View** ([ViewNumber]): the highest view number of a quorum certificate contained in a block that has a child. 
//! - **Highest View Entered** ([ViewNumber]): the highest view that this validator has entered.
//! - **Highest Quorum Certificate** ([QuorumCertificate]): among the quorum certificates this validator has seen and verified the signatures of, the one with the highest view number. 
//! - **Highest Committed Block** ([CryptoHash]).
//! - **Newest Block** ([CryptoHash]): the most recent block to be inserted into the block tree.
//! 
//! The location of each of these variables in a KV store is defined in [paths]. Note that the fields of a
//! block are itself stored in different tuples. This is so that user code can get a subset of a block's data
//! without loading the entire block from storage, which may be expensive. The key suffixes on which each of
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
//! 
//! ## Safety

use borsh::{BorshSerialize, BorshDeserialize};
use crate::types::*;

/// A read and write handle into the block tree, exclusively owned by the algorithm thread.
pub struct BlockTree<K: KVStore>(K);

impl<K: KVStore> BlockTree<K> {
    pub(crate) fn new(kv_store: K) -> Self {
        BlockTree(kv_store)
    } 

    pub unsafe fn new_unsafe(kv_store: K) -> Self {
        Self::new(kv_store)
    }

    /* ↓↓↓ Initialize ↓↓↓ */

    pub(crate) fn initialize(&mut self, initial_app_state: &AppStateUpdates, initial_validator_set: &ValidatorSetUpdates) {
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

    // Returns whether a block can be safely inserted. For this, it is necessary that:
    // 1. self.qc_can_be_inserted(&block.justify).
    // 2. No block with the same block hash is already in the block tree. 
    // 3. Its qc's must be either a generic qc or a commit qc.
    //
    // This function evaluates [Self::qc_can_be_inserted], then checks 2 and 3.
    //
    // # Precondition
    // [Block::is_correct]
    pub(crate) fn block_can_be_inserted(&self, block: &Block) -> bool {
        /* 1 */ self.qc_can_be_inserted(&block.justify) &&
        /* 2 */ self.contains(&block.hash)  &&
        /* 3 */ block.justify.phase == Phase::Commit && block.justify.phase == Phase::Generic
    }

    // If the insertion causes a block to be committed, returns the changes that this causes of the validator set, if any.
    // 
    // # Precondition
    // [Self::block_can_be_inserted]
    pub(crate) fn insert_block(
        &mut self, 
        block: &Block, 
        app_state_updates: Option<&AppStateUpdates>, 
        validator_set_updates: Option<&ValidatorSetUpdates>
    ) -> Option<ValidatorSetUpdates> {
        let mut wb = BlockTreeWriteBatch::new();

        // 1. Block: Insert block.
        wb.set_block(block);
        wb.set_newest_block(&block.hash);
        wb.set_pending_app_state_updates(&block.hash, app_state_updates);
        wb.set_pending_validator_set_updates(&block.hash, validator_set_updates);

        let mut siblings = self.children(&block.justify.block).unwrap_or(ChildrenList::new());
        siblings.push(block.hash);
        wb.set_children_list(&block.justify.block, &siblings);

        if self.qc_can_replace_highest(&block.justify) {
            wb.set_highest_qc(&block.justify);
        }

        if block.justify.is_genesis_qc() {
            self.write(wb);
            return None
        }

        // 2. Parent: consider updating locked view with parent's justify's view number.
        let parent = block.justify.block;
        let parent_justify_view = self.0.block_justify(&parent).unwrap().view;
        if parent_justify_view > self.locked_view() {
            wb.set_locked_view(parent_justify_view);
        }

        let parent_justify = self.0.block_justify(&parent).unwrap(); 
        if parent_justify.is_genesis_qc() {
            self.write(wb);
            return None
        } 

        // 3. Grandparent: do nothing with it.
        let grandparent = parent_justify.block;
        let grandparent_justify = self.0.block_justify(&grandparent).unwrap();
        if grandparent_justify.is_genesis_qc() {
            self.write(wb);
            return None
        }

        // 4. Great-grandparent: if great-grandparent is higher than the current highest committed block, commit it. 
        let great_grandparent = grandparent_justify.block;
        let great_grandparent_height = self.0.block_height(&great_grandparent).unwrap();

        let validator_set_changes = match self.highest_committed_block_height() {
            Some(highest_committed_block_height) if great_grandparent_height > highest_committed_block_height => self.commit_block(&mut wb, &great_grandparent),
            None => self.commit_block(&mut wb, &great_grandparent),
            _ => {
                self.write(wb);
                return None
            }
        };

        let great_grandparent_justify = self.0.block_justify(&great_grandparent).unwrap();
        if great_grandparent_justify.is_genesis_qc() {
            self.write(wb);
            return validator_set_changes 
        }

        // 4. Great-great-grandparent: delete all children of except great-grandparent.
        let great_great_grandparent = great_grandparent_justify.block;
        self.delete_children_of(&mut wb, &great_great_grandparent, &great_grandparent);

        self.write(wb);
        return validator_set_changes
    } 

    // Returns whether a qc be inserted into the block tree as part of a block. For this, it is necessary that:
    // 1. It justifies a known block.
    // 2. Its view number is greater than or equal to locked view.
    // 3. If it is a prepare, precommit, or commit qc, the block it justifies has a pending validator state update.
    // 4. If its qc is a generic qc, the block it justifies *does not* have a pending validator set update.
    //
    // # Precondition
    // [QuorumCertificate::is_correct]
    pub(crate) fn qc_can_be_inserted(&self, qc: &QuorumCertificate) -> bool {
        (self.contains(&qc.block) || qc.is_genesis_qc()) &&
        qc.view >= self.locked_view() &&
        ((qc.phase == Phase::Prepare || qc.phase == Phase::Precommit || qc.phase == Phase::Commit) && self.pending_validator_set_updates(&qc.block).is_some()) &&
        (qc.phase == Phase::Generic && self.pending_validator_set_updates(&qc.block).is_none())
    }

    // Returns whether a qc can be set as the highest qc in the block tree. For this, it is necessary that:
    // 1. self.qc_can_be_inserted(qc).
    // 2. Its view number is greater than the view number of the current highest qc.
    // 
    // This function evaluates [Self::qc_can_be_inserted], and then checks 2.
    //
    // # Precondition
    // [QuorumCertificate::is_correct]
    pub(crate) fn qc_can_replace_highest(&self, qc: &QuorumCertificate) -> bool {
        self.qc_can_be_inserted(qc) &&
        qc.view > self.highest_qc().view
    }

    pub(crate) fn set_highest_qc(&mut self, qc: &QuorumCertificate) {
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_qc(qc);
        self.write(wb);
    }

    pub(crate) fn set_highest_entered_view(&mut self, view: ViewNumber) { 
        let mut wb = BlockTreeWriteBatch::new();
        wb.set_highest_view_entered(view);
        self.write(wb);
    }

    /* ↓↓↓ For committing a block in insert_block ↓↓↓ */

    // Returns the validator set updates that become committed when the block becomes committed, if any.
    fn commit_block(&mut self, wb: &mut BlockTreeWriteBatch<K::WriteBatch>, block: &CryptoHash) -> Option<ValidatorSetUpdates> {
        if let Some(pending_app_state_updates) = self.pending_app_state_updates(block) {
            wb.apply_app_state_updates(&pending_app_state_updates); 
            wb.delete_pending_app_state_updates(block);
        }
        

        if let Some(pending_validator_set_updates) = self.pending_validator_set_updates(block) {
            let mut committed_validator_set = self.committed_validator_set();
            committed_validator_set.apply_updates(&pending_validator_set_updates);

            wb.set_committed_validator_set(&committed_validator_set);
            wb.delete_pending_validator_set_updates(block);

            return Some(pending_validator_set_updates)
        }

        None
    }

    /* ↓↓↓ For deleting abandoned branches in insert_block ↓↓↓ */

    fn delete_children_of(&mut self, wb: &mut BlockTreeWriteBatch<K::WriteBatch>, block: &CryptoHash, except: &CryptoHash) {
        for sibling in self.children(&block).unwrap().iter().filter(|sib| *sib != except) {
            self.delete_branch(wb, sibling);
        }

        wb.set_children_list(&block, &vec![*block]);
    }

    fn delete_branch(&mut self, wb: &mut BlockTreeWriteBatch<K::WriteBatch>, tail: &CryptoHash) {
        if let Some(children) = self.children(tail) {
            for child in children {
                self.delete_branch(wb, &child);
            }
        }

        wb.delete_children_list(tail);
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
            }    
        }

        let parent_app_state_updates = self.pending_app_state_updates(parent.unwrap());
        let parent_justify = self.block_justify(parent.unwrap()).unwrap();

        if parent_justify.is_genesis_qc() {
            return AppBlockTreeView {
                block_tree: self,
                parent_app_state_updates,
                grandparent_app_state_updates: None,
                great_grandparent_app_state_updates: None,
            } 
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
            }
        }

        let great_grandparent = grandparent_justify.block;
        let great_grandparent_app_state_updates = self.pending_app_state_updates(&great_grandparent);

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
                return None
            } else if let Some(value) = parent_app_state_updates.get_insert(&key.to_vec()) {
                return Some(value.clone())
            }
        }
        
        if let Some(grandparent_app_state_changes) = &self.grandparent_app_state_updates {
            if grandparent_app_state_changes.contains_delete(&key.to_vec()) {
                return None
            } else if let Some(value) = grandparent_app_state_changes.get_insert(&key.to_vec()) {
                return Some(value.clone())
            }
        }

        if let Some(great_grandparent_app_state_changes) = &self.great_grandparent_app_state_updates {
            if great_grandparent_app_state_changes.contains_delete(&key.to_vec()) {
                return None
            } else if let Some(value) = great_grandparent_app_state_changes.get_insert(&key.to_vec()) {
                return Some(value.clone())
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
    fn new() -> BlockTreeWriteBatch<W> {
        BlockTreeWriteBatch(W::new())
    }

    pub fn new_unsafe() -> BlockTreeWriteBatch<W> {
        Self::new()
    }

    /* ↓↓↓ Block ↓↓↓  */

    pub fn set_block(&mut self, block: &Block) {
        let block_prefix = combine(&BLOCKS, &block.hash); 

        self.0.set(&combine(&block_prefix, &BLOCK_HEIGHT), &block.height.try_to_vec().unwrap());
        self.0.set(&combine(&block_prefix, &BLOCK_JUSTIFY), &block.justify.try_to_vec().unwrap());
        self.0.set(&combine(&block_prefix, &BLOCK_DATA_HASH), &block.data_hash.try_to_vec().unwrap());
        self.0.set(&combine(&block_prefix, &BLOCK_DATA_LEN), &block.data.len().try_to_vec().unwrap());

        // Insert datums.
        let block_data_prefix = combine(&block_prefix, &BLOCK_DATA);
        for (i, datum) in block.data.iter().enumerate() {
            let datum_key = combine(&block_data_prefix, &i.try_to_vec().unwrap());
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

    /* ↓↓↓ Block Height to Block ↓↓↓ */

    pub fn set_block_height_to_block(&mut self, height: BlockHeight, block: &CryptoHash) {
        let block_prefix = combine(&BLOCKS, block);

        self.0.set(&combine(&block_prefix, &BLOCK_HEIGHT_TO_HASH), &height.try_to_vec().unwrap());
    } 

    /* ↓↓↓ Block to Children ↓↓↓ */

    pub fn set_children_list(&mut self, block: &CryptoHash, children: &ChildrenList) {
        self.0.set(&combine(&BLOCK_HASH_TO_CHILDREN, block), &children.try_to_vec().unwrap());
    }

    pub fn delete_children_list(&mut self, block: &CryptoHash) {
        self.0.delete(&combine(&BLOCK_HASH_TO_CHILDREN, block));
    }

    /* ↓↓↓ Committed App State ↓↓↓ */ 

    pub fn set_committed_app_state(&mut self, key: &[u8], value: &[u8]) {
        self.0.set(&combine(&COMMITTED_APP_STATE, key), value);
    }

    pub fn delete_committed_app_state(&mut self, key: &[u8]) {
        self.0.delete(&combine(&COMMITTED_APP_STATE, key));
    } 

    /* ↓↓↓ Pending App State Updates ↓↓↓ */

    pub fn set_pending_app_state_updates(&mut self, block: &CryptoHash, app_state_updates: Option<&AppStateUpdates>) {
        self.0.set(&combine(block, &PENDING_APP_STATE_UPDATES), &app_state_updates.try_to_vec().unwrap());
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
        self.0.delete(&combine(block, &PENDING_APP_STATE_UPDATES));
    }

    /* ↓↓↓ Commmitted Validator Set */

    pub fn set_committed_validator_set(&mut self, validator_set: &ValidatorSet) {
        self.0.set(&COMMITTED_VALIDATOR_SET, &validator_set.try_to_vec().unwrap())
    } 

    /* ↓↓↓ Pending Validator Set Updates */

    pub fn set_pending_validator_set_updates(&mut self, block: &CryptoHash, validator_set_updates: Option<&ValidatorSetUpdates>) {
        self.0.set(&combine(&PENDING_VALIDATOR_SET_UPDATES, block), &validator_set_updates.try_to_vec().unwrap())
    }

    pub fn delete_pending_validator_set_updates(&mut self, block: &CryptoHash) {
        self.0.delete(&combine(&PENDING_VALIDATOR_SET_UPDATES, block))
    }

    /* ↓↓↓ Locked View ↓↓↓ */

    pub fn set_locked_view(&mut self, view: ViewNumber) {
        self.0.set(&LOCKED_VIEW, &view.try_to_vec().unwrap())
    }

    /* ↓↓↓ Highest View Entered ↓↓↓ */

    pub fn set_highest_view_entered(&mut self, view: ViewNumber) {
        self.0.set(&HIGHEST_VIEW_ENTERED, &view.try_to_vec().unwrap())
    }

    /* ↓↓↓ Highest Quorum Certificate ↓↓↓ */

    pub fn set_highest_qc(&mut self, qc: &QuorumCertificate) {
        self.0.set(&HIGHEST_QC, &qc.try_to_vec().unwrap())
    }
    
    /* ↓↓↓ Highest Committed Block ↓↓↓ */
    
    pub fn set_highest_committed_block(&mut self, block: &CryptoHash) {
        self.0.set(&HIGHEST_COMMITTED_BLOCK, &block.try_to_vec().unwrap())
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

    pub(crate) fn blocks_from_tail_to_newest_block(&self, tail: Option<&CryptoHash>, limit: u32) -> Vec<Block> {
        let mut res = Vec::with_capacity(limit as usize);

        // Get committed blocks, extending from tail.
        if let Some(tail) = tail {
            if let Some(tail_block) = self.block(tail) {
                res.push(tail_block);
    
                while res.len() < limit as usize {
                    let child = match self.block_at_height(res.last().unwrap().height + 1) {
                        Some(block_hash) => self.block(&block_hash).unwrap(),
                        None => break,
                    };
                    res.push(child);
                }
            } else {
                return Vec::new()
            }
        }

        // Get speculative blocks.
        if res.len() < limit as usize {
            if let Some(newest_block) = self.newest_block() {
                let uncommitted_blocks = self.chain_between_speculative_block_and_highest_committed_block(&newest_block).into_iter().rev();
                res.extend(uncommitted_blocks);
            }
        }

        res
    }

    fn chain_between_speculative_block_and_highest_committed_block(&self, head_block: &CryptoHash) -> Vec<Block> {
        let mut res = Vec::new();

        let head_block = self.block(head_block).unwrap();
        if let Some(highest_committed_block) = self.highest_committed_block() {
            let mut cursor = head_block.justify.block;
            res.push(head_block);

            while cursor != highest_committed_block {
                let block = self.block(&cursor).unwrap();
                cursor = block.justify.block;
                res.push(block);
            }
        };

        res
    }
}

pub trait KVStore: KVGet + Clone + Send + 'static {
    type WriteBatch: WriteBatch;
    type Snapshot<'a>: 'a + KVGet;

    fn write(&mut self, wb: Self::WriteBatch);
    fn clear(&mut self);
    fn snapshot<'b>(&'b self) -> Self::Snapshot<'b>;
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

re_export_getters_from_block_tree_and_block_tree_snapshot!(self,
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
            data
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
        Some(QuorumCertificate::deserialize(&mut &*self.get(&combine(&BLOCKS, &combine(block, &BLOCK_JUSTIFY)))?).unwrap())
    }

    fn block_data_hash(&self, block: &CryptoHash) -> Option<CryptoHash> {
        Some(CryptoHash::deserialize(&mut &*self.get(&combine(&BLOCKS, &combine(block, &BLOCK_DATA_HASH)))?).unwrap())
    }

    fn block_data_len(&self, block: &CryptoHash) -> Option<DataLen> {
        Some(DataLen::deserialize(&mut &*self.get(&combine(&BLOCKS, &combine(block, &BLOCK_DATA_LEN)))?).unwrap())
    }

    fn block_data(&self, block: &CryptoHash) -> Option<Data> {
        let block_data_prefix = combine(&BLOCKS, &combine(block, &BLOCK_DATA));

        let data_len = self.block_data_len(block)?;
        let data = (0..data_len).map(|i| self.get(&combine(&block_data_prefix, &i.try_to_vec().unwrap())).unwrap()).collect();

        Some(data)
    }

    fn block_datum(&self, block: &CryptoHash, datum_index: u32) -> Option<Datum> {
        let block_data_prefix = combine(&BLOCKS, &combine(block, &BLOCK_DATA));
        self.get(&combine(&block_data_prefix, &datum_index.try_to_vec().unwrap()))
    } 

    /* ↓↓↓ Block Height to Block ↓↓↓ */

    fn block_at_height(&self, height: BlockHeight) -> Option<CryptoHash> {
        let block_hash_key = combine(&BLOCK_HEIGHT_TO_HASH, &height.to_le_bytes());
        let block_hash = {
            let bs = self.get(&block_hash_key)?;
            CryptoHash::deserialize(&mut bs.as_slice()).unwrap()
        };

        Some(block_hash)
    }

    /* ↓↓↓ Block to Children ↓↓↓ */

    fn children(&self, block: &CryptoHash) -> Option<ChildrenList> {
        Some(ChildrenList::deserialize(&mut &*self.get(&combine(&BLOCK_HASH_TO_CHILDREN, block))?).unwrap())
    }

    /* ↓↓↓ Committed App State ↓↓↓ */

    fn committed_app_state(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.get(&combine(&COMMITTED_APP_STATE, key))
    } 

    /* ↓↓↓ Pending App State Updates ↓↓↓ */

    fn pending_app_state_updates(&self, block: &CryptoHash) -> Option<AppStateUpdates> {
        Some(AppStateUpdates::deserialize(&mut &*self.get(&combine(&PENDING_APP_STATE_UPDATES, block))?).unwrap())
    }

    /* ↓↓↓ Commmitted Validator Set */

    fn committed_validator_set(&self) -> ValidatorSet {
        ValidatorSet::deserialize(&mut &*self.get(&COMMITTED_VALIDATOR_SET).unwrap()).unwrap()
    } 

    /* ↓↓↓ Pending Validator Set Updates */

    fn pending_validator_set_updates(&self, block: &CryptoHash) -> Option<ValidatorSetUpdates> {
        Some(ValidatorSetUpdates::deserialize(&mut &*self.get(&combine(&PENDING_APP_STATE_UPDATES, block))?).unwrap())
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
    pub(super) const BLOCK_HEIGHT_TO_HASH: [u8; 1] = [1]; 
    pub(super) const BLOCK_HASH_TO_CHILDREN: [u8; 1] = [2];
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
    pub(super) const BLOCK_JUSTIFY: [u8; 1] = [02];
    pub(super) const BLOCK_DATA_HASH: [u8; 1] = [03];
    pub(super) const BLOCK_DATA_LEN: [u8; 1] = [03];
    pub(super) const BLOCK_DATA: [u8; 1] = [04];
}

/// Takes references to two byteslices and returns a vector containing the bytes of the first one, and then the bytes of the 
/// second one.
fn combine(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut res = Vec::with_capacity(a.len() + b.len());
    res.extend_from_slice(a);
    res.extend_from_slice(b);
    res
}