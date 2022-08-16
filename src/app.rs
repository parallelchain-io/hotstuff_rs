use crate::node_tree::{self, NodeTree};
use crate::node_tree::{Key, Value, WriteSet};
use crate::msg_types::{self, SerDe};

pub trait App {
    fn create_leaf(
        &mut self,
        parent_node: Node,
        state: State,
    ) -> (msg_types::Command, State);

    fn validate(
        &mut self, 
        node: Node, 
        state: State
    ) -> bool;
}

/// A wrapper around msg_types::Node that implements convenience methods for accessing the persisted copy of the Node, its derived
/// attributes (`write_set` and `State`), and the branch it heads in the NodeTree.
pub struct Node {
    node: msg_types::Node,
    node_tree: NodeTree,
}

impl Node {
    pub fn get_command(&self) -> msg_types::Command { todo!() }
    pub fn get_qc(&self) -> msg_types::QuorumCertificate { todo!() }
    pub fn get_parent(&self) -> Node { todo!() }
    pub fn get_write_set(&self) -> Result<node_tree::WriteSet, AlreadyCommittedError> { todo!() } 
    pub fn get_speculative_state(&self) -> Result<State, AlreadyCommittedError> { todo!() }
} 

/// To minimize storage use, `NodeTree::insert_node` deletes the `WriteSet`s of Nodes after they become committed. This error is
/// returned if `get_write_set` a Node that is already committed, or if `get_speculative_state` is called 
pub struct AlreadyCommittedError;

pub struct State {
    writes: WriteSet, 
    parent_writes: WriteSet,
    grandparent_writes: WriteSet,
}

impl State {
    pub(crate) fn new(parent_writes: WriteSet, grandparent_writes: WriteSet) -> State {
        State {
            writes: WriteSet::new(),
            parent_writes,
            grandparent_writes,
        }
    }

    pub fn set(&mut self, key: Key, value: Value) {
        self.writes.insert(key, Some(value));
    }

    pub fn get(&self, key: &Key) -> Option<Value> {
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

    pub fn delete(&mut self, key: Key) {
        self.writes.insert(key, None);
    } 
}
