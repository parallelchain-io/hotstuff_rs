use crate::msg_types::{self, NodeHash, QuorumCertificate};
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
#[derive(Clone)]
pub struct NodeTree {
    db: Database,
}

impl NodeTree {
    pub(crate) fn open() -> NodeTree { 
        let db = Database::open().unwrap();

        NodeTree {
            db
        }
    } 

    pub(crate) fn insert_node(&mut self, node: msg_types::Node, writes: WriteSet) -> Result<(), InsertError> { 
        // 1. Open WriteBatch.
        let mut wb = WriteBatch::new();

        // 2. Check if `node.justify.node_hash` exists in Database.
        if self.db.get_node(&node.justify.node_hash).unwrap().is_none() {
            return Err(InsertError::ParentNotInDB)
        }

        // 3. 'Register' node as a child of parent.
        let mut parent_children = self.db.get_children(&node.justify.node_hash).unwrap();
        parent_children.insert(node.hash());
        wb.set_children(&node.justify.node_hash, &parent_children);

        // 4. Apply the writes of node's grandparent into State, since it's now the tail of a 3-Chain.
        let grandparent_hash = {
            let parent = self.db.get_node(&node.justify.node_hash).unwrap().unwrap();
            parent.justify.node_hash
        };
        let grandparent_writes = self.db.get_write_set(&grandparent_hash).unwrap();
        wb.apply_writes_to_state(&grandparent_writes);

        // 5. Abandon grandparent's sibling nodes.
        self.abandon_siblings(&grandparent_hash);

        // 6. Write node.
        wb.set_node(&node.hash(), &node);

        // 7. Write node's writes.
        wb.set_write_set(&node.hash(), &writes);

        Ok(())
    }

    pub(crate) fn get_node(&self, hash: &NodeHash) -> Option<msg_types::Node> {
        self.db.get_node(hash).unwrap()
    }

    pub(crate) fn get_generic_qc(&self) -> QuorumCertificate { 
        self.db.get_node_with_generic_qc().unwrap().justify
    } 

    pub(crate) fn get_locked_qc(&self) -> QuorumCertificate { 
        let generic_qc = self.get_generic_qc();
        let node_with_locked_qc = self.get_node(&generic_qc.node_hash).unwrap();

        node_with_locked_qc.justify
    }

    fn abandon_siblings(&self, of_node: &NodeHash) {
        todo!()
    }
}

pub(crate) enum InsertError {
    ParentNotInDB,
}
