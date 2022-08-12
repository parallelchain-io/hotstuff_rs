use crate::msg_types::{self, NodeHash, QuorumCertificate, SerDe};
use crate::node_tree::{Database, WriteSet};

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

    pub(crate) fn insert_node(&mut self, node: msg_types::Node, writes: WriteSet) { 
        // 1. Open WriteBatch.
        let mut wb = rocksdb::WriteBatch::default();

        // 2. Read and deserialize parent at `node.justify.node_hash` from DB.
        let parent_hash = node.justify.node_hash;
        let mut parent = self.get_node(&parent_hash).unwrap();

        // 3. (Using WriteBatch) 'register' node as a child of parent.



        // 4. (using WriteBatch) Serialize and insert updated node.parent.
        wb.put(parent_hash, parent.serialize());
    }

    pub(crate) fn get_node(&self, hash: &NodeHash) -> Option<msg_types::Node> {
        self.db.get_node(hash).unwrap()
    }

    pub(crate) fn get_generic_qc(&self) -> QuorumCertificate { 
        self.db.get_node_with_generic_qc().unwrap().unwrap().justify
    } 

    pub(crate) fn get_locked_qc(&self) -> QuorumCertificate { 
        let generic_qc = self.get_generic_qc();
        let node_with_locked_qc = self.get_node(&generic_qc.node_hash).unwrap();

        node_with_locked_qc.justify
    }
}
