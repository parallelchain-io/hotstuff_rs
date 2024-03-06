/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for 'inert' types, i.e., those that are sent around and inspected, but have no active behavior.
//! The types and traits defined in [crate::types] are either common across the sub-protocols or
//! required for the signature of the [block tree][crate::state::BlockTree]. Other types and traits, specific to 
//! the components of the hotstuff-rs protocol, can be found in the respetive directories.

use std::{
    collections::{hash_map, hash_set, HashMap, HashSet},
    hash::Hash,
};
use borsh::{BorshDeserialize, BorshSerialize};
pub use ed25519_dalek::{SigningKey, VerifyingKey, Signature};
pub use sha2::Sha256 as CryptoHasher;

pub type ChainID = u64;
pub type BlockHeight = u64;
pub type ChildrenList = Vec<CryptoHash>;
pub type CryptoHash = [u8; 32];
pub type Data = Vec<Datum>;
pub type DataLen = u32;
pub type Datum = Vec<u8>;
pub type Power = u64;
pub type TotalPower = u128;
pub type SignatureBytes = [u8; 64];
pub type SignatureSet = Vec<Option<SignatureBytes>>;
pub type ViewNumber = u64;
pub type EpochLength = u32;
pub type BufferSize = u64;

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct UpdateSet<K: Eq + Hash, V: Eq + Hash> {
    pub inserts: HashMap<K, V>,
    pub deletes: HashSet<K>,
}

impl<K: Eq + Hash, V: Eq + Hash> UpdateSet<K, V>
where
    K:,
{
    pub fn new() -> Self {
        Self {
            inserts: HashMap::new(),
            deletes: HashSet::new(),
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        self.deletes.remove(&key);
        self.inserts.insert(key, value);
    }

    pub fn delete(&mut self, key: K) {
        self.inserts.remove(&key);
        self.deletes.insert(key);
    }

    pub fn get_insert(&self, key: &K) -> Option<&V> {
        self.inserts.get(key)
    }

    pub fn contains_delete(&self, key: &K) -> bool {
        self.deletes.contains(key)
    }

    /// Get an iterator over all of the key-value pairs inserted by this [UpdateSet].
    pub fn inserts(&self) -> hash_map::Iter<K, V> {
        self.inserts.iter()
    }

    /// Get an iterator over all of the keys that are deleted by this [UpdateSet].
    pub fn deletions(&self) -> hash_set::Iter<K> {
        self.deletes.iter()
    }
}

pub type AppStateUpdates = UpdateSet<Vec<u8>, Vec<u8>>;