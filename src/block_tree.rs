struct BlockTree;

impl BlockTree {
    fn insert_block(block: Block, qc: QuorumCertificate);
    fn insert_qc(qc: QuorumCertificate);
    fn insert_vote(&mut self, vote: Vote);
}

impl BlockTree {
    fn get_locked_qc() -> QuorumCertificate;
    fn 

}

enum Item {
    Node(Block, QuorumCertificate),
    FullQC(QuorumCertificate),
    PartialQC(QuorumCertificate),
}

struct Block;
struct QuorumCertificate;
struct Vote;
