pub(crate) struct ExecutedNode(Node, WriteSet);

pub struct Node(Command, QuorumCertificate);

pub struct QuorumCertificate(ViewNumber, NodeHash, Signatures);

pub(crate) enum ConsensusMsg {
    Proposal(ViewNumber, Node),
    Vote(ViewNumber, NodeHash, Signature),
    NewView(ViewNumber, QuorumCertificate),
}

pub(crate) type WriteSet = Vec<(Vec<u8>, Vec<u8>)>;

pub(crate) type ViewNumber = u64;

pub(crate) type Command = Vec<u8>;

pub(crate) type NodeHash = [u8; 32];

pub(crate) type PublicKey = [u8; 32];

pub(crate) type ParticipantSet = Vec<PublicKey>;

pub(crate) type Signature = [u8; 64];

pub(crate) type Signatures = Vec<Option<Signature>>;
