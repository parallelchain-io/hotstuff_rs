use std::convert::identity;
use std::ops::Deref;
use std::collections::HashSet;
use std::time::Instant;
use crate::node_tree::NodeTreeWriter;
use crate::stored_types::{WriteSet, Key};
use crate::msg_types::{Command, Node as MsgNode, NodeHash};

pub trait App: Send + 'static {
    fn create_leaf(
        &mut self, 
        parent_node: &Node,
        state: WorldStateHandle,
        deadline: Instant
    ) -> (Command, WorldStateHandle);

    fn execute(
        &mut self,
        node: &Node,
        state: WorldStateHandle,
        deadline: Instant
    ) -> Result<WorldStateHandle, ExecuteError>;
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

pub struct WorldStateHandle<'a> {
    writes: WriteSet,
    parent_writes: WriteSet,
    grandparent_writes: WriteSet,
    node_tree: &'a NodeTreeWriter,
}

impl<'a> WorldStateHandle<'a> {
    pub fn open(node_tree: &NodeTreeWriter, parent_node_hash: &NodeHash) -> WorldStateHandle {
        let parent_writes = node_tree.get_write_set(&parent_node_hash).map_or(WriteSet::new(), identity);
        let grandparent_writes = {
            let grandparent_node_hash = node_tree.get_node(parent_node_hash).unwrap().justify.node_hash;
            node_tree.get_write_set(&grandparent_node_hash).map_or(WriteSet::new(), identity)
        };

        WorldStateHandle {
            writes: WriteSet::new(),
            parent_writes,
            grandparent_writes,
            node_tree
        }
    }

    pub fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.writes.insert(key, value); 
    }

    pub fn get(&self, key: &[u8]) -> Vec<u8> {
        todo!()
    }
}

impl<'a> Into<WriteSet> for WorldStateHandle<'a> {
    fn into(self) -> WriteSet {
        self.writes
    }
}