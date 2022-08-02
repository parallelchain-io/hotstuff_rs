struct NodeTree;

impl NodeTree {
    fn insert_block(&self, block: Block, qc: QuorumCertificate) { todo!() }
    fn insert_qc(&self, qc: QuorumCertificate) { todo!() }
    fn insert_vote(&self, vote: Vote) { todo!() }
}

impl NodeTree {
    fn get_locked_qc(&self) -> QuorumCertificate { todo!() }
}


struct Item(Node, Writes);

struct Node(QuorumCertificate);
struct QuorumCertificate;
struct Vote;
