use std::array;
use std::mem;
use crate::ParticipantSet;

pub type ViewNumber = u64;
pub type NodeHash = [u8; 32];
pub type Command = Vec<u8>;
pub type Signature = [u8; 64];

#[derive(Clone)]
pub enum ConsensusMsg {
    Propose(ViewNumber, Node),
    Vote(ViewNumber, NodeHash, Signature),
    NewView(ViewNumber, QuorumCertificate),
}

type KindPrefix = [u8; 1];

impl ConsensusMsg {
    pub const PREFIX_PROPOSE: KindPrefix = [0u8];
    pub const PREFIX_VOTE: KindPrefix = [1u8];
    pub const PREFIX_NEW_VIEW: KindPrefix = [2u8]; 
}

impl SerDe for ConsensusMsg {
    // # Encodings
    //
    // ## Propose
    // PREFIX_PROPOSE 
    // ++ vn.to_le_bytes()
    // ++ node.serialize()
    // 
    // ## Vote
    // PREFIX_VOTE
    // ++ vn.to_le_bytes()
    // ++ node_hash
    // ++ signature
    //
    // ## NewView
    // PREFIX_NEW_VIEW
    // ++ vn.to_le_bytes()
    // ++ qc.serialize()
    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::Propose(vn, node) => {
                buf.extend_from_slice(&Self::PREFIX_PROPOSE);
                buf.extend_from_slice(&vn.to_le_bytes());
                buf.extend_from_slice(&node.serialize());
            },
            Self::Vote(vn, node_hash, signature) => {
                buf.extend_from_slice(&Self::PREFIX_VOTE);
                buf.extend_from_slice(&vn.to_le_bytes());
                buf.extend_from_slice(node_hash);
                buf.extend_from_slice(signature);
            },
            Self::NewView(vn, qc) => {
                buf.extend_from_slice(&Self::PREFIX_NEW_VIEW);
                buf.extend_from_slice(&vn.to_le_bytes());
                buf.extend_from_slice(&qc.serialize());
            }
        }

        return buf
    }

    fn deserialize(bs: &[u8]) -> Result<Self, DeserializationError> {
        let mut cursor = 0usize;

        let variant_prefix = bs[cursor..mem::size_of::<KindPrefix>()].try_into()?;
        cursor += mem::size_of::<KindPrefix>();

        let vn = u64::from_le_bytes(bs[cursor..mem::size_of::<ViewNumber>()].try_into()?); 
        cursor += mem::size_of::<ViewNumber>();
        match variant_prefix {
            Self::PREFIX_PROPOSE => {
                let node = Node::deserialize(&bs[cursor..])?;
                Ok(Self::Propose(vn, node))
            },
            Self::PREFIX_VOTE => {
                let node_hash = bs[cursor..mem::size_of::<NodeHash>()].try_into()?;
                cursor += mem::size_of::<NodeHash>();

                let signature = bs[cursor..mem::size_of::<Signature>()].try_into()?;
                cursor += mem::size_of::<Signature>();
                Ok(Self::Vote(vn, node_hash, signature))
            },
            Self::PREFIX_NEW_VIEW => {
                let qc = QuorumCertificate::deserialize(&bs[cursor..])?;
                Ok(Self::NewView(vn, qc))
            },
            _ => Err(DeserializationError) 
        }


    }
}

#[derive(Clone)]
pub struct Node {
    pub command: Command,
    pub justify: QuorumCertificate,
}

impl Node {
    pub fn hash(&self) -> NodeHash {
        todo!()
    }
}

impl SerDe for Node {
    fn serialize(&self) -> Vec<u8> {
        let mut bs = Vec::new();
        bs.extend_from_slice(&u64::to_le_bytes(self.command.len() as u64));
        bs.extend_from_slice(&self.command);
        bs.extend_from_slice(&self.justify.serialize());
        bs
    }

    fn deserialize(bs: &[u8]) -> Result<Self, DeserializationError> {
        let mut cursor = 0;

        let command_len = u64::from_le_bytes(bs[cursor..mem::size_of::<u64>()].try_into()?);
        cursor += mem::size_of::<u64>();

        let command = bs[cursor..command_len as usize].to_vec();
        cursor += command_len as usize;

        let justify = QuorumCertificate::deserialize(&bs[cursor..])?; 

        Ok(Node {
            command,
            justify
        })
    }
}

#[derive(Clone)]
pub struct QuorumCertificate {
    pub view_number: ViewNumber,
    pub node_hash: NodeHash,
    pub sigs: SignatureSet,
}

impl SerDe for QuorumCertificate {
    fn serialize(&self) -> Vec<u8> {
        let mut bs = Vec::new();
        bs.extend_from_slice(&self.view_number.to_le_bytes());
        bs.extend_from_slice(&self.node_hash);
        bs.extend_from_slice(&self.sigs.serialize());

        return bs
    }

    fn deserialize(bs: &[u8]) -> Result<QuorumCertificate, DeserializationError> {
        let vn = u64::from_le_bytes(bs[0..8].try_into()?);
        let node_hash = bs[8..40].try_into()?;
        let sigs = SignatureSet::deserialize(&bs[40..])?;

        Ok(QuorumCertificate {
            view_number: vn,
            node_hash,
            sigs,
        })
    }
}

#[derive(Clone)]
pub struct SignatureSet {
    pub signatures: Vec<Option<Signature>>,
} 

impl SignatureSet {
    pub const SOME_PREFIX: u8 = 1;
    pub const NONE_PREFIX: u8 = 0; 

    fn verify(&self, participant_set: ParticipantSet) -> bool {
        todo!()
    }
}

impl SerDe for SignatureSet {
    // Encoding:
    // `participant_set.len()` as u64
    // ++
    // for each signature in signatures:
    //     if Some:
    //         1 ++ signature
    //     if None: 
    //         0 
    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for signature in &self.signatures {
            match signature {
                Some(sig) => { 
                    buf.push(Self::SOME_PREFIX);
                    buf.extend_from_slice(sig);
                },
                None => {
                    buf.push(Self::NONE_PREFIX);
                }
            }
        }
        buf
    }

    fn deserialize(bs: &[u8]) -> Result<Self, DeserializationError> {
        let mut signatures = Vec::new();

        let mut cursor = 0usize;
        let num_sigs = u64::from_le_bytes(bs[0..mem::size_of::<u64>()].try_into().unwrap());
        cursor += mem::size_of::<u64>();

        for _ in 0..num_sigs {
            let variant_prefix = u8::from_le_bytes(bs[cursor..mem::size_of::<u8>()].try_into().unwrap());
            cursor += mem::size_of::<u8>();
            match variant_prefix {
                Self::SOME_PREFIX => {
                    let sig = bs[cursor..mem::size_of::<Signature>()].try_into().unwrap();
                    signatures.push(Some(sig));
                }, 
                Self::NONE_PREFIX => {
                    signatures.push(None);
                },
                _ => return Err(DeserializationError)
            }
        }

        Ok(SignatureSet {
            signatures 
        })
    }
}

pub trait SerDe: Sized {
    fn serialize(&self) -> Vec<u8>;
    fn deserialize(bs: &[u8]) -> Result<Self, DeserializationError>;
}

#[derive(Debug)]
pub struct DeserializationError;

impl From<array::TryFromSliceError> for DeserializationError {
    fn from(_: array::TryFromSliceError) -> Self {
        DeserializationError
    }
}
