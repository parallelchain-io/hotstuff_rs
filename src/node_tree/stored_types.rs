use std::mem;
use std::collections::{HashSet, HashMap};
use crate::msg_types::*;

pub type WriteSet = HashMap<Key, Value>;

impl SerDe for WriteSet {
    // # Encoding
    // for each (key, value):
    // key.len().to_le_bytes() ++
    // key ++
    // value.length().to_le_bytes() ++
    // value
    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::<u8>::new();
        for (key, value) in self {
            buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
            buf.extend_from_slice(&key);
            buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
            buf.extend_from_slice(&value);
        }   
        buf
    }
    
    fn deserialize(bs: Vec<u8>) -> Result<WriteSet, DeserializationError> {
        let mut res = WriteSet::new();
        let mut cursor = 0usize;
        while cursor < bs.len() {
            let key_len = u32::from_le_bytes(bs[cursor..mem::size_of::<u32>()].try_into().unwrap()); 
            cursor += mem::size_of::<u32>();

            let key = bs[cursor..key_len as usize].to_vec();
            cursor += key_len as usize;

            let value_len = u32::from_le_bytes(bs[cursor..mem::size_of::<u32>()].try_into().unwrap());
            cursor += mem::size_of::<u32>();

            let value = bs[cursor..value_len as usize].to_vec();
            cursor += value_len as usize;

            res.insert(key, value);
        }

        if cursor == bs.len() {
            Ok(res)
        } else {
            Err(DeserializationError)
        }
    }
}

pub type Key = Vec<u8>;

pub type Value = Vec<u8>;

pub type Children = HashSet<NodeHash>;

impl SerDe for Children {
    fn serialize(&self) -> Vec<u8> {
        todo!()
    }

    fn deserialize(bs: Vec<u8>) -> Result<Self, DeserializationError> {
        todo!()    
    }
}
