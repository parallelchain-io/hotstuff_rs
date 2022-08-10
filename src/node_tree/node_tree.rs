use crate::basic_types::*;
use crate::node_tree::types::*;

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
pub struct NodeTree {
    db: rocksdb::DB,
}

impl NodeTree {
    pub(crate) fn open() -> Result<NodeTree, rocksdb::Error> { 
        const DB_PATH: &str = "./database"; 
        let db = rocksdb::DB::open_default(DB_PATH)?;

        Ok(NodeTree {
            db
        })
    } 

    pub(crate) fn insert_node(&mut self, node: ExecutedNode) { 
        // 1. Open WriteBatch.
        let wb = rocksdb::WriteBatch::default();

        // 2. Read and deserialize parent at `node.justify.node_hash` from DB.
        let parent_hash = node.node.justify.node_hash;
        let parent = self.get_stored_node(&parent_hash).unwrap();

        // 3. Add node.hash to parent.children.
        parent.children.push(node.node.hash());

        // 4. (using WriteBatch) Serialize and insert updated node.parent.
        wb.put(parent_hash, parent.serialize());

        // 5. Transform node into a StoredNode with no `children`.
        let stored_node = StoredNode {
            node,
            children: Vec::new()
        };

        // 6. (using WriteBatch) Insert node into DB.
        wb.put(&stored_node.node.node.hash(), stored_node.serialize());

        // 7. (using WriteBatch) Set NODE_CONTAINING_GENERIC_QC to node.hash.
        wb.put(special_keys::STORED_NODE_WITH_GENERIC_QC, stored_node.node.node.hash());

        // 8. Read and deserialize grandparent = node.parent.parent from DB, this can now be committed.
        let grandparent_hash = parent.node.node.justify.node_hash;
        let grandparent = self.get_stored_node(&grandparent_hash).unwrap();
        
        // 9. (Using WriteBatch) Apply grandparent's writes into State.
        for (key, value) in grandparent.node.write_set {
            wb.put(key, value);
        }

        // 10. Commit WriteBatch.
        self.db.write(wb).unwrap()
    }

    pub(crate) fn get_generic_qc(&self) -> QuorumCertificate { 
        let node_with_generic_qc = { 
            let hash = self.db.get(special_keys::STORED_NODE_WITH_GENERIC_QC).unwrap().unwrap();
            self.get_node(&hash.try_into().unwrap()).unwrap()
        };

        node_with_generic_qc.justify
    } 

    pub(crate) fn get_locked_qc(&self) -> QuorumCertificate { 
        let generic_qc = self.get_generic_qc();
        let node_with_locked_qc = self.get_node(&generic_qc.node_hash).unwrap();

        node_with_locked_qc.justify
    }

    pub(crate) fn get_node(&self, hash: &NodeHash) -> Option<Node> {
        let stored_node = self.get_stored_node(hash)?;

        Some(stored_node.node.node)
    }

    fn get_stored_node(&self, hash: &NodeHash) -> Option<StoredNode> {
        let bs = self.db.get(hash).unwrap()?;
        let stored_node = StoredNode::deserialize(bs).unwrap();

        Some(stored_node)
    }
}

mod special_keys {
    // Invariants that are maintained by NodeTree:
    // 1. get_generic_qc().node.justify == LockedQC.
    // 2. get_locked_qc().node == HighestCommittedNode.

    pub const STORED_NODE_WITH_GENERIC_QC: [u8; 1] = [0];
    pub const STATE_KEYSPACE_PREFIX: [u8; 1] = [1];
}