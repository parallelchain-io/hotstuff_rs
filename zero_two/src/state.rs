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
//! and read through an instance of [BlockTreeSnapshot].
//! 
//! HotStuff-rs structures its state into separate conceptual 'variables' which are stored in tuples that sit
//! at a particular key path or prefix in the library user's chosen KV store. These variables are:
//! - **Blocks** ([CryptoHash] -> [Block]).
//! - **Block Height to Block** ([BlockHeight] -> [CryptoHash]): a mapping between a block's number and a block's hash. This mapping only contains blocks that are committed, because if a block hasn't been committed, there may be multiple blocks at the same height.
//! - **Block Hash to Children** ([CryptoHash] -> [ChildrenList]): a mapping between a block's hash and the children it has in the block tree. A block may have multiple chilren if they have not been committed.
//! - **Committed App State** ([Key] -> [Value]).
//! - **Committed Validator Set** ([ValidatorSet]).
//! - **Pending App State Updates** ([CryptoHash] -> [AppStateUpdates]).
//! - **Pending Validator Set Updates** ([CryptoHash] -> [ValidatorSetUpdates]).
//! - **Locked View** ([ViewNumber]): the highest view number of a quorum certificate contained in a block that has a child. 
//! - **Highest View Entered** ([ViewNumber]): the highest view that this validator has entered.
//! - **Highest Quorum Certificate** ([QuorumCertificate]): among the quorum certificates this validator has seen and verified the signatures of, the one with the highest view number. 
//! - **Highest Committed Block Hash** ([CryptoHash]).
//! 
//! The location of each of these variables in a KV store is defined in [kv_paths]. Note that the fields of a
//! block are itself stored in different tuples. This is so that user code can get a subset of a block's data
//! without loading the entire block from storage, which may be expensive. The key suffixes on which each of
//! block's fields are stored are also defined in [kv_paths].
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

use borsh::BorshSerialize;

use crate::types::*;
use std::marker::PhantomData;

/// A read and write handle into the block tree, exclusively owned by the algorithm thread.
pub(crate) struct BlockTree<'a, K: KVStore<'a>>(K, PhantomData<&'a ()>);

impl<'a, K: KVStore<'a> + KVGet> BlockTree<'a, K> {
    pub(crate) fn new(kv_store: K) -> BlockTree<'a, K> {
        BlockTree(kv_store, PhantomData)
    } 

    /* ↓↓↓ Initialize ↓↓↓ */

    pub(crate) fn initialize(&mut self, initial_app_state: &AppStateUpdates, initial_validator_set: &ValidatorSetUpdates) {

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

    // # Precondition
    // [Self::block_can_be_inserted]
    pub(crate) fn insert_block(
        &mut self, 
        block: &Block, 
        app_state_updates: Option<&AppStateUpdates>, 
        validator_set_updates: Option<&ValidatorSetUpdates>
    ) {
        let mut wb = BlockTreeWriteBatch::new();

        // 1. Block: Insert block.
        wb.set_block(block);
        wb.set_pending_app_state_updates(&block.hash, app_state_updates);
        wb.set_pending_validator_set_updates(&block.hash, validator_set_updates);

        let mut siblings = self.children(&block.justify.block).unwrap_or(ChildrenList::new());
        siblings.push(block.hash);
        wb.set_children_list(&block.justify.block, &siblings);

        if self.qc_can_be_inserted(&block.justify) {
            wb.set_highest_qc(&block.justify);
        }

        if block.justify.is_genesis_qc() {
            self.write(wb);
            return
        }

        // 2. Parent: consider updating locked view with parent's justify's view number.
        let parent = block.justify.block;
        let parent_justify_view = self.block_justify(&parent).unwrap().view;
        if parent_justify_view > self.locked_view() {
            wb.set_locked_view(parent_justify_view);
        }

        let parent_justify = self.block_justify(&parent).unwrap(); 
        if parent_justify.is_genesis_qc() {
            self.write(wb);
            return
        } 

        // 3. Grandparent: do nothing with it.
        let grandparent = parent_justify.block;
        let grandparent_justify = self.block_justify(&grandparent).unwrap();
        if grandparent_justify.is_genesis_qc() {
            self.write(wb);
            return
        }

        // 4. Great-grandparent: if great-grandparent is higher than the current highest committed block, commit it. 
        let great_grandparent = grandparent_justify.block;
        let great_grandparent_height = self.block_height(&great_grandparent).unwrap();

        match self.highest_committed_block_height() {
            Some(highest_committed_block_height) if great_grandparent_height > highest_committed_block_height => self.commit_block(&mut wb, &great_grandparent),
            None => self.commit_block(&mut wb, &great_grandparent),
            _ => {
                self.write(wb);
                return
            }
        }

        let great_grandparent_justify = self.block_justify(&great_grandparent).unwrap();
        if great_grandparent_justify.is_genesis_qc() {
            self.write(wb);
            return
        }

        // 4. Great-great-grandparent: delete all children of except great-grandparent.
        let great_great_grandparent = great_grandparent_justify.block;
        self.delete_children_of(&mut wb, &great_great_grandparent, &great_grandparent);

        self.write(wb);
    } 

    // Returns whether a qc can be safely set as highest_qc. For this, it is necessary that:
    // 1. It justifies a known block.
    // 2. Its view number is greater than or equal to locked view.
    // 3. If it is a prepare, precommit, or commit qc, the block it justifies has a pending validator state update.
    // 4. If its qc is a generic qc, the block it justifies *does not* have a pending validator set update.
    // # Precondition
    // [QuorumCertificate::is_correct]
    pub(crate) fn qc_can_be_inserted(&self, qc: &QuorumCertificate) -> bool {
        (self.contains(&qc.block) || qc.is_genesis_qc()) &&
        qc.view >= self.locked_view() &&
        ((qc.phase == Phase::Prepare || qc.phase == Phase::Precommit || qc.phase == Phase::Commit) && self.pending_validator_set_updates(&qc.block).is_some()) &&
        (qc.phase == Phase::Generic && self.pending_validator_set_updates(&qc.block).is_none())
    }
    pub(crate) fn set_highest_qc(&mut self, qc: &QuorumCertificate) {
        todo!()
    }

    /* ↓↓↓ Set highest view entered ↓↓↓ */
    
    pub(crate) fn set_highest_entered_view(&mut self, view: ViewNumber) {

    }

    /* ↓↓↓ State getters ↓↓↓ */

    pub(crate) fn contains(&self, block: &CryptoHash) -> bool {
        todo!()
    }

    pub(crate) fn highest_committed_block(&self) -> Option<CryptoHash> {
        todo!()
    }

    pub(crate) fn highest_committed_block_height(&self) -> Option<BlockHeight> {
        todo!()
    }

    pub(crate) fn highest_qc(&self) -> QuorumCertificate {
        todo!()
    }

    pub(crate) fn highest_entered_view(&self) -> ViewNumber {
        todo!()
    }

    pub(crate) fn committed_validator_set(&self) -> ValidatorSet {
        todo!()
    }

    /* ↓↓↓ Snapshot ↓↓↓ */

    pub(crate) fn snapshot(&self) -> BlockTreeSnapshot<K::Snapshot> {
        todo!()
    }

    /* ↓↓↓ WriteBatch commit ↓↓↓ */

    pub(crate) fn write(&mut self, write_batch: BlockTreeWriteBatch<K::WriteBatch>) {
        todo!()
    }

    /* ↓↓↓ For committing a block in insert_block ↓↓↓ */

    fn commit_block(&mut self, wb: &mut BlockTreeWriteBatch<K::WriteBatch>, block: &CryptoHash) {
        if let Some(pending_app_state_updates) = self.pending_app_state_updates(block) {
            for (key, value) in pending_app_state_updates.inserts() {
                wb.set_committed_app_state(key, value);
            }

            for key in pending_app_state_updates.deletes() {
                wb.delete_committed_app_state(key);
            }
        }
        wb.delete_pending_app_state_updates(block);
        
        if let Some(pending_validator_set_updates) = self.pending_validator_set_updates(block) {
            let committed_validator_set = self.committed_validator_set();
            for (peer, new_power) in pending_validator_set_updates {
                if new_power == 0 {
                    committed_validator_set.delete(&peer);
                } else {
                    committed_validator_set.put(&peer, new_power);
                }
            }
        }
        wb.delete_pending_validator_set_updates(block);
    }

    /* ↓↓↓ For deleting abandoned branches in insert_block ↓↓↓ */

    fn delete_children_of(&mut self, wb: &mut BlockTreeWriteBatch<K::WriteBatch>, block: &CryptoHash, except: &CryptoHash) {
        let siblings = self.children(&block).unwrap().iter().filter(|sib| *sib != except);
        for sibling in siblings {
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

    /* ↓↓↓ Private getters ↓↓↓ */

    fn block_height(&self, block: &CryptoHash) -> Option<BlockHeight> {
        todo!()
    }

    fn block_justify(&self, block: &CryptoHash) -> Option<QuorumCertificate> {
        todo!()
    }

    fn locked_view(&self) -> ViewNumber {
        todo!()
    }

    fn pending_app_state_updates(&self, block: &CryptoHash) -> Option<AppStateUpdates> {
        todo!()
    } 

    fn pending_validator_set_updates(&self, block: &CryptoHash) -> Option<ValidatorSetUpdates> {
        todo!()
    }

    fn children(&self, block: &CryptoHash) -> Option<ChildrenList> {
        todo!()
    }
}


pub(crate) struct BlockTreeWriteBatch<W: WriteBatch>(W);

use paths::*;
impl<W: WriteBatch> BlockTreeWriteBatch<W> {
    fn new() -> BlockTreeWriteBatch<W> {
        BlockTreeWriteBatch(W::new())
    }


    fn set_block(&mut self, block: &Block) {
        let block_prefix = combine(&BLOCKS, &block.hash); 

        self.0.set(&combine(&block_prefix, &BLOCK_HEIGHT), &block.height.try_to_vec().unwrap());
        self.0.set(&combine(&block_prefix, &BLOCK_JUSTIFY), &block.justify.try_to_vec().unwrap());
        self.0.set(&combine(&block_prefix, &BLOCK_DATA_HASH), &block.data_hash.try_to_vec().unwrap());
        self.0.set(&combine(&block_prefix, &BLOCK_DATA_LEN), &block.data.len().try_to_vec().unwrap());

        // Insert datums.
        let block_data_prefix = combine(&block_prefix, &BLOCK_DATA);
        for (i, datum) in block.data.iter().enumerate() {
            let datum_key = combine(&block_data_prefix, &i.to_le_bytes());
            self.0.set(&datum_key, datum);
        }
    }

    fn set_block_at_height(&mut self, height: BlockHeight, block: &CryptoHash) {
        let block_prefix = combine(&BLOCKS, block);

        self.0.set(&combine(&block_prefix, &BLOCK_HEIGHT_TO_HASH), &height.try_to_vec().unwrap());
    }

    fn delete_block(&mut self, block: &CryptoHash) {
        todo!()
    }

    fn set_highest_committed_block(&mut self, block: &CryptoHash) {
        todo!()
    }

    fn set_highest_qc(&mut self, qc: &QuorumCertificate) {
        todo!()
    }

    fn set_highest_entered_view(&mut self, view: ViewNumber) {
        todo!()
    }

    fn set_locked_view(&mut self, view: ViewNumber) {
        todo!()
    }

    fn set_committed_app_state(&mut self, key: &[u8], value: &[u8]) {
        todo!()
    }

    fn delete_committed_app_state(&mut self, key: &[u8]) {
        todo!()
    }

    fn set_committed_validator_set(&mut self, validator_set: &ValidatorSet) {
        todo!()
    }

    fn set_pending_app_state_updates(&mut self, block: &CryptoHash, app_state_updates: Option<&AppStateUpdates>) {
        todo!()
    }

    fn delete_pending_app_state_updates(&mut self, block: &CryptoHash) {
        todo!()
    }

    fn set_pending_validator_set_updates(&mut self, block: &CryptoHash, validator_set_updates: Option<&ValidatorSetUpdates>) {
        todo!()
    }

    fn delete_pending_validator_set_updates(&mut self, block: &CryptoHash) {
        todo!()
    }

    fn set_children_list(&mut self, block: &CryptoHash, children: &ChildrenList) {
        todo!()
    }

    fn delete_children_list(&mut self, block: &CryptoHash) {
        todo!()
    }
}

pub struct BlockTreeCamera<'a, K: KVStore<'a>>(K, PhantomData<&'a ()>);

impl<'a, K: KVStore<'a>> BlockTreeCamera<'a, K> {
    pub fn new(kv_store: K) -> BlockTreeCamera<'a, K> {
        BlockTreeCamera(kv_store, PhantomData)
    }

    pub fn snapshot(&self) -> BlockTreeSnapshot<'a, K::Snapshot> {
        todo!()
    }
}

/// A read view into the block tree that is guaranteed to stay unchanged.
pub struct BlockTreeSnapshot<'a, S: 'a + KVGet>(S, PhantomData<&'a ()>);

impl<'a, S: 'a + KVGet> BlockTreeSnapshot<'a, S> {
    fn state(&self) -> Value {
        todo!()
    }

    /* ↓↓↓ Used for syncing ↓↓↓ */

    pub fn blocks_from_tail(&self, tail: &CryptoHash, limit: u32) -> Option<Vec<Block>> {
        let mut res = Vec::with_capacity(limit as usize);

        // 1. Get tail block.
        let tail_block = self.block(tail)?;
        let mut cursor = tail_block.hash;
        res.push(tail_block);

        // 2. Walk through tail block's descendants until limit is satisfied or we hit uncommitted blocks.
        while res.len() < limit {
            let child = match self.child(&cursor) {
                Ok(block) => block,
                Err(ChildrenNotYetCommittedError) => break,
            };
            cursor = child.hash;
            res.push(child);
        }

        // 3. If limit is not yet satisfied, get speculative blocks.
        todo!();
        // if res.len() < limit {
        //     if let Some(block) = self.get_top_block() {
        //         // We reverse (.rev) uncommitted blocks so that blocks with lower heights appear first.
        //         let uncommitted_blocks: Vec<Block> = self.get_chain_between_speculative_block_and_highest_committed_block(&block.hash).into_iter().rev().collect();
        //         res.extend_from_slice(&uncommitted_blocks[..min(limit - res.len(), uncommitted_blocks.len())]);
        //     }
        // }

        Some(res)
    }
}

pub trait KVStore<'a>: KVGet + Clone + Send + 'static {
    type WriteBatch: WriteBatch;
    type Snapshot: 'a + KVGet;

    fn write(&mut self, wb: Self::WriteBatch);
    fn clear(&mut self);
    fn snapshot(&'a self) -> Self::Snapshot;
}

pub trait WriteBatch {
    fn new() -> Self;
    fn set(&mut self, key: &[u8], value: &[u8]);
    fn delete(&mut self, key: &[u8]);
}

pub trait KVGet {
    fn get(&mut self, key: &[u8]) -> Option<Vec<u8>>;
}

mod paths {
    // State variables
    pub(super) const BLOCKS: [u8; 1] = [0];
    pub(super) const BLOCK_HEIGHT_TO_HASH: [u8; 1] = [1]; 
    pub(super) const BLOCK_HASH_TO_CHILDREN: [u8; 1] = [2];
    pub(super) const COMMITTED_APP_STATE: [u8; 1] = [3];
    pub(super) const COMMITTED_VALIDATOR_SET: [u8; 1] = [4];
    pub(super) const PENDING_APP_STATE_UPDATES: [u8; 1] = [5]; 
    pub(super) const PENDING_VALIDATOR_SET_UPDATES: [u8; 1] = [6];
    pub(super) const LOCKED_VIEW: [u8; 1] = [7];
    pub(super) const HIGHEST_VIEW_ENTERED: [u8; 1] = [8];
    pub(super) const HIGHEST_QC: [u8; 1] = [9];

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