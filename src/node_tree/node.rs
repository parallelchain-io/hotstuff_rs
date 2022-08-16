use std::ops::Deref;
use crate::msg_types;
use crate::node_tree::{storage, State};

/// A wrapper around msg_types::Node that implements convenience methods for reading Nodes' direct fields, derived
/// attributes (`WriteSet` and `State`), and the branch it heads in the NodeTree.
pub struct Node {
    pub(in crate::node_tree) inner: msg_types::Node,
    pub(in crate::node_tree) db: storage::Database,
}

impl Node {
    /// Returns None if called on the genesis Node.
    pub fn get_parent(&self) -> Option<Node> {
        let parent = Node {
            inner: self.db.get_node(&self.inner.justify.node_hash).unwrap()?,
            db: self.db.clone(),
        };

        Some(parent)
    }

    /// # Undefined behavior
    /// This method must *not* be called on a Node that is the tail of a 2-Chain. `NodeTree::insert_node` deletes
    /// the WriteSets of committed block to save storage space. Calling this on a Node that is the tail of a 2-Chain
    /// causes a speculative State to be formed partly containing the writes of the Node's grandparent, which has
    /// been deleted. 
    pub(crate) fn get_speculative_state(&self) -> State {
        let parent_writes = self.db.get_write_set(&self.justify.node_hash).unwrap();
        let grandparent_writes = {
            let parent = self.db.get_node(&self.justify.node_hash).unwrap().unwrap();
            self.db.get_write_set(&parent.justify.node_hash).unwrap()
        };
        State::new(parent_writes, grandparent_writes)
    }
}

impl Deref for Node {
    type Target = msg_types::Node;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
