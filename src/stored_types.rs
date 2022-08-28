use crate::msg_types::SerDe;

pub struct ChildrenList;

impl SerDe for ChildrenList {
    fn deserialize(bs: &[u8]) -> Result<Self, crate::msg_types::DeserializationError> {
        todo!() 
    }

    fn serialize(&self) -> Vec<u8> {
        todo!() 
    }
}

pub struct WriteSet;

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