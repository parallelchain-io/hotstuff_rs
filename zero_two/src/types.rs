/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

use std::collections::{hash_set, HashSet, hash_map, HashMap};
use rand::seq::SliceRandom;
use crate::messages::Vote;

pub use ed25519_dalek::{
    Keypair as DalekKeypair,
    PublicKey,
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
pub type PublicKeyBytes = [u8; 32];
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

impl Block {
    /// Checks if data_hash, hash, and justify are cryptographically correct.
    pub fn is_correct(&self, validator_set: &ValidatorSet) -> bool {

    }
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

    pub(crate) fn random(&self) -> Option<&(PublicKeyBytes, u64)> {
        self.0.choose(&mut rand::thread_rng())

    }
}

/// Helps leaders incrementally form QuorumCertificates by combining votes for the same view, block, and phase.
pub(crate) struct VoteCollector<'a> {
    view: ViewNumber,
    validator_set: &'a ValidatorSet,
}

impl<'a> VoteCollector<'a> {
    pub(crate) fn new(view: ViewNumber, validator_set: &ValidatorSet) -> VoteCollector {
        todo!()
    }

    // Adds the vote to a signature set for the specified view, block, and phase. Returning a quorum certificate
    // if adding the vote allows for one to be created.
    // 
    // If the vote is not signed correctly, or doesn't match the collector's view, or the signer is not part
    // of its validator set, then this is a no-op.
    pub(crate) fn collect(&mut self, signer: &PublicKeyBytes, vote: Vote) -> Option<QuorumCertificate> {
        todo!()
    }
}
