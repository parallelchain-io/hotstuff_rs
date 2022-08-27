use std::collections::HashMap;
use crate::msg_types::Node;
use crate::NodeTree;
use crate::node_tree::Database;
use crate::node_tree::stored_types::{Key, Value, WriteSet};

/// Read the itemdoc for the enclosing `state` module.
pub struct WorldState {
    writes: WriteSet, 
    parent_writes: WriteSet,
    grandparent_writes: WriteSet,
    db: Database,
}

impl WorldState { 
    pub(crate) fn open(node_tree: &NodeTree, parent_node: &Node) -> WorldState {
        let db = node_tree.get_db();
        let parent_writes = db.get_write_set(&parent_node.hash()).unwrap(); 
        let grandparent_writes = db.get_write_set(&parent_node.justify.node_hash).unwrap();

        WorldState {
            writes: HashMap::new(),
            parent_writes,
            grandparent_writes,
            db
        }
    }

    /// To delete a Key-Value pair, set the key to Value::new().
    pub fn set(&mut self, key: Key, value: Value) {
        self.writes.insert(key, value);
    }

    /// To simplify some internal implementation details, get does not make a distinction between None (has never been set, or
    /// has been previously deleted) Values, and Empty (0-length Vector of bytes) Values.  
    pub fn get(&self, key: &Key) -> Value {
        if let Some(value) = self.writes.get(key) {
            value.clone()
        } else if let Some(value) = self.parent_writes.get(key) {
            value.clone()
        } else if let Some(value) = self.grandparent_writes.get(key) {
            value.clone()
        } else {
            self.db.get_from_state(key).unwrap()
        }
    }

    pub(crate) fn get_writes(self) -> WriteSet {
        self.writes
    }
}
