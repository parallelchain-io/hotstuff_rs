use std::array;

pub type ViewNumber = u64;
pub type NodeHash = [u8; 32];
pub type Command = Vec<u8>;
pub type PublicAddress = [u8; 32];
pub type Signature = [u8; 64];
pub type Signatures = Vec<Option<Signature>>; 

pub enum ConsensusMsg {
    Propose(ViewNumber, Node),
    Vote(ViewNumber, NodeHash),
    NewView(ViewNumber, QuorumCertificate),
}

impl SerDe for ConsensusMsg {
    fn serialize(&self) -> Vec<u8> {
        todo!()
    }

    fn deserialize(bs: Vec<u8>) -> Result<Self, DeserializationError> {
        todo!()
    }
}

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
        todo!()
        // Encoding
        // command.length() ++
        // command
    }

    fn deserialize(bs: Vec<u8>) -> Result<Self, DeserializationError> {
        todo!()
    }
}

#[derive(Clone)]
pub struct QuorumCertificate {
    pub vn: ViewNumber,
    pub node_hash: NodeHash,
    pub sigs: Signatures,
}


impl SerDe for QuorumCertificate {
    fn serialize(&self) -> Vec<u8> {
        let mut bs = Vec::new();
        bs.extend_from_slice(&self.vn.to_le_bytes());
        bs.extend_from_slice(&self.node_hash);
        bs.extend_from_slice(&self.sigs.serialize());

        return bs
    }

    fn deserialize(bs: Vec<u8>) -> Result<QuorumCertificate, DeserializationError> {
        let vn = u64::from_le_bytes(bs[0..8].try_into()?);
        let node_hash = bs[8..40].try_into()?;
        let sigs = Signatures::deserialize(bs[40..].to_vec())?;

        Ok(QuorumCertificate {
            vn,
            node_hash,
            sigs,
        })
    }
}

impl From<array::TryFromSliceError> for DeserializationError {
    fn from(_: array::TryFromSliceError) -> Self {
        DeserializationError
    }
}

impl SerDe for Signatures {
    // Encoding:
    // if None: 
    // 0 
    // if Some:
    // 1 ++
    // signature
    fn serialize(&self) -> Vec<u8> {
        todo!()
    }

    fn deserialize(bs: Vec<u8>) -> Result<Self, DeserializationError> {
        todo!() 
    }
}

pub trait SerDe: Sized {
    fn serialize(&self) -> Vec<u8>;
    fn deserialize(bs: Vec<u8>) -> Result<Self, DeserializationError>;
}

#[derive(Debug)]
pub struct DeserializationError;
