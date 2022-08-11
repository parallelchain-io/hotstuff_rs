use std::collections::HashSet;
use crate::node_tree::{Key, Value, WriteSet};
use crate::msg_types::{self, NodeHash};

pub(in crate::node_tree) struct Database;

impl Database {
    pub fn get_node(&self, hash: &NodeHash) -> msg_types::Node {
        todo!()
    }

    pub fn get_write_set(&self, of_node: &NodeHash) -> Option<WriteSet> {
        todo!()
    }

    pub fn get_children(&self, of_node: &NodeHash) -> Option<HashSet<NodeHash>> {
        todo!()
    }

    pub fn get_from_state(&self, key: Key) -> Value {
        todo!()
    }

    pub fn write(&self, write_batch: WriteBatch) {
        todo!()
    }
}


#[derive(Default)]
pub(in crate::node_tree) struct WriteBatch;

impl WriteBatch {
    pub fn set_node(&mut self, hash: &NodeHash, node: msg_types::Node) {
        todo!()
    }

    pub fn set_write_set(&mut self, of_node: &NodeHash) {
        todo!()
    }

    pub fn set_children(&mut self, of_node: &NodeHash) {
        todo!()
    }

    pub fn apply_writes_to_state(&mut self, writes: WriteBatch) {
        todo!()
    }
}