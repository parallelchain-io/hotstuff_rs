use crate::basic_types::*;
use crate::node_tree::types::*;

pub struct NodeTree {
    db: rocksdb::DB,
}

impl NodeTree {
    fn open() -> Result<NodeTree, rocksdb::Error> { 
        const DB_PATH: &str = "./database"; 
        let db = rocksdb::DB::open_default(DB_PATH)?;

        Ok(NodeTree {
            db
        })
    } 

    fn insert_node(&mut self, node: ExecutedNode) { 
        // 0. Open WriteBatch.

        // 1. Read and deserialize node.parent from DB.

        // 2. Add node.hash to node.parent.children.

        // 3. (using WriteBatch) Serialize and insert updated node.parent.

        // 4. Transform node into a StoredNode with no `children`.

        // 5. (using WriteBatch) Insert node into DB.

        // 6. (using WriteBatch) Set NODE_CONTAINING_PREPARE_QC to node.hash.

        // 7. (using WriteBatch) Set NODE_CONTAINING_LOCKED_QC to node.parent.hash.

        // 8. Read and deserialize node.parent.parent from DB.
        
        // 9. (Using WriteBatch) Apply node.parent.parent.writes into State.

        // 10. Commit WriteBatch.
        
        // 11. Return.
    }

    fn get_prepare_qc(&self) -> QuorumCertificate { todo!() } 
    fn get_locked_qc(&self) -> QuorumCertificate { todo!() }
}

mod special_keys {
    pub const NODE_CONTAINING_PREPARE_QC: [u8; 1] = [00]; 
    pub const NODE_CONTAINING_LOCKED_QC: [u8; 1] = [01];
}