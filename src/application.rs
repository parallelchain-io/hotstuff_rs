use crate::node_tree::{self, NodeTree};
use crate::node_tree::{Key, Value, WriteSet};
use crate::msg_types;

pub trait Application {
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
    pub fn get_parent(&self) -> Node { todo!() }
    pub fn get_write_set(&self) -> Result<node_tree::WriteSet, AlreadyCommittedError> { todo!() } 
    pub fn get_speculative_state(&self) -> Result<State, AlreadyCommittedError> { todo!() }
} 

pub struct AlreadyCommittedError;

pub struct State;

impl State {
    pub(crate) fn new(parent_writes: WriteSet, grandparent_writes: WriteSet) -> State {
        todo!()
    }

    pub fn set(&mut self, key: Key, value: Value) {
        todo!()
    }

    pub fn get(&self, key: Key) -> Value {
        todo!()
    }
}
