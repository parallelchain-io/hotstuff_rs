struct BlockTree;

impl BlockTree {
    fn insert_block(&self, block: Block, qc: QuorumCertificate) { todo!() }
    fn insert_qc(&self, qc: QuorumCertificate) { todo!() }
    fn insert_vote(&self, vote: Vote) { todo!() }
}

impl BlockTree {
    fn get_locked_qc(&self) -> QuorumCertificate { todo!() }
}

enum Item {
    Node(Block, QuorumCertificate),
    FullQC(QuorumCertificate),
    PartialQC(QuorumCertificate),
}

struct Block;
struct QuorumCertificate;
struct Vote;
