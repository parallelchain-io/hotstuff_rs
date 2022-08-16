use std::collections::{HashSet, HashMap};
use crate::msg_types::*;

pub type WriteSet = HashMap<Key, Option<Value>>;


impl SerDe for WriteSet {
    fn serialize(&self) -> Vec<u8> {
        todo!()
        // Encoding
        // for each (key, value):
        // key.length() ++
        // key ++
        // value.length() ++
        // value 
    }
    
    fn deserialize(bs: Vec<u8>) -> Result<WriteSet, DeserializationError> {
        todo!()
    }
}

pub type Key = Vec<u8>;

pub type Value = Vec<u8>;

impl SerDe for Value {
    fn serialize(&self) -> Vec<u8> {
        todo!()
    }

    fn deserialize(bs: Vec<u8>) -> Result<Self, DeserializationError> {
        todo!()
    }
}

pub type Children = HashSet<NodeHash>;

impl SerDe for Children {
    fn serialize(&self) -> Vec<u8> {
        todo!()
    }

    fn deserialize(bs: Vec<u8>) -> Result<Self, DeserializationError> {
        todo!()    
    }
}
