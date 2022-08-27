use crate::config::NodeTreeConfig;
use crate::msg_types::{Node, NodeHash};
use crate::node_tree::storage::{Database, WriteBatch};
use crate::node_tree::stored_types::WriteSet;

/// NodeTree maintains a directed acyclic graph of 'Nodes', the object of consensus. From the point of view of
/// an Application, a NodeTree is a sequence of commands that may mutate State. In this view, a Node is a single
/// command in the sequence. 
/// 
/// NodeTree transparently detects the formation of 2-Chains and 3-Chains in NodeTree caused by `insert_node`
/// and automatically applies writes that can be safely written into State. It also automatically removes Nodes
/// that can no longer become committed, because one of its siblings became committed first. For brevity, we refer
/// to these Nodes as 'abandoned Nodes' in our documentation.
/// 
/// Internally, NodeTree is implemented as a wrapper around `Database`, which itself is a wrapper around an
/// Arc<rocksdb::DB>. Hence, NodeTree is cheaply Clone-able and shareable between threads.
#[derive(Clone, Debug)]
pub struct NodeTree {
    db: Database,
}

impl NodeTree {
    pub(crate) fn open(config: NodeTreeConfig) -> NodeTree { 
        let db = Database::open(&config.db_path).unwrap();

        NodeTree {
            db
        }
    } 

    /// Inserts a Node into the NodeTree as a child of node.justify.node_hash and atomically executes the 2 kinds of
    /// persistent storage operations that may need to occur as a result of the Node being inserted:
    /// 1. Applying the writes of (a) Node that may become committed as a result of this insertion, and
    /// 2. Deleting abandoned branches.
    /// 
    /// If node.justify.node_hash is not in the NodeTree, returns a ParentNotInDBError.
    pub(crate) fn try_insert_node(&mut self, node: &Node, writes: &WriteSet) -> Result<(), ParentNotFoundError> { 
        // 1. Open WriteBatch.
        let mut wb = WriteBatch::new();

        // 2. Check if `node.justify.node_hash` exists in Database.
        if self.db.get_node(&node.justify.node_hash).unwrap().is_none() {
            return Err(ParentNotFoundError)
        }

        // 3. 'Register' node as a child of parent.
        let mut parent_children = self.db.get_children(&node.justify.node_hash).unwrap();
        parent_children.insert(node.hash());
        wb.set_children(&node.justify.node_hash, Some(&parent_children));

        // 4. Apply the WriteSet of node's grandparent into State, since it's now the tail of a 3-Chain.
        let grandparent_hash = {
            let parent = self.db.get_node(&node.justify.node_hash).unwrap().unwrap();
            parent.justify.node_hash
        };
        let grandparent_writes = self.db.get_write_set(&grandparent_hash).unwrap();
        wb.apply_writes_to_state(&grandparent_writes);

        // 5. Delete grandparent's WriteSet.
        wb.set_write_set(&grandparent_hash, None);

        // 6. Abandon grandparent's sibling nodes.
        self.abandon_siblings(&grandparent_hash, &mut wb);

        // 7. Write node.
        wb.set_node(&node.hash(), Some(&node));

        // 8. Update node containing generic qc.
        wb.set_node_containing_generic_qc(&node);

        // 9. Write node's writes.
        wb.set_write_set(&node.hash(), Some(&writes));

        Ok(())
    }

    pub(crate) fn get_node(&self, hash: &NodeHash) -> Option<Node> {
        self.db.get_node(hash).unwrap()
    }

    pub(crate) fn get_node_with_generic_qc(&self) -> Node {
        self.db.get_node_with_generic_qc().unwrap()
    }

    pub(crate) fn get_node_with_locked_qc(&self) -> Node {
        let node_with_generic_qc = self.get_node_with_generic_qc();
        self.get_parent(&node_with_generic_qc.hash())
            .expect("Programming error: Node with Generic QC exists but its parent does not.")
    }

    pub(crate) fn get_parent(&self, of_node: &NodeHash) -> Result<Node, GetParentError> {
        let child = self.db.get_node(&of_node).unwrap().ok_or(GetParentError::OfNodeNotFound)?;
        self.db.get_node(&child.justify.node_hash).unwrap().ok_or(GetParentError::ParentNotFound)
    }

    pub(crate) fn get_child(&self, of_node: &NodeHash) -> Result<Node, ChildNotYetCommittedError> {
        todo!()
    }

    pub(crate) fn get_height(&self, node: &NodeHash) -> Option<usize> {
        todo!()
    }

    pub(crate) fn get_db(&self) -> Database {
        self.db.clone()
    }

    fn abandon_siblings(&self, of_node: &NodeHash, wb: &mut WriteBatch) {
        let parent_hash = self.get_node(of_node).unwrap().justify.node_hash;
        let siblings = self.db.get_children(&parent_hash).unwrap();
        for sibling in siblings {
            self.delete_branch(&sibling, wb);
        }
    }

    fn delete_branch(&self, tail_node: &NodeHash, wb: &mut WriteBatch) {
        // 1. Delete children.
        let children = self.db.get_children(tail_node).unwrap();
        for child in children {
            self.delete_branch(&child, wb);
        }

        // 2. Delete tail.
        wb.set_node(tail_node, None);
        wb.set_write_set(tail_node, None);
        wb.set_children(tail_node, None);
    }


}

#[derive(Debug)]
pub enum GetParentError {
    OfNodeNotFound,
    ParentNotFound,
}

#[derive(Debug)]
pub struct ParentNotFoundError;

#[derive(Debug)]
pub struct ChildNotYetCommittedError;
