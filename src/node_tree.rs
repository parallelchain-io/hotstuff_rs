use crate::types;
use crate::Database;

pub struct NodeTree {
    // Stores /only/ committed things.
    database: Database,
}

impl NodeTree {
    fn open() -> NodeTree { todo!() } 

    // In the case nodes[0].qc.node_hash does not refer to an existing node, this is a no-op.
    fn insert_nodes(&mut self, nodes: &[types::ExecutedNode]) { todo!() }
    fn get_locked_qc(&self) -> types::QuorumCertificate { todo!() }
    fn get_prepare_qc(&self) -> types::QuorumCertificate { todo!() }
}
