use rocksdb::WriteBatch;
use crate::stored_types::{WriteSet, ChildrenList, Key, Value};
use crate::msg_types::{Node, NodeHash};

fn open(db_path: &str) -> (NodeTreeWriter, NodeTreeSnapshotFactory) {
    todo!()
}

/// Used exclusively by the single thread of the Engine module.
struct NodeTreeWriter;

impl NodeTreeWriter {
    /// # Assumptions
    /// 1. node satisfies the SafeNode predicate.
    /// 2. node.justify.node_hash is in the NodeTree.
    ///
    /// # Panics
    /// If node.justify.node_hash is not in the NodeTree.
    fn insert_node(&self, node: &Node, write_set: &WriteSet) {
        // Parent Node (`parent`) = the node identified by node.justify.node_hash.
        // Grandparent Node (`grandparent`) = the node identified by parent.justify.node_hash.
        // Great-grandparent Node (`great_grandparent`) = the node identified by grandparent.justify.node_hash.

        // 1. Insert node to parent's ChildrenList.

        // 2. Insert node to the NODES keyspace. 

        // 3. Insert node's write_set to the WriteSet keyspace.

        // 4. Check if great_grandparent.height is greater than HIGHEST_COMMITTED_NODE.height. If so, apply
        // HIGHEST_COMMITTED_NODE+1..great_grandparent's writes to World State.

        // 5. Abandon all children of the Nodes in HIGHEST_COMMITTED_NODE+1..great_grandparent's which are not
        // part of the chain ending in great_grandparent. 

        // 6. Update Special Keys. 
        //     a. PREFERRED_VIEW = max(PREFERRED_VIEW, parent.justify.view_number);
        //     b. TOP_NODE = node;
        //     c. If great_grandparent.height > HIGHEST_COMMITTED_NODE.height, HIGHEST_COMMITTED_NODE = 
        //        great_grandparent.
        todo!()
    }

    fn get_node(&self, node_hash: &NodeHash) -> Option<Node> {
        todo!()
    }

    fn get_write_set(&self, node_hash: &NodeHash) -> Option<WriteSet> {
        todo!()
    }

    fn delete_write_set(wb: &mut WriteBatch, node_hash: &NodeHash) -> bool {
        todo!()
    }

    fn get_children_list(&self, node_hash: &NodeHash) -> Option<ChildrenList> {
        todo!()
    }

    fn set_children_list(wb: &mut WriteBatch, node_hash: &NodeHash, children_list: &ChildrenList) {
        todo!()
    }

    fn delete_children_list(wb: &mut WriteBatch, node_hash: &NodeHash) -> bool {
        todo!()
    }
}


/// Shared between the multiple threads of the Node Tree REST API.
pub struct NodeTreeSnapshotFactory;

/// Reads NodeTree as if it holds a read-lock on the Database.
pub struct NodeTreeSnapshot;

impl NodeTreeSnapshot {
    pub fn get_node(&self, node: &NodeHash) -> Option<Node> {
        todo!()
    }

    pub fn get_child(&self, ) -> Result<Node, IsHighestCommittedNodeError> {
        todo!()
    }
}

pub struct IsHighestCommittedNodeError;

mod special_prefixes {
    pub const NODES: [u8; 1] = [00];
    pub const WRITE_SETS: [u8; 1] = [01];
    pub const CHILDREN_LISTS: [u8; 1] = [02];
    pub const WORLD_STATE: [u8; 1] = [03];
}

mod special_keys {
    pub const PREFERRED_VIEW: [u8; 1] = [10];
    pub const HASH_OF_TOP_NODE: [u8; 1] = [11];
    pub const HASH_OF_HIGHEST_COMMITTED_NODE: [u8; 1] = [12];
}

pub struct WorldState;

impl WorldState {
    pub fn open(node_tree: &NodeTreeWriter, parent_node_hash: &NodeHash) -> WorldState {
        todo!()
    }

    pub fn set(&mut self, key: Key, value: Value) {
        todo!()
    }

    pub fn delete(&mut self, key: &Key) {
        todo!()
    }

    pub fn get(&self, key: &Key) -> Value {
        todo!()
    }
}

impl Into<WriteSet> for WorldState {
    fn into(self) -> WriteSet {
        todo!()
    }
}
