use std::array;
use std::mem;
use sha2::Digest;
use sha2::Sha256;
use ed25519_dalek::Signature as DalekSignature;
use crate::identity::{PublicAddr, ParticipantSet};

pub type ViewNumber = u64;
pub type NodeHeight = u64;
pub type NodeHash = [u8; 32];
pub type Command = Vec<u8>;

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
                buf.extend_from_slice(&signature.to_bytes());
            },
            Self::NewView(vn, qc) => {
                buf.extend_from_slice(&Self::PREFIX_NEW_VIEW);
                buf.extend_from_slice(&vn.to_le_bytes());
                buf.extend_from_slice(&qc.serialize());
            }
        }

        return buf
    }

    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError> {
        let mut cursor = 0usize;

        let variant_prefix = bs[cursor..mem::size_of::<KindPrefix>()].try_into()?;
        cursor += mem::size_of::<KindPrefix>();

        let vn = u64::from_le_bytes(bs[cursor..mem::size_of::<ViewNumber>()].try_into()?); 
        cursor += mem::size_of::<ViewNumber>();
        match variant_prefix {
            Self::PREFIX_PROPOSE => {
                let (bytes_read, node) = Node::deserialize(&bs[cursor..])?;
                cursor += bytes_read;
                Ok((cursor, Self::Propose(vn, node)))
            },
            Self::PREFIX_VOTE => {
                let node_hash = bs[cursor..mem::size_of::<NodeHash>()].try_into()?;
                cursor += mem::size_of::<NodeHash>();

                let signature = <[u8; 64]>::try_from(&bs[cursor..64]).unwrap().into();
                cursor += mem::size_of::<Signature>();
                Ok((cursor, Self::Vote(vn, node_hash, signature)))
            },
            Self::PREFIX_NEW_VIEW => {
                let (bytes_read, qc) = QuorumCertificate::deserialize(&bs[cursor..])?;
                cursor += bytes_read;
                Ok((cursor, Self::NewView(vn, qc)))
            },
            _ => Err(DeserializationError) 
        }


    }
}

#[derive(Clone)]
pub struct Node {
    pub hash: NodeHash,
    pub height: NodeHeight,
    pub command: Command,
    pub justify: QuorumCertificate,
}

impl Node {
    pub fn hash(height: NodeHeight, command: &Command, justify: &QuorumCertificate) -> NodeHash {
        Sha256::new()
            .chain_update(height.to_le_bytes())
            .chain_update(command)
            .chain_update(justify.serialize())
            .finalize()
            .into()
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

    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError> {
        let mut cursor = 0;

        let hash = bs[cursor..mem::size_of::<NodeHash>()].try_into()?;
        cursor += mem::size_of::<NodeHash>();

        let height = NodeHeight::from_le_bytes(bs[cursor..mem::size_of::<NodeHeight>()].try_into()?);
        cursor += mem::size_of::<NodeHeight>();

        let command_len = u64::from_le_bytes(bs[cursor..mem::size_of::<u64>()].try_into()?);
        cursor += mem::size_of::<u64>();

        let command = bs[cursor..command_len as usize].to_vec();
        cursor += command_len as usize;

        let (bytes_read, justify) = QuorumCertificate::deserialize(&bs[cursor..])?; 
        cursor += bytes_read;

        let node = Node {
            hash,
            height,
            command,
            justify,
        };

        Ok((cursor, node))
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

    fn deserialize(bs: &[u8]) -> Result<(BytesRead, QuorumCertificate), DeserializationError> {
        let mut cursor = 0;

        let vn = u64::from_le_bytes(bs[cursor..mem::size_of::<u64>()].try_into()?);
        cursor += mem::size_of::<u64>();

        let node_hash = bs[cursor..mem::size_of::<NodeHash>()].try_into()?;
        cursor += mem::size_of::<NodeHash>();

        let (bytes_read, sigs) = SignatureSet::deserialize(&bs[40..])?;
        cursor += bytes_read;

        let qc = QuorumCertificate {
            view_number: vn,
            node_hash,
            sigs,
        };

        Ok((cursor, qc))
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
    pub fn insert(&mut self, signature: Signature, by_public_addr: PublicAddr) -> Result<bool, QCBuilderInsertError> {
        if QuorumCertificate::is_quorum(self.signature_set.count(), self.participant_set.len()) {
            return Err(QCBuilderInsertError::AlreadyAQuorum)
        }

        if let Some(position) = self.participant_set.keys().position(|pk| *pk == by_public_addr) {
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
    pub fn insert(&mut self, index: usize, signature: Signature) -> Result<(), AlreadyInsertedError> {
        if self.signatures[index].is_some() {
            Err(AlreadyInsertedError)
        } else {
            self.signatures[index] = Some(signature);
            self.count += 1;
            Ok(())
        }
    }  

    pub fn count(&self) -> usize {
        self.count
    }
}

pub struct AlreadyInsertedError;

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
                    buf.extend_from_slice(&sig.to_bytes());
                },
                None => {
                    buf.push(Self::NONE_PREFIX);
                }
            }
        }
        buf
    }

    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError> {
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
                    let sig = <Signature>::try_from(&bs[cursor..64]).unwrap().into();
                    cursor += mem::size_of::<Signature>();
                    signatures.push(Some(sig));
                    count += 1;
                }, 
                Self::NONE_PREFIX => {
                    signatures.push(None);
                },
                _ => return Err(DeserializationError)
            }
        }
        
        let sig_set = SignatureSet {
            signatures,
            count,
        };

        Ok((cursor, sig_set))
    }
}

pub type Signature = DalekSignature;

pub trait SerDe: Sized {
    fn serialize(&self) -> Vec<u8>;
    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError>;
}

pub type BytesRead = usize;

#[derive(Debug)]
pub struct DeserializationError;

impl From<array::TryFromSliceError> for DeserializationError {
    fn from(_: array::TryFromSliceError) -> Self {
        DeserializationError
    }
}

impl<T: SerDe> SerDe for Vec<T> {
    fn deserialize(bs: &[u8]) -> Result<(BytesRead, Self), DeserializationError> {
        let mut cursor = 0;

        let num_elems = u64::from_le_bytes(bs[cursor..mem::size_of::<u64>()].try_into()?);
        let mut res = Vec::with_capacity(num_elems as usize);

        for _ in 0..num_elems {
            let (bytes_read, elem) = T::deserialize(&bs[cursor..])?;  
            cursor += bytes_read;
            res.push(elem);
        }

        Ok((cursor, res))
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        let num_elems = self.len() as u64;
        buf.extend_from_slice(&num_elems.to_le_bytes());

        for elem in self {
            buf.append(&mut elem.serialize())
        }

        buf
    }
}
