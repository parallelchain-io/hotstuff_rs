use crate::msg_types::{self, *};
use crate::node_tree::NodeTree;

pub type WriteSet = Vec<(Key, Value)>;

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
