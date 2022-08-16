use crate::node_tree::stored_types;

pub struct State {
    writes: stored_types::WriteSet, 
    parent_writes: stored_types::WriteSet,
    grandparent_writes: stored_types::WriteSet,
}

impl State {
    pub(crate) fn new(parent_writes: stored_types::WriteSet, grandparent_writes: stored_types::WriteSet) -> State {
        State {
            writes: stored_types::WriteSet::new(),
            parent_writes,
            grandparent_writes,
        }
    }

    pub fn set(&mut self, key: stored_types::Key, value: stored_types::Value) {
        self.writes.insert(key, Some(value));
    }

    pub fn get(&self, key: &stored_types::Key) -> Option<stored_types::Value> {
        if let Some(value) = self.writes.get(key) {
            value.clone()
        } else if let Some(value) = self.parent_writes.get(key) {
            value.clone()
        } else if let Some(value) = self.grandparent_writes.get(key) {
            value.clone()
        } else {
            None
        }
    }

    pub fn delete(&mut self, key: stored_types::Key) {
        self.writes.insert(key, None);
    } 
}