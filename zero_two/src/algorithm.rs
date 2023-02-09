/// Helps leaders incrementally form QuorumCertificates by combining votes for the same view, block, and phase.
pub struct VoteCollection {
    view: ViewNumber,
    block: CryptoHash,
    phase: Phase,
    signature_set: SignatureSet,
}

impl VoteCollection {

}