struct BlockTree;

impl BlockTree {
    fn insert_block(block: Block, qc: QuorumCertificate) { todo!() }
    fn insert_qc(qc: QuorumCertificate) { todo!() }
    fn insert_vote(&mut self, vote: Vote) { todo!() }
}

impl BlockTree {
    fn get_locked_qc() -> QuorumCertificate { todo!() }
}

enum Item {
    Node(Block, QuorumCertificate),
    FullQC(QuorumCertificate),
    PartialQC(QuorumCertificate),
}

struct Block;
struct QuorumCertificate;
struct Vote;
