use std::mem;
use std::ops::Deref;
use std::collections::{hash_set, HashSet, HashMap};
use crate::msg_types::{NodeHash, SerDe};

pub struct ChildrenList(HashSet<NodeHash>);

impl ChildrenList {
    pub fn new() -> ChildrenList {
        ChildrenList(HashSet::new())
    }

    pub fn iter(&self) -> hash_set::Iter<NodeHash> {
        self.0.iter()
    }
}

impl SerDe for ChildrenList {
    fn deserialize(bs: &[u8]) -> Result<Self, crate::msg_types::DeserializationError> {
        let mut res = ChildrenList::new();
        let mut cursor = 0usize;
        while cursor < bs.len() {
            let child_hash = bs[cursor..mem::size_of::<NodeHash>()].try_into().unwrap();
            cursor += mem::size_of::<NodeHash>();
            if res.0.insert(child_hash) {
                // Safety: entering this block implies that we registered the same Node twice as a child. 
                unreachable!()
            }
        }

        if cursor != bs.len() {
            // Safety: the condition implies that we previously wrote a wrongly-serialized Children into our Database.
            unreachable!()
        }

        Ok(res)
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for child_hash in &self.0 {
            buf.extend_from_slice(child_hash);
        }

        return buf
    }
}

pub struct WriteSet(HashMap<Key, Value>);
pub type Key = Vec<u8>;
pub type Value = Vec<u8>;

impl WriteSet {
    pub fn new() -> WriteSet {
        WriteSet(HashMap::new())
    }
}

impl SerDe for WriteSet {
    fn deserialize(bs: &[u8]) -> Result<Self, crate::msg_types::DeserializationError> {
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

        if cursor != bs.len() {
            // Safety: the condition implies that we previously wrote a wrongly-serialized WriteSet into our Database. 
            unreachable!()
        }

        Ok(res)
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::<u8>::new();
        for (key, value) in self.0 {
            buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
            buf.extend_from_slice(&key);
            buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
            buf.extend_from_slice(&value);
        }   
        buf
    }
}

impl Deref for WriteSet {
    type Target = HashMap<Key, Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
