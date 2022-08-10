use crate::basic_types::*;

pub struct StoredNode {
    pub node: ExecutedNode,

    // Used to delete 'abandoned' siblings when a Node gets committed.
    pub children: Vec<NodeHash>,
}

pub struct ExecutedNode {
    pub node: Node,
    pub write_set: WriteSet,
}

impl SerDe for ExecutedNode {
    fn serialize(&self) -> Vec<u8> {
        todo!()
        // Encoding
        // node.serialize() ++
        // write_set.serialize()
    }

    fn deserialize(bs: Vec<u8>) -> Result<ExecutedNode, DeserializationError> {
        todo!()
    } 
}

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