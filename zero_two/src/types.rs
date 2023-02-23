/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

use std::{collections::{hash_set, HashSet, hash_map::{self, Iter, Keys}, HashMap}, hash::Hash};
use borsh::{BorshSerialize, BorshDeserialize};
use ed25519_dalek::Verifier;
use rand::seq::SliceRandom;
use sha2::Digest;
use crate::messages::Vote;

pub use sha2::Sha256 as CryptoHasher;
pub use ed25519_dalek::{
    Keypair as DalekKeypair,
    PublicKey,
    Signature,
};

pub type AppID = u64;
pub type BlockHeight = u64;
pub type ChildrenList = Vec<CryptoHash>;
pub type CryptoHash = [u8; 32];
pub type Data = Vec<Datum>;
pub type DataLen = u32;
pub type Datum = Vec<u8>;
pub type Power = u64;
pub type PublicKeyBytes = [u8; 32];
pub type SignatureBytes = [u8; 64];
pub type SignatureSet = Vec<Option<SignatureBytes>>;
pub type ViewNumber = u64;

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Block {
    pub height: BlockHeight,
    pub hash: CryptoHash,
    pub justify: QuorumCertificate,
    pub data_hash: CryptoHash,
    pub data: Data,
}

impl Block {
    pub fn new(height: BlockHeight, justify: QuorumCertificate, data_hash: CryptoHash, data: Data) -> Block {
        Block {
            height,
            hash: Block::hash(height, &justify, &data_hash),
            justify,
            data_hash,
            data,
        }
    }

    pub fn hash(height: BlockHeight, justify: &QuorumCertificate, data_hash: &CryptoHash) -> CryptoHash {
        let mut hasher = CryptoHasher::new();
        hasher.update(&height.try_to_vec().unwrap());
        hasher.update(&justify.try_to_vec().unwrap());
        hasher.update(&data_hash.try_to_vec().unwrap());
        hasher.finalize().into()
    }

    /// Checks if hash and justify are cryptographically correct.
    pub fn is_correct(&self, validator_set: &ValidatorSet) -> bool {
        self.hash == Block::hash(self.height, &self.justify, &self.data_hash) &&
        self.justify.is_correct(validator_set)
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub struct QuorumCertificate {
    pub app_id: AppID,
    pub view: ViewNumber,
    pub block: CryptoHash,
    pub phase: Phase,
    pub signatures: SignatureSet,
}

impl QuorumCertificate {
    /// Checks if all of the signatures in the certificate are correct, and if there's the right amount of signatures:
    /// the sum of the powers of the signers must be a quorum, and no strict subset of the signers must be a quorum.
    pub fn is_correct(&self, validator_set: &ValidatorSet) -> bool {
        if self.signatures.len() != validator_set.len() {
            return false
        }

        let quorum = Self::quorum(validator_set.total_power());
        let mut signature_set_power = 0;
        let mut is_already_quorum = false; 
        for (signature, (signer, power)) in self.signatures.iter().zip(validator_set.iter()) {
            if let Some(signature) = signature {
                let signer = PublicKey::from_bytes(signer).unwrap();
                if let Ok(signature) = Signature::from_bytes(signature) {
                    if signer.verify(&(self.app_id, self.view, self.block, self.phase).try_to_vec().unwrap(), &signature).is_ok() {
                        signature_set_power += power;
                        
                        if is_already_quorum {
                            return true 
                        }                        

                        if signature_set_power >= quorum {
                            is_already_quorum = true;
                        }
                    } else {
                        return false
                    }
                } else {
                    return false
                }
            }
        }

        is_already_quorum
    }

    pub const fn genesis_qc() -> QuorumCertificate {
        QuorumCertificate { 
            app_id: 0,
            view: 0,
            block: [0u8; 32],
            phase: Phase::Generic,
            signatures: SignatureSet::new(),
        }
    }

    pub fn is_genesis_qc(&self) -> bool {
        *self == Self::genesis_qc()
    }

    pub fn quorum(validator_set_power: Power) -> Power {
        ((validator_set_power * 2) / 3) + 1
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub enum Phase {
    Generic,
    Prepare,
    Precommit,
    Commit
}

pub type AppStateUpdates = UpdateSet<Vec<u8>, Vec<u8>>;
pub type ValidatorSetUpdates = UpdateSet<PublicKeyBytes, Power>;

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct UpdateSet<K: Eq + Hash, V: Eq + Hash> {
    inserts: HashMap<K, V>,
    deletes: HashSet<K>,
}

impl<K: Eq + Hash, V: Eq + Hash> UpdateSet<K, V> where K:  {
    pub fn new() -> Self {
        Self {
            inserts: HashMap::new(),
            deletes: HashSet::new(),
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        self.deletes.remove(&key);
        self.inserts.insert(key, value);
    }

    pub fn delete(&mut self, key: K) {
        self.inserts.remove(&key);
        self.deletes.insert(key);
    }

    pub(crate) fn get_insert(&self, key: &K) -> Option<&V> {
        self.inserts.get(key)
    } 

    pub(crate) fn contains_delete(&self, key: &K) -> bool {
        self.deletes.contains(key)
    }

    /// Get an iterator over all of the key-value pairs inserted by this ChangeSet.
    pub(crate) fn inserts(&self) -> hash_map::Iter<K, V> {
        self.inserts.iter()
    } 

    /// Get an iterator over all of the keys that are deleted by this ChangeSet.
    pub(crate) fn deletions(&self) -> hash_set::Iter<K> {
        self.deletes.iter()
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct ValidatorSet {
    // The public keys of validators are included here in ascending order.
    validators: Vec<PublicKeyBytes>,
    powers: HashMap<PublicKeyBytes, Power>,
}

impl ValidatorSet {
    pub fn put(&mut self, validator: &PublicKeyBytes, power: Power) {
        if !self.powers.contains_key(validator) {
            let insert_pos = self.validators.binary_search(validator).unwrap_err();
            self.validators.insert(insert_pos, *validator);
        }

        self.powers.insert(*validator, power);
    }

    pub fn power(&self, validator: &PublicKeyBytes) -> Option<&Power> {
        self.powers.get(validator)
    }

    pub fn total_power(&self) -> Power {
        self.powers.values().sum()
    }

    pub fn contains(&self, validator: &PublicKeyBytes) -> bool {
        self.powers.contains_key(validator)
    }

    pub fn remove(&mut self, validator: &PublicKeyBytes) -> Option<(PublicKeyBytes, Power)> {
        if let Ok(pos) = self.validators.binary_search(validator) {
            self.validators.remove(pos);
            self.powers.remove_entry(validator)
        } else {
            None
        }
    }

    pub fn validators(&self) -> Keys<PublicKeyBytes, Power> {
        self.powers.keys()
    }

    pub fn iter(&self) -> Iter<PublicKeyBytes, Power> {
        self.powers.iter()
    }

    pub fn len(&self) -> usize {
        self.validators.len() 
    }

    pub fn position(&self, validator: &PublicKeyBytes) -> Option<usize> {
        match self.validators.binary_search(validator) {
            Ok(pos) => Some(pos),
            Err(_) => None
        }
    } 

    pub(crate) fn random(&self) -> Option<&PublicKeyBytes> {
        self.validators.choose(&mut rand::thread_rng())
    }
}

/// Helps leaders incrementally form QuorumCertificates by combining votes for the same app_id, view, block, and phase.
pub(crate) struct VoteCollector<'a> {
    app_id: AppID,
    view: ViewNumber,
    validator_set: &'a ValidatorSet,
    validator_set_power: Power,
    signature_sets: HashMap<(CryptoHash, Phase), (SignatureSet, Power)>,
}

impl<'a> VoteCollector<'a> {
    pub(crate) fn new(app_id: AppID, view: ViewNumber, validator_set: &'a ValidatorSet) -> VoteCollector<'a> {
        let total_validator_set_power = validator_set.total_power();

        Self {
            app_id,
            view,
            validator_set,
            validator_set_power: total_validator_set_power,
            signature_sets: HashMap::new(),
        }

    }

    // Adds the vote to a signature set for the specified view, block, and phase. Returning a quorum certificate
    // if adding the vote allows for one to be created.
    // 
    // If the vote is not signed correctly, or doesn't match the collector's view, or the signer is not part
    // of its validator set, then this is a no-op.
    //
    // # Preconditions
    // vote.is_correct(signer)
    //
    // # Panics
    // vote.app_id and vote.view must be the same as the app_id and the view used to create this VoteCollector.
    pub(crate) fn collect(&mut self, signer: &PublicKeyBytes, vote: Vote) -> Option<QuorumCertificate> {
        if self.app_id != vote.app_id || self.view != vote.view {
            panic!()
        }

        if let Some(pos) = self.validator_set.position(signer) {
            if let Some((signature_set, power)) = self.signature_sets.get_mut(&(vote.block, vote.phase)) {
                if signature_set.get(pos).is_none() {
                    signature_set[pos] = Some(vote.signature);
                    *power += self.validator_set.power(signer).unwrap();

                    if *power >= QuorumCertificate::quorum(self.validator_set_power) {
                        let (signatures, _) = self.signature_sets.remove(&(vote.block, vote.phase)).unwrap();
                        let collected_qc = QuorumCertificate {
                            app_id: self.app_id,
                            view: self.view,
                            block: vote.block,
                            phase: vote.phase,
                            signatures,
                        };

                        return Some(collected_qc)
                    }
                }
            }
        }

        None
    }
}
