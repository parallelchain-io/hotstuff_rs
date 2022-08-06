pub trait SerDe {
    fn serialize(&self) -> Vec<u8>;
    fn deserialize(bs: Vec<u8>) -> Result<Vec<u8>, DeserializationError>;
}
pub struct DeserializationError;

pub struct Node(Command, QuorumCertificate);

impl SerDe for Node {
    fn serialize(&self) -> Vec<u8> {
        todo!()
        // Encoding
        // command.length() ++
        // command
    }

    fn deserialize(bs: Vec<u8>) -> Result<Vec<u8>, DeserializationError> {
        todo!()
    }
}

pub struct QuorumCertificate(ViewNumber, NodeHash, Signatures);


pub type ViewNumber = u64;
pub type Command = Vec<u8>; 
pub type NodeHash = [u8; 32];
pub type Signature = [u8; 64];
pub type Signatures = Vec<Option<Signature>>; 
