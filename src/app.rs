use std::ops::Deref;
use std::time::Instant;
use crate::node_tree::{WorldState, NodeTreeWriter};
use crate::msg_types::{Command, Node as MsgNode};

pub trait App: Send + 'static {
    fn create_leaf(
        &mut self, 
        parent_node: &Node,
        state: WorldState,
        deadline: Instant
    ) -> (Command, WorldState);

    fn execute(
        &mut self,
        node: &Node,
        state: WorldState,
        deadline: Instant
    ) -> Result<WorldState, ExecuteError>;
}

pub enum ExecuteError {
    RanOutOfTime,
    InvalidNode,
}

pub struct Node<'a> {
    inner: MsgNode,
    node_tree: &'a NodeTreeWriter,
}

impl<'a> Node<'a> {
    pub fn new(node: MsgNode, node_tree: &NodeTreeWriter) -> Node {
        Node {
            inner: node,
            node_tree,
        }
    }

    /// Returns None if called on the Genesis Node.
    pub fn get_parent(&self) -> Option<Node> {
        if self.justify.view_number == 0 {
            return None
        }

        let parent = self.node_tree.get_node(&self.inner.justify.node_hash)
            .expect("Programming error: Non-Genesis node does not have a parent.");

        let parent = Node {
            inner: parent,
            node_tree: self.node_tree.clone(),
        };

        Some(parent)
    }
}

impl<'a> Deref for Node<'a> {
    type Target = MsgNode;

    fn deref(&self) -> &Self::Target {
        &self.inner 
    }
}