use crate::node_tree::storage;
use crate::node_tree::stored_types::{Key, Value, WriteSet};

/// Read the itemdoc for the enclosing `state` module.
pub struct State {
    pub(in crate::node_tree) writes: WriteSet, 
    pub(in crate::node_tree) parent_writes: WriteSet,
    pub(in crate::node_tree) grandparent_writes: WriteSet,
    pub(in crate::node_tree) db: storage::Database,
}

impl State {
    pub(in crate::node_tree) fn new(parent_writes: WriteSet, grandparent_writes: WriteSet, db: storage::Database) -> State {
        State {
            writes: WriteSet::new(),
            parent_writes,
            grandparent_writes,
            db
        }
    }

    /// To delete a Key-Value pair, set the key to Value::new().
    pub(in crate::node_tree) fn set(&mut self, key: Key, value: Value) {
        self.writes.insert(key, value);
    }

    /// To simplify some internal implementation details, get does not make a distinction between None (has never been set, or
    /// has been previously deleted) Values, and Empty (0-length Vector of bytes) Values.  
    pub(in crate::node_tree) fn get(&self, key: &Key) -> Value {
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