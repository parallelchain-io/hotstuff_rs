use std::array;
use std::mem;
use crate::identity::{PublicAddr, ParticipantSet};

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

impl QuorumCertificate {
    pub fn is_quorum(num_votes: usize, num_participants: usize) -> bool {
        // '/' here is integer (floor) division. 
        num_votes >= (num_participants * 2) / 3 + 1
    }
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

pub struct QuorumCertificateBuilder {
    view_number: ViewNumber,
    node_hash: NodeHash,
    participant_set: ParticipantSet,
    signature_set: SignatureSet,
}

impl QuorumCertificateBuilder {
    pub fn new(view_number: ViewNumber, node_hash: NodeHash, participant_set: ParticipantSet) -> QuorumCertificateBuilder {
        QuorumCertificateBuilder {
            view_number,
            node_hash,
            signature_set: SignatureSet::new(participant_set.len()),
            participant_set,
        }
    }

    /// - This does not check whether signature is a correct signature.
    /// - Returns Ok(true) when insertion makes QuorumCertificateBuilder contain enough Signatures to form a QuorumCertificate.
    pub fn insert(&mut self, signature: Signature, of_public_addr: PublicAddr) -> Result<bool, QCBuilderInsertError> {
        if QuorumCertificate::is_quorum(self.signature_set.count(), self.participant_set.len()) {
            return Err(QCBuilderInsertError::AlreadyAQuorum)
        }

        if let Some(position) = self.participant_set.keys().position(|public_addr| *public_addr == of_public_addr) {
            self.signature_set.insert(position, signature);

            Ok(QuorumCertificate::is_quorum(self.signature_set.count(), self.participant_set.len()))
        } else {
            Err(QCBuilderInsertError::PublicAddrNotInParticipantSet)
        }
    }

    pub fn into_qc(self) -> QuorumCertificate {
        QuorumCertificate { 
            view_number: self.view_number,
            node_hash: self.node_hash,
            sigs:  self.signature_set
        }
    }
}

pub enum QCBuilderInsertError {
    AlreadyAQuorum,
    PublicAddrNotInParticipantSet,
}

#[derive(Clone)]
pub struct SignatureSet {
    pub signatures: Vec<Option<Signature>>,
    pub count: usize,
} 

impl SignatureSet {
    pub const SOME_PREFIX: u8 = 1;
    pub const NONE_PREFIX: u8 = 0; 

    pub fn new(length: usize) -> SignatureSet {
        let signatures = vec![None; length];
        SignatureSet {
            signatures,
            count: 0,
        }
    }

    /// The caller has the responsibility to ensure that Signatures in the SignatureSet are sorted in ascending order of the
    /// PublicAddr that produced them, i.e., the n-th item in SignatureSet, if Some, was produced by the SecretKey corresponding
    /// to the 'length - n' numerically largest Participant in a ParticipantSet. By imposing an order on SignatureSet, mappings
    /// between PublicAddr and Signature can be omitted from SignatureSet's bytes-encoding, saving message and storage size.
    pub fn insert(&mut self, index: usize, signature: Signature) {
        todo!()
    }  

    pub fn verify(&self, participant_set: ParticipantSet) -> bool {
        todo!()
    }

    pub fn count(&self) -> usize {
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

        let mut count = 0;
        for _ in 0..num_sigs {
            let variant_prefix = u8::from_le_bytes(bs[cursor..mem::size_of::<u8>()].try_into().unwrap());
            cursor += mem::size_of::<u8>();
            match variant_prefix {
                Self::SOME_PREFIX => {
                    let sig = bs[cursor..mem::size_of::<Signature>()].try_into().unwrap();
                    signatures.push(Some(sig));
                    count += 1;
                }, 
                Self::NONE_PREFIX => {
                    signatures.push(None);
                },
                _ => return Err(DeserializationError)
            }
        }

        Ok(SignatureSet {
            signatures, 
            count,
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
