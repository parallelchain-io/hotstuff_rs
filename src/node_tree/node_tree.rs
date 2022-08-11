use std::sync::Arc;
use crate::msg_types::{self, NodeHash, QuorumCertificate, SerDe};
use crate::node_tree::WriteSet;

/// NodeTree maintains a directed acyclic graph of 'Nodes', the object of consensus. From the point of view of
/// an Application, a NodeTree is a sequence of commands that may mutate State. In this view, a Node is a single
/// command in the sequence. 
/// 
/// An 'N-Chain' is a sequence of Nodes, linked together via QuorumCertificates. A Node becomes 'locked' when it
/// becomes the tail of a 2-Chain, and becomes 'committed' when it becomes the tail of a 3-Chain. When a Node becomes
/// committed, its writes can be safely written into persistent State.
/// 
/// NodeTree transparently detects the formation of 2-Chains and 3-Chains in NodeTree caused by `insert_node`
/// and automatically applies writes that can be safely written into State. It also automatically removes Nodes
/// that can no longer become committed, because one of its siblings became committed first. For brevity, we refer
/// to these Nodes as 'abandoned Nodes' in our documentation.
/// 
/// Internally, NodeTree is a wrapper around an `Arc<rocksdb::DB>`, and therefore is costless to clone and share
/// between threads.
#[derive(Clone)]
pub struct NodeTree {
    db: Arc<rocksdb::DB>,
}

impl NodeTree {
    pub(crate) fn open() -> Result<NodeTree, rocksdb::Error> { 
        const DB_PATH: &str = "./database"; 
        let db = Arc::new(rocksdb::DB::open_default(DB_PATH)?);

        Ok(NodeTree {
            db
        })
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
        use special_keys::{prefix, NODES_PREFIX};
        let bs = self.db.get(prefix(NODES_PREFIX, hash)).unwrap()?;
        let node = msg_types::Node::deserialize(bs).unwrap();

        Some(node)
    }

    pub(crate) fn get_generic_qc(&self) -> QuorumCertificate { 
        let node_with_generic_qc = { 
            let hash = self.db.get(special_keys::NODE_CONTAINING_GENERIC_QC).unwrap().unwrap();
            self.get_node(&hash.try_into().unwrap()).unwrap()
        };

        node_with_generic_qc.justify
    } 

    pub(crate) fn get_locked_qc(&self) -> QuorumCertificate { 
        let generic_qc = self.get_generic_qc();
        let node_with_locked_qc = self.get_node(&generic_qc.node_hash).unwrap();

        node_with_locked_qc.justify
    }
}

mod special_keys {
    type Prefix = [u8; 1];

    pub const NODES_PREFIX: Prefix               = [00];
    pub const WRITE_SETS_PREFIX: Prefix          = [01];
    pub const CHILDREN_PREFIX: Prefix            = [02];
    pub const STATE_PREFIX: Prefix               = [03];

    pub const NODE_CONTAINING_GENERIC_QC: Prefix = [10];

    pub fn prefix(prefix: Prefix, key: &[u8]) -> Vec<u8> {
        let mut prefixed_key = Vec::with_capacity(1 + key.len());
        prefixed_key.extend_from_slice(&prefix);
        prefixed_key.extend_from_slice(key);

        prefixed_key
    }
}