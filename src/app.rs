use std::ops::Deref;
use std::time::Instant;
use crate::node_tree::{WorldState, NodeTree, GetParentError};
use crate::msg_types::{self, Command};

pub trait App: Send + 'static {
    fn create_leaf(
        &mut self, 
        parent_node: &Node,
        state: WorldState,
        deadline: Instant
    ) -> (Command, WorldState);

    fn try_execute(
        &mut self,
        node: &Node,
        state: WorldState,
        deadline: Instant
    ) -> Result<WorldState, TryExecuteError>;
}

pub enum TryExecuteError {
    RanOutOfTime,
    InvalidNode,
}

pub struct Node {
    pub(crate) inner: msg_types::Node,
    pub(crate) node_tree: NodeTree,
}

impl<'a, 'b> Node {
    /// Returns None if called on the Genesis Node.
    pub fn get_parent(&self) -> Option<Node> {
        let parent = match self.node_tree.get_parent(&self.hash()) {
            Ok(node) => node,
            Err(GetParentError::IsGenesisNode) => return None,
            Err(GetParentError::OfNodeNotFound) => unreachable!(),
        };
        let parent = Node {
            inner: parent,
            node_tree: self.node_tree.clone(),
        };

        Some(parent)
    }
}

impl Deref for Node {
    type Target = msg_types::Node;

    fn deref(&self) -> &Self::Target {
        &self.inner 
    }
}