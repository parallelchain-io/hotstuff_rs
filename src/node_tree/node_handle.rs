use std::ops::Deref;
use crate::msg_types::{self, NodeHeight};
use crate::node_tree::storage::Database;

/// A wrapper around msg_types::Node that implements convenience methods for reading Nodes' direct fields, derived
/// attributes (`WriteSet` and `State`), and the branch it heads in the NodeTree.
pub struct NodeHandle {
    node: msg_types::Node,
    db: Database,
}

impl NodeHandle {
    /// Returns None if called on the genesis Node.
    pub fn get_parent(&self) -> Option<NodeHandle> {
        let parent = NodeHandle {
            node: self.db.get_node(&self.node.justify.node_hash).unwrap()?,
            db: self.db.clone(),
        };

        Some(parent)
    }

    pub fn get_height(&self) -> Option<NodeHeight> {
        todo!()
    }

    pub(crate) fn get_child(&self) -> Result<NodeHandle, ChildNotYetCommittedError> {
        todo!()
    }
}

pub struct ChildNotYetCommittedError;

impl Deref for NodeHandle {
    type Target = msg_types::Node;

    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

impl Into<msg_types::Node> for NodeHandle {
    fn into(self) -> msg_types::Node {
        self.node
    }
}
