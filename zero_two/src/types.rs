use std::collections::{hash_set, HashSet, hash_map, HashMap};

pub use ed25519_dalek::{
    PublicKey,
    Keypair, 
};

pub type AppID = u64;
pub type BlockNumber = u64;
pub type ChildrenList = Vec<CryptoHash>;
pub type CryptoHash = [u8; 32];
pub type Data = Vec<Datum>;
pub type Datum = Vec<u8>;
pub type Key = Vec<u8>;
pub type Power = u64;
pub type ValidatorSet = Vec<(PublicKey, Power)>;
pub type ValidatorSetUpdates = Vec<(PublicKey, Power)>;
pub type Value = Vec<u8>;
pub type ViewNumber = u64;

pub struct Block {
    num: BlockNumber,
    hash: CryptoHash,
    justify: QuorumCertificate,
    data_hash: CryptoHash,
    data: Data,
}

pub struct QuorumCertificate {
    app_id: AppID,
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
