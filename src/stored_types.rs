use std::collections::{hash_set, HashSet};
use crate::msg_types::{NodeHash, SerDe};

pub struct ChildrenList(HashSet<NodeHash>);

impl ChildrenList {
    pub fn new() -> ChildrenList {
        todo!()
    }

    pub fn iter(&self) -> hash_set::Iter<NodeHash> {
        self.0.iter()
    }
}

impl SerDe for ChildrenList {
    fn deserialize(bs: &[u8]) -> Result<Self, crate::msg_types::DeserializationError> {
        todo!() 
    }

    fn serialize(&self) -> Vec<u8> {
        todo!() 
    }
}

pub struct WriteSet;

impl WriteSet {
    pub fn new() -> WriteSet {
        todo!()
    }
}

impl SerDe for WriteSet {
    fn deserialize(bs: &[u8]) -> Result<Self, crate::msg_types::DeserializationError> {
        todo!() 
    }

    fn serialize(&self) -> Vec<u8> {
        todo!()
    }
}

pub type Key = Vec<u8>;

pub type Value = Vec<u8>;