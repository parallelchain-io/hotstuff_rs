//! This module defines types and methods used to access and mutate the persistent state that a HotStuff-rs
//! validator keeps track for the operation of the protocol, and for its application.
//! 
//! This state may be stored in a key-value store of the library user's own choosing, as long as that KV store
//! can provide a type that implements [KVWrite], [KVGet], and [KVCamera]. This state can be mutated through
//! an instance of [BlockTree], and read through an instance of [BlockTreeSnapshot].
//! 
//! HotStuff-rs structures its state into separate conceptual 'variables' which are stored in tuples that sit
//! at a particular key path or prefix in the library user's chosen KV store. These variables are:
//! - **Blocks** ([CryptoHash] -> [Block]).
//! - **Block Number to Block** ([BlockNumber] -> [CryptoHash]): a mapping between a block's number and a block's hash. This mapping only contains blocks that are committed, because if a block hasn't been committed, there may be multiple blocks at the same height.
//! - **Block Hash to Children** ([CryptoHash] -> [ChildrenList]): a mapping between a block's hash and the children it has in the block tree. A block may have multiple chilren if they have not been committed.
//! - **Committed App State** ([Key] -> [Value]).
//! - **Committed Validator Set** ([ValidatorSet]).
//! - **Pending App State Changes** (`CryptoHash -> AppStateChanges`).
//! - **Pending Validator Set Changes** (`CryptoHash -> ValidatorSetChanges`).
//! - **Locked View** (`ViewNumber`): the highest view number of a quorum certificate contained in a block that has a child.
//! - **Highest Entered View** (`ViewNumber`): the highest view that this validator has entered.
//! - **Highest Quorum Certificate** (`QuorumCertificate`): among the quorum certificates this validator has seen and verified the signatures of, the one with the highest view number.
//! 
//! The location of each of these variables in a KV store is defined in [kv_paths]. Note that the fields of a
//! block are itself stored in different tuples. This is so that user code can get a subset of a block's data
//! without loading the entire block from storage, which may be expensive. The key suffixes on which each of
//! block's fields are stored are also defined in [kv_paths].

use crate::types::*;
use std::marker::PhantomData;

/// A read and write handle into the block tree, exclusively owned by the algorithm thread.
pub(crate) struct BlockTree<'a, K: KVStore<'a>>(K, PhantomData<&'a ()>);

impl<'a, K: KVStore<'a> + KVGet> BlockTree<'a, K> {
    pub(crate) fn insert_block(&mut self, block: Block, app_state_updates: Option<AppStateUpdates>, validator_set_updates: Option<ValidatorSetUpdates>) {
        todo!()
    }
}

pub struct BlockTreeCamera<'a, K: KVStore<'a>>(K, PhantomData<&'a ()>);

/// A read view into the block tree that is guaranteed to stay unchanged.
pub struct BlockTreeSnapshot<'a, S: 'a + KVGet>(S, PhantomData<&'a ()>);

impl<'a, S: 'a + KVGet> BlockTreeSnapshot<'a, S> {
    fn state(&self) -> Value {
        todo!()
    }
}

pub trait KVStore<'a>: KVGet + 'static {
    type WriteBatch: WriteBatch;
    type Snapshot: 'a + KVGet;

    fn write(&mut self, wb: Self::WriteBatch);
    fn clear(&mut self);
    fn snapshot(&'a self) -> Self::Snapshot;
}

pub trait WriteBatch {
    fn new() -> Self;
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>);
}

pub trait KVGet {
    fn get(&mut self, key: &[u8]) -> Option<Vec<u8>>;
}


mod kv_paths {
    // State variables
    pub(super) const BLOCKS: [u8; 1] = [0];
    pub(super) const BLOCK_NUM_TO_HASH: [u8; 1] = [1]; 
    pub(super) const BLOCK_HASH_TO_CHILDREN: [u8; 1] = [2];
    pub(super) const COMMITTED_APP_STATE: [u8; 1] = [3];
    pub(super) const COMMITTED_VALIDATOR_SET: [u8; 1] = [4];
    pub(super) const PENDING_APP_STATE_CHANGES: [u8; 1] = [5];
    pub(super) const PENDING_VALIDATOR_SET_CHANGES: [u8; 1] = [6];
    pub(super) const LOCKED_VIEW: [u8; 1] = [7];
    pub(super) const HIGHEST_ENTERED_VIEW: [u8; 1] = [8];
    pub(super) const HIGHEST_QC: [u8; 1] = [9];

    // Fields of Block
    pub(super) const NUM: [u8; 1] = [0];
    pub(super) const JUSTIFY: [u8; 1] = [02];
    pub(super) const DATA_HASH: [u8; 1] = [03];
    pub(super) const DATA: [u8; 1] = [04];
}

/// Takes references to two byteslices and returns a vector containing the bytes of the first one, and then the bytes of the 
/// second one.
fn combine(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut res = Vec::with_capacity(a.len() + b.len());
    res.extend_from_slice(a);
    res.extend_from_slice(b);
    res
}