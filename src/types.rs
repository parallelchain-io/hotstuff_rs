/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for 'inert' types, i.e., those that are sent around and inspected, but have no active behavior.

use std::{collections::{hash_set, HashSet, hash_map, HashMap}, hash::Hash, slice};
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

pub type ChainID = u64;
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
    pub chain_id: ChainID,
    pub view: ViewNumber,
    pub block: CryptoHash,
    pub phase: Phase,
    pub signatures: SignatureSet,
}

impl QuorumCertificate {
    /// Checks if all of the signatures in the certificate are correct, and if there's the set of signatures forms a quorum.
    /// 
    /// A special case is if the qc is the genesis qc, in which case it is automatically correct.
    pub fn is_correct(&self, validator_set: &ValidatorSet) -> bool {
        if self.is_genesis_qc() {
            true
        } else {
            // Check whether the size of the signature set is the same as the size of the validator set.
            if self.signatures.len() != validator_set.len() {
                return false
            }

            // Check whether every signature is correct and tally up their powers.
            let mut signature_set_power = 0;
            for (signature, (signer, power)) in self.signatures.iter().zip(validator_set.validators_and_powers()) {
                if let Some(signature) = signature {
                    let signer = PublicKey::from_bytes(&signer).unwrap();
                    if let Ok(signature) = Signature::from_bytes(signature) {
                        if signer.verify(&(self.chain_id, self.view, self.block, self.phase).try_to_vec().unwrap(), &signature).is_ok() {
                            signature_set_power += power;
                        } else {
                            // qc contains incorrect signature.
                            return false
                        }
                    } else {
                        // qc contains incorrect signature.
                        return false
                    } 
                }
            }

            // Check if the signatures form a quorum.
            let quorum = Self::quorum(validator_set.total_power());
            if signature_set_power >= quorum {
                true
            } else {
                false
            }
        }
    }

    pub const fn genesis_qc() -> QuorumCertificate {
        QuorumCertificate { 
            chain_id: 0,
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

#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize, Debug)]
pub enum Phase {
    // ↓↓↓ For pipelined flow ↓↓↓ //   
    
    Generic,

    // ↓↓↓ For phased flow ↓↓↓ //

    Prepare, 

    // The inner view number is the view number of the *prepare* qc contained in the nudge which triggered the
    // vote containing this phase.
    Precommit(ViewNumber),

    // The inner view number is the view number of the *precommit* qc contained in the nudge which triggered the
    // vote containing this phase.
    Commit(ViewNumber),
}

impl Phase {
    pub fn is_generic(self) -> bool {
        self == Phase::Generic
    }

    pub fn is_prepare(self) -> bool {
        self == Phase::Prepare
    }

    pub fn is_precommit(self) -> bool {
        if let Phase::Precommit(_) = self {
            true
        } else {
            false
        }
    }

    pub fn is_commit(self) -> bool {
        if let Phase::Commit(_) = self {
            true
        } else {
            false
        }
    }
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

    pub fn get_insert(&self, key: &K) -> Option<&V> {
        self.inserts.get(key)
    } 

    pub fn contains_delete(&self, key: &K) -> bool {
        self.deletes.contains(key)
    }

    /// Get an iterator over all of the key-value pairs inserted by this ChangeSet.
    pub fn inserts(&self) -> hash_map::Iter<K, V> {
        self.inserts.iter()
    } 

    /// Get an iterator over all of the keys that are deleted by this ChangeSet.
    pub fn deletions(&self) -> hash_set::Iter<K> {
        self.deletes.iter()
    }
}

/// Identities of validators and their voting powers.
/// 
/// The validator set maintains the list of validators in ascending order of their public keys, and avails methods:
/// [ValidatorSet::validators] and [Validators::validators_and_powers] to get them in this order. 
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct ValidatorSet {
    // The public keys of validators are included here in ascending order.
    validators: Vec<PublicKeyBytes>,
    powers: HashMap<PublicKeyBytes, Power>,
}

impl ValidatorSet {
    pub fn new() -> ValidatorSet {
        Self {
            validators: Vec::new(),
            powers: HashMap::new(),
        }
    }

    pub fn put(&mut self, validator: &PublicKeyBytes, power: Power) {
        if !self.powers.contains_key(validator) {
            let insert_pos = self.validators.binary_search(validator).unwrap_err();
            self.validators.insert(insert_pos, *validator);
        }

        self.powers.insert(*validator, power);
    }

    pub fn apply_updates(&mut self, updates: &ValidatorSetUpdates) {
        for (peer, new_power) in updates.inserts() {
            self.put(&peer, *new_power);
        }

        for peer in updates.deletions() {
            self.remove(peer);
        }
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

    /// Get an iterator through validators' public keys which walks through them in ascending order.
    pub fn validators(&self) -> slice::Iter<PublicKeyBytes> {
        self.validators.iter()
    }

    /// Get a vector containing each validator and its power, in ascending order of the validators' public keys.
    pub fn validators_and_powers(&self) -> Vec<([u8; 32], u64)> {
        self.validators().map(|v| (*v, *self.power(v).unwrap())).collect()
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

/// Helps leaders incrementally form QuorumCertificates by combining votes for the same chain_id, view, block, and phase by replicas
/// in a given validator set.
pub(crate) struct VoteCollector {
    chain_id: ChainID,
    view: ViewNumber,
    validator_set: ValidatorSet,
    validator_set_power: Power,
    signature_sets: HashMap<(CryptoHash, Phase), (SignatureSet, Power)>,
}

impl VoteCollector {
    pub(crate) fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> VoteCollector {
        let total_validator_set_power = validator_set.total_power();

        Self {
            chain_id,
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
    // vote.chain_id and vote.view must be the same as the chain_id and the view used to create this VoteCollector.
    pub(crate) fn collect(&mut self, signer: &PublicKeyBytes, vote: Vote) -> Option<QuorumCertificate> {
        if self.chain_id != vote.chain_id || self.view != vote.view {
            panic!()
        }

        // Check if the signer is actually in the validator set.
        if let Some(pos) = self.validator_set.position(signer) {

            // If the vote is for a new (block, phase) pair, prepare an empty signature set.
            if !self.signature_sets.contains_key(&(vote.block, vote.phase)) { 
                self.signature_sets.insert((vote.block, vote.phase), (vec![None; self.validator_set.len()], 0));
            }

            let (signature_set, power) = self.signature_sets.get_mut(&(vote.block, vote.phase)).unwrap();

            // If a vote for the (block, phase) from the signer hasn't been collected before, insert it into the signature set.
            if signature_set[pos].is_none() {
                signature_set[pos] = Some(vote.signature);
                *power += self.validator_set.power(signer).unwrap();

                // If inserting the vote makes the signature set form a quorum, then create a quorum certificate.
                if *power >= QuorumCertificate::quorum(self.validator_set_power) {
                    let (signatures, _) = self.signature_sets.remove(&(vote.block, vote.phase)).unwrap();
                    let collected_qc = QuorumCertificate {
                        chain_id: self.chain_id,
                        view: self.view,
                        block: vote.block,
                        phase: vote.phase,
                        signatures,
                    };


                    return Some(collected_qc)
                }
            }
        }

        None
    }
}

pub(crate) struct NewViewCollector {
    validator_set: ValidatorSet,
    validator_set_power: Power,
    collected_from: HashSet<PublicKeyBytes>,
    accumulated_power: Power,
}

impl NewViewCollector {
    pub(crate) fn new(validator_set: ValidatorSet) -> NewViewCollector {
        let validator_set_power = validator_set.total_power();

        Self {
            validator_set,
            validator_set_power,
            collected_from: HashSet::new(),
            accumulated_power: 0,
        }
    }

    /// Notes that we have collected a new view message from the specified replica in the given view. Then, returns whether
    /// by collecting this message we have collected new view messages from a quorum of validators in this view. If the sender
    /// is not part of the validator set, then this function does nothing and returns false.
    pub(crate) fn collect(&mut self, sender: &PublicKeyBytes) -> bool {
        if !self.validator_set.contains(sender) {
            return false
        }

        if !self.collected_from.contains(sender) {
            self.collected_from.insert(*sender);
            self.accumulated_power += self.validator_set.power(sender).unwrap()
        }

        if self.accumulated_power >= QuorumCertificate::quorum(self.validator_set_power) {
            true
        } else {
            false 
        }
    }
}
