use std::collections::{hash_set, HashSet, hash_map, HashMap};
use crate::messages::Vote;

pub use ed25519_dalek::{
    PublicKey,
    Keypair, 
    Signature,
};

pub type AppID = u64;
pub type BlockNumber = u64;
pub type ChildrenList = Vec<CryptoHash>;
pub type CryptoHash = [u8; 32];
pub type Data = Vec<Datum>;
pub type Datum = Vec<u8>;
pub type Key = Vec<u8>;
pub type Power = u64;

// PublicKey in version 1 of dalek doesn't implement hash: https://github.com/dalek-cryptography/ed25519-dalek/issues/183
pub type PublicKeyBytes = [u8; 32];
pub type SignatureBytes = [u8; 64];

pub type ValidatorSetUpdates = Vec<(PublicKeyBytes, Power)>;
pub type Value = Vec<u8>;
pub type ViewNumber = u64;

#[derive(Clone)]
pub struct Block {
    pub num: BlockNumber,
    pub hash: CryptoHash,
    pub justify: QuorumCertificate,
    pub data_hash: CryptoHash,
    pub data: Data,
}

#[derive(Clone)]
pub struct QuorumCertificate {
    pub app_id: AppID,
    pub view: ViewNumber,
    pub block: CryptoHash,
    pub phase: Phase,
    pub signatures: SignatureSet,
}

impl QuorumCertificate {
    pub fn is_correct(&self, validator_set: &ValidatorSet) -> bool {
        todo!()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Phase {
    Generic,
    Prepare,
    Precommit,
    Commit
}

#[derive(Clone)]
pub struct SignatureSet;

#[derive(Clone)]
pub struct AppStateUpdates {
    inserts: HashMap<Key, Value>,
    deletes: HashSet<Key>, 
}

impl AppStateUpdates {
    pub fn new() -> AppStateUpdates {
        AppStateUpdates {
            inserts: HashMap::new(),
            deletes: HashSet::new(),
        }
    }

    pub fn insert(&mut self, key: Key, value: Value) {
        self.deletes.remove(&key);
        self.inserts.insert(key, value);
    }

    pub fn delete(&mut self, key: Key) {
        self.inserts.remove(&key);
        self.deletes.insert(key);
    }

    pub(crate) fn get_insert(&self, key: &Key) -> Option<&Value> {
        self.inserts.get(key)
    } 

    pub(crate) fn contains_delete(&self, key: &Key) -> bool {
        self.deletes.contains(key)
    }

    /// Get an iterator over all of the key, value pairs in this WriteSet.
    pub(crate) fn inserts(&self) -> hash_map::Iter<Key, Value> {
        self.inserts.iter()
    } 

    /// Get an iterator over all of the keys that are deleted by this WriteSet.
    pub(crate) fn deletes(&self) -> hash_set::Iter<Key> {
        self.deletes.iter()
    }
}

pub struct ValidatorSet(Vec<(PublicKeyBytes, Power)>);

impl ValidatorSet {
    pub fn contains(&self, validator: &PublicKeyBytes) -> bool {
        todo!()
    }
}

/// Helps leaders incrementally form QuorumCertificates by combining votes for the same view, block, and phase.
pub(crate) struct VoteCollector;

impl VoteCollector {
    pub(crate) fn collect(&mut self, vote: Vote) -> Option<QuorumCertificate> {
        todo!()
    }
}
