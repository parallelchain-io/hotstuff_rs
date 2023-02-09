use std::collections::HashMap;

pub type BlockNumber = u64;
pub type ViewNumber = u64;
pub type PublicKey = [u8; 32]; 
pub type Power = u64;
pub type CryptoHash = [u8; 32];
pub type Data = Vec<Datum>;
pub type Datum = Vec<u8>;
pub type Key = Vec<u8>;
pub type Value = Vec<u8>;
pub type AppStateUpdates = HashMap<Key, Value>;
pub type ValidatorSetUpdates = HashMap<Key, Value>;

pub struct Block {
    num: BlockNumber,
    hash: CryptoHash,
    justify: QuorumCertificate,
    data_hash: CryptoHash,
    data: Data,
}

pub struct QuorumCertificate {
    view: ViewNumber,
    block: CryptoHash,
    phase: Phase,
    signatures: SignatureSet,
}

pub enum Phase {
    Generic,
    Prepare,
    Precommit,
    Commit
}
