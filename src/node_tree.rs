use std::sync::Arc;
use std::convert::identity;
use rocksdb::{DB, WriteBatch, Snapshot};
use crate::config::NodeTreeConfig;
use crate::stored_types::{WriteSet, ChildrenList};
use crate::msg_types::{Node, NodeHash, ViewNumber, SerDe};

pub fn open(node_tree_config: &NodeTreeConfig) -> (NodeTreeWriter, NodeTreeSnapshotFactory) {
    let db = DB::open_default(node_tree_config.db_path)
        .expect("Configuration error: fail to open DB.");
    let db = Arc::new(db);
    
    let node_tree_writer = NodeTreeWriter {
        db: Arc::clone(&db),
    };

    let node_tree_snapshot_factory = NodeTreeSnapshotFactory {
        db: Arc::clone(&db),
    };

    (node_tree_writer, node_tree_snapshot_factory)
}

/// Used exclusively by the single thread of the Engine module.
pub struct NodeTreeWriter {
    db: Arc<DB>,
}

impl NodeTreeWriter {
    /// This function assumes that `node` satisfies the SafeNode predicate, and that it has a parent, grandparent,
    /// great-grandparent, and great-great-grandparent are the NodeTree. In open, the Genesis Node is 'padded' with
    /// 3 empty descendant Nodes to force this invariant.
    pub fn insert_node(&self, node: &Node, write_set: &WriteSet) {
        let wb = WriteBatch::default();

        let parent_node = self.get_node(&node.justify.node_hash).unwrap();
        let grandparent_node = self.get_node(&parent_node.justify.node_hash).unwrap();
        let great_grandparent_node = self.get_node(&grandparent_node.justify.node_hash).unwrap(); 
        let great_great_grandparent_node = self.get_node(&great_grandparent_node.justify.node_hash).unwrap();

        // 1. Insert node to parent's ChildrenList.
        let parent_children = self.get_children_list(&parent_node.hash).map_or(ChildrenList::new(), identity);
        Self::set_children_list(&mut wb, &parent_node.hash, &parent_children);

        // 2. Insert node to the NODES keyspace. 
        Self::set_node(&mut wb, &node);

        // 3. Insert node's write_set to the WriteSet keyspace.
        Self::set_write_set(&mut wb, &node.hash, write_set);

        // 4. Check if great_grandparent.height is greater than HIGHEST_COMMITTED_NODE.height. If so, apply great_grandparent's writes to World State.
        // TODO: argue that no more than one Node can become committed because of a single insertion.
        let highest_committed_node = self.get_highest_committed_node();
        if great_grandparent_node.height > highest_committed_node.height {
            let great_grandparent_writes = self.get_write_set(&great_grandparent_node.hash).map_or(WriteSet::new(), identity);
            Self::apply_write_set(&mut wb, &great_grandparent_writes);
            Self::delete_write_set(&mut wb, &great_grandparent_node.hash);

            // 4.1. Update HIGHEST_COMMITTED_NODE.
            Self::set_highest_committed_node(&mut wb, &great_grandparent_node.hash);
        } 

        // 5. Abandon siblings of great_grandparent_node.
        let great_great_grandparent_children = self.get_children_list(&great_great_grandparent_node.hash).unwrap(); 
        let siblings = great_great_grandparent_children
            .iter()
            .filter(|child_hash| **child_hash != great_great_grandparent_node.hash);
        for sibling_hash in siblings {
            self.delete_branch(&mut wb, &sibling_hash)
        }
                
        // 6. Update Special Keys (HIGHEST_COMMITTED_NODE was updated in Step 4.1).

        // 6.1. LOCKED_VIEW. As `node` is assumed to satisfy the SafeNode predicate, the QC it carries is guaranteed to have a view number greater than or equal
        // to locked_view.
        Self::set_locked_view(&mut wb, parent_node.justify.view_number);

        // 6.2. TOP_NODE.
        Self::set_top_node(&mut wb, &node.hash);
    }
}

// Getters and setters for Special Keys.
impl NodeTreeWriter {
    fn set_locked_view(wb: &mut WriteBatch, view_num: ViewNumber) {
        wb.put(special_keys::LOCKED_VIEW, view_num.to_le_bytes());
    }

    pub fn get_locked_view(&self) -> usize {
        usize::from_le_bytes(self.db.get(special_keys::LOCKED_VIEW).unwrap().unwrap().try_into().unwrap())
    }

    fn set_top_node(wb: &mut WriteBatch, node_hash: &NodeHash) {
        wb.put(special_keys::HASH_OF_TOP_NODE, node_hash)
    }

    pub fn get_top_node(&self) -> Node {
        let top_node_hash = NodeHash::try_from(self.db.get(special_keys::HASH_OF_TOP_NODE).unwrap().unwrap()).unwrap();
        self.get_node(&top_node_hash).unwrap()
    }
    
    fn set_highest_committed_node(wb: &mut WriteBatch, node_hash: &NodeHash) {
        wb.put(special_keys::HASH_OF_HIGHEST_COMMITTED_NODE, node_hash)
    }

    pub fn get_highest_committed_node(&self) -> Node {
        let highest_committed_node_hash = NodeHash::try_from(self.db.get(special_keys::HASH_OF_HIGHEST_COMMITTED_NODE).unwrap().unwrap()).unwrap();
        self.get_node(&highest_committed_node_hash).unwrap()
    } 
}

// Getters and setters for Nodes and chains of Nodes.
impl NodeTreeWriter {
    fn set_node(wb: &mut WriteBatch, node: &Node) {
        wb.put(prefix(special_prefixes::NODES, &node.hash), node.serialize());
    }

    pub fn get_node(&self, node_hash: &NodeHash) -> Option<Node> {
        Some(Node::deserialize(&self.db.get(prefix(special_prefixes::NODES, node_hash)).unwrap()?).unwrap())
    }

    fn delete_branch(&self, wb: &mut WriteBatch, tail_node_hash: &NodeHash) {
        // 1. Delete children.
        if let Some(children) = self.get_children_list(tail_node_hash) {
            for child_hash in children.iter() {
                self.delete_branch(wb, &child_hash);
            }
        }

        // 2. Delete tail.
        Self::delete_node(wb, tail_node_hash);
        Self::delete_write_set(wb, tail_node_hash);
        Self::delete_children_list(wb, tail_node_hash);
    }

    fn delete_node(wb: &mut WriteBatch, node_hash: &NodeHash) {
        wb.delete(prefix(special_prefixes::NODES, node_hash))
    }
}

// Getters and Setters for WriteSets.
impl NodeTreeWriter {
    pub fn set_write_set(wb: &mut WriteBatch, node_hash: &NodeHash, write_set: &WriteSet) {
        wb.put(prefix(special_prefixes::WRITE_SETS, node_hash), write_set.serialize());
    }

    pub fn get_write_set(&self, node_hash: &NodeHash) -> Option<WriteSet> {
        Some(WriteSet::deserialize(&self.db.get(&prefix(special_prefixes::WRITE_SETS, node_hash)).unwrap()?).unwrap())
    }

    pub fn apply_write_set(wb: &mut WriteBatch, write_set: &WriteSet) {
        for (key, value) in write_set.iter() {
            wb.put(prefix(special_prefixes::WORLD_STATE, key), value)
        }
    }

    pub fn delete_write_set(wb: &mut WriteBatch, node_hash: &NodeHash) {
        wb.delete(prefix(special_prefixes::WRITE_SETS, node_hash))
    } 
}

// Getters and Setters for ChildrenLists.
impl NodeTreeWriter {
    pub fn set_children_list(wb: &mut WriteBatch, node_hash: &NodeHash, children_list: &ChildrenList) {
        wb.put(prefix(special_prefixes::CHILDREN_LISTS, node_hash), children_list.serialize())
    }

    pub fn get_children_list(&self, node_hash: &NodeHash) -> Option<ChildrenList> {
        Some(ChildrenList::deserialize(&self.db.get(&prefix(special_prefixes::CHILDREN_LISTS, node_hash)).unwrap()?).unwrap())
    }

    pub fn delete_children_list(wb: &mut WriteBatch, node_hash: &NodeHash) {
        wb.delete(prefix(special_prefixes::CHILDREN_LISTS, node_hash))
    }
}


/// Shared between the multiple threads of the Node Tree REST API.
pub struct NodeTreeSnapshotFactory {
    db: Arc<DB>,
}

impl NodeTreeSnapshotFactory {
    pub fn snapshot(&self) -> NodeTreeSnapshot {
        NodeTreeSnapshot {
            db_snapshot: self.db.snapshot()
        }
    }
}

/// Reads NodeTree as if it holds a read-lock on the Database.
pub struct NodeTreeSnapshot<'a> {
    db_snapshot: Snapshot<'a>,
}

impl<'a> NodeTreeSnapshot<'a> {
    pub fn get_node(&self, node_hash: &NodeHash) -> Option<Node> {
        Some(Node::deserialize(&self.db_snapshot.get(prefix(special_prefixes::NODES, node_hash)).unwrap()?).unwrap())
    }

    pub fn get_child(&self, parent_node_hash: &NodeHash) -> Result<Node, ChildrenNotYetCommittedError> {
        let highest_committed_node_hash = NodeHash::try_from(self.db_snapshot.get(special_keys::HASH_OF_HIGHEST_COMMITTED_NODE).unwrap().unwrap()).unwrap();
        let highest_committed_node = self.get_node(&highest_committed_node_hash).unwrap();
        let parent_node = self.get_node(&parent_node_hash).unwrap();
        if parent_node.height >= highest_committed_node.height {
            return Err(ChildrenNotYetCommittedError)
        }

        let parent_children = ChildrenList::deserialize(&self.db_snapshot.get(&prefix(special_prefixes::CHILDREN_LISTS, parent_node_hash)).unwrap().unwrap()).unwrap();
        // Safety: parent_children.len() must be 1, since parent is an ancestor of a committed Node. 
        let child_hash = parent_children.iter().next().unwrap();
        Ok(self.get_node(child_hash).unwrap())
    }

    pub fn get_top_node(&self) -> Node {
        let top_node_hash = NodeHash::try_from(self.db_snapshot.get(special_keys::HASH_OF_TOP_NODE).unwrap().unwrap()).unwrap();
        self.get_node(&top_node_hash).unwrap()
    }

    pub fn get_highest_committed_node(&self) -> Node {
        let highest_committed_node_hash = NodeHash::try_from(self.db_snapshot.get(special_keys::HASH_OF_HIGHEST_COMMITTED_NODE).unwrap().unwrap()).unwrap();
        self.get_node(&highest_committed_node_hash).unwrap()
    }
}

pub struct ChildrenNotYetCommittedError;

mod special_prefixes {
    pub(super) const NODES: [u8; 1] = [00];
    pub(super) const WRITE_SETS: [u8; 1] = [01];
    pub(super) const CHILDREN_LISTS: [u8; 1] = [02];
    pub(super) const WORLD_STATE: [u8; 1] = [03]; 
}

fn prefix(prefix: [u8; 1], additional_key: &[u8]) -> Vec<u8> {
    let prefixed_key = Vec::with_capacity(prefix.len() + additional_key.len());
    prefixed_key.extend_from_slice(&prefix);
    prefixed_key.extend_from_slice(additional_key);
    prefixed_key
}

mod special_keys {
    pub(super) const LOCKED_VIEW: [u8; 1] = [10];
    pub(super) const HASH_OF_TOP_NODE: [u8; 1] = [11];
    pub(super) const HASH_OF_HIGHEST_COMMITTED_NODE: [u8; 1] = [12];
}
