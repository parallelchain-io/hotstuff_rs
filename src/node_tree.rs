use crate::types;
use crate::Database;

pub struct NodeTree {
    // Stores /only/ committed things.
    database: Database,
}

impl NodeTree {
    fn open() -> NodeTree { todo!() } 

    // In the case nodes[0].qc.node_hash does not refer to an existing node, this is a no-op.
    fn insert_nodes(&mut self, nodes: &[ExecutedNode]) { todo!() }
    fn get_locked_qc(&self) -> QuorumCertificate { todo!() }
    fn get_prepare_qc(&self) -> QuorumCertificate { todo!() }
}

pub(crate) struct ExecutedNode(Node, WriteSet);

pub struct Node(Command, QuorumCertificate);

pub struct QuorumCertificate(ViewNumber, NodeHash, Signatures);


pub type WriteSet = Vec<(Key, Value)>;
pub type Key = Vec<u8>;
pub type Value = Vec<u8>;

pub type ViewNumber = u64;
pub type NodeHash = [u8; 32];
pub type Signature = [u8; 64];
pub type Signatures = []
pub type Command = Vec<u8>; 
