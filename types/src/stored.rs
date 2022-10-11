use std::collections::{hash_set, HashSet, HashMap, hash_map};
use borsh::{BorshSerialize, BorshDeserialize};
use crate::messages::BlockHash;

#[derive(BorshSerialize, BorshDeserialize)]
pub struct ChildrenList(HashSet<BlockHash>);

impl ChildrenList {
    pub fn new() -> ChildrenList {
        ChildrenList(HashSet::new())
    }

    pub fn insert(&mut self, child_hash: BlockHash) {
        self.0.insert(child_hash);
    }

    pub fn iter(&self) -> hash_set::Iter<BlockHash> {
        self.0.iter()
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct StorageMutations {
    inserts: HashMap<Key, Value>,
    deletes: HashSet<Key>, 
}
pub type Key = Vec<u8>;
pub type Value = Vec<u8>;

impl StorageMutations {
    pub fn new() -> StorageMutations {
        StorageMutations {
            inserts: HashMap::new(),
            deletes: HashSet::new(),
        }
    }

    pub fn insert(&mut self, key: Key, value: Value) {
        self.deletes.remove(&key);
        self.inserts.insert(key, value);
    }

    pub fn delete(&mut self, key: Key) {
        self.inserts.remove(&key);
        self.deletes.insert(key);
    }

    pub fn get_insert(&self, key: &Key) -> Option<&Value> {
        self.inserts.get(key)
    } 

    pub fn contains_delete(&self, key: &Key) -> bool {
        self.deletes.contains(key)
    }

    /// Get an iterator over all of the key, value pairs in this WriteSet.
    pub fn inserts(&self) -> hash_map::Iter<Key, Value> {
        self.inserts.iter()
    } 

    /// Get an iterator over all of the keys that are deleted by this WriteSet.
    pub fn deletes(&self) -> hash_set::Iter<Key> {
        self.deletes.iter()
    } 
}

pub type DataLen = u64;
