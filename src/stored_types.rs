use borsh::{BorshSerialize, BorshDeserialize};
use std::ops::{Deref, DerefMut};
use std::collections::{hash_set, HashSet, HashMap};
use crate::msg_types::BlockHash;

#[derive(BorshSerialize, BorshDeserialize)]
pub struct ChildrenList(HashSet<BlockHash>);

impl ChildrenList {
    pub fn new() -> ChildrenList {
        ChildrenList(HashSet::new())
    }

    pub fn iter(&self) -> hash_set::Iter<BlockHash> {
        self.0.iter()
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct WriteSet(HashMap<Key, Value>);
pub type Key = Vec<u8>;
pub type Value = Vec<u8>;

impl WriteSet {
    pub fn new() -> WriteSet {
        WriteSet(HashMap::new())
    }
}

impl Deref for WriteSet {
    type Target = HashMap<Key, Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for WriteSet {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub(crate) type DataLen = u64;
