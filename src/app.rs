use std::convert::identity;
use std::ops::Deref;
use std::time::Instant;
use crate::node_tree::NodeTreeWriter;
use crate::stored_types::{WriteSet, Key, Value};
use crate::msg_types::{Command, Node as MsgNode, NodeHash};

pub trait App: Send + 'static {
    /// Called by StateMachine when this Participant becomes the Leader and has to propose a new Node which extends a branch of the NodeTree.
    /// 
    /// # Parameters
    /// 1. `parent_node`: the Node which the new Node directly descends from.
    /// 2. `world_state`: a read-and-writable view of the World State after executing all Nodes in the branch headed by `parent_node`.
    /// 3. `deadline`: this function call should return at the latest by this instant in time. Otherwise, this view in which the Participant
    /// is the Leader is likely to fail because of view timeout.
    /// 
    /// # Return value
    /// A two-tuple consisting of:
    /// 1. A Command. This will occupy the `command` field of the Node proposed by this Participant in this view. 
    /// 2. The instance of WorldStateHandle which was passed into this function. `set`s into this Handle will be applied into the World State
    /// when the Node containing the returned Command becomes committed.
    fn create_leaf(
        &mut self, 
        parent_node: &Node,
        world_state: WorldStateHandle,
        deadline: Instant
    ) -> (Command, WorldStateHandle);

    /// Called by StateMachine when this Participant is a Replica and has to decide whether or not to vote on a Node which was proposed by
    /// the Leader.
    /// 
    /// # Parameters
    /// 1. `node`: the Node which was proposed by the Leader of this view.
    /// 2. `world_state`: read the corresponding entry in the itemdoc for `create_leaf`.
    /// 3. `deadline`: this function call should return at the latest by this instant in time. If not, this Participant might not be able to 
    /// Vote in time for its signature to be included in this round's QuorumCertificate. 
    /// 
    /// # Return value
    /// A two-tuple consisting of:
    /// 1. The instance of WorldStateHandle which was passed into this function. Read the corresponding entry in the itemdoc for `create_leaf`.
    /// 2. An ExecuteError. Read the itemdoc for `ExecuteError`.
    fn execute(
        &mut self,
        node: &Node,
        world_state: WorldStateHandle,
        deadline: Instant
    ) -> Result<WorldStateHandle, ExecuteError>;
}

/// Circumstances in which an App could reject a proposed Node, causing this Participant to skip this round without voting.
pub enum ExecuteError {
    /// deadline was exceeded while processing the proposed Node.
    RanOutOfTime,

    /// The contents of the Node, in the context of its proposed position in the Node Tree, is invalid in the view of App-level validation rules.
    InvalidNode,
}

/// A wrapper around msg_types::Node which, besides allowing App's methods to look into the Node's fields, exposes methods for traversing the 
/// branch that this Node heads.
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

    /// Gets the parent of this Node. This returns None if called on the Genesis Node.
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

/// A read-and-writable view of the World State after executing all the Nodes in some branch of the Node Tree. The writes (`set`s)
/// applied into this WorldStateHandle only become permanent when the Node containing the Command that it corresponds to becomes committed. 
/// 
/// This structure should NOT be used in App code outside the context of `create_leaf` and `execute`.
pub struct WorldStateHandle<'a> {
    writes: WriteSet,
    parent_writes: WriteSet,
    grandparent_writes: WriteSet,
    node_tree: &'a NodeTreeWriter,
}

impl<'a> WorldStateHandle<'a> {
    pub(crate) fn open(node_tree: &'a NodeTreeWriter, parent_node_hash: &NodeHash) -> WorldStateHandle<'a> {
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

    /// set a key-value pair in the World State.
    pub fn set(&mut self, key: Key, value: Value) {
        self.writes.insert(key, value); 
    }

    /// get a value in the World State.
    pub fn get(&self, key: &Key) -> Value {
        if let Some(value) = self.writes.get(key) {
            value.clone()
        } else if let Some(value) = self.parent_writes.get(key) {
            value.clone()
        } else if let Some(value) = self.grandparent_writes.get(key) {
            value.clone()
        } else {
            self.node_tree.get_from_world_state(key)
        }
    }

}

impl<'a> Into<WriteSet> for WorldStateHandle<'a> {
    fn into(self) -> WriteSet {
        self.writes
    }
}