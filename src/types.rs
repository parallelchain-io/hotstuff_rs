/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for 'inert' types, i.e., those that are sent around and inspected, but have no active behavior.

use std::{
    collections::{hash_map, hash_set, HashMap, HashSet},
    hash::Hash,
    slice,
};

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Verifier, ed25519::Error};
use rand::seq::SliceRandom;
use sha2::Digest;
pub use ed25519_dalek::{SigningKey, VerifyingKey, Signature};
pub use sha2::Sha256 as CryptoHasher;

use crate::messages::Vote;

pub type ChainID = u64;
pub type BlockHeight = u64;
pub type ChildrenList = Vec<CryptoHash>;
pub type CryptoHash = [u8; 32];
pub type Data = Vec<Datum>;
pub type DataLen = u32;
pub type Datum = Vec<u8>;
pub type Power = u64;
pub type TotalPower = u128;
pub type SignatureBytes = [u8; 64];
pub type SignatureSet = Vec<Option<SignatureBytes>>;
pub type ViewNumber = u64;

type VerifyingKeyBytes = [u8; 32];

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Block {
    pub height: BlockHeight,
    pub hash: CryptoHash,
    pub justify: QuorumCertificate,
    pub data_hash: CryptoHash,
    pub data: Data,
}

impl Block {
    pub fn new(
        height: BlockHeight,
        justify: QuorumCertificate,
        data_hash: CryptoHash,
        data: Data,
    ) -> Block {
        Block {
            height,
            hash: Block::hash(height, &justify, &data_hash),
            justify,
            data_hash,
            data,
        }
    }

    pub fn hash(
        height: BlockHeight,
        justify: &QuorumCertificate,
        data_hash: &CryptoHash,
    ) -> CryptoHash {
        let mut hasher = CryptoHasher::new();
        hasher.update(&height.try_to_vec().unwrap());
        hasher.update(&justify.try_to_vec().unwrap());
        hasher.update(&data_hash.try_to_vec().unwrap());
        hasher.finalize().into()
    }

    /// Checks if hash and justify are cryptographically correct.
    pub fn is_correct(&self, validator_set: &ValidatorSet) -> bool {
        self.hash == Block::hash(self.height, &self.justify, &self.data_hash)
            && self.justify.is_correct(validator_set)
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
                return false;
            }

            // Check whether every signature is correct and tally up their powers.
            let mut total_power: TotalPower = 0;
            for (signature, (signer, power)) in self
                .signatures
                .iter()
                .zip(validator_set.validators_and_powers())
            {
                if let Some(signature) = signature {
                    if let Ok(signature) = Signature::from_slice(signature) {
                        if signer
                            .verify(
                                &(self.chain_id, self.view, self.block, self.phase)
                                    .try_to_vec()
                                    .unwrap(),
                                &signature,
                            )
                            .is_ok()
                        {
                            total_power += power as u128;
                        } else {
                            // qc contains incorrect signature.
                            return false;
                        }
                    } else {
                        // qc contains incorrect signature.
                        return false;
                    }
                }
            }

            // Check if the signatures form a quorum.
            let quorum = Self::quorum(validator_set.total_power());
            total_power >= quorum
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

    pub fn quorum(validator_set_power: TotalPower) -> TotalPower {
        const TOTAL_POWER_OVERFLOW: &str = "Validator set power exceeds u128::MAX/2. Read the itemdoc for Validator Set.";

        (validator_set_power
            .checked_mul(2)
            .expect(TOTAL_POWER_OVERFLOW)
            / 3)
            + 1
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
        matches!(self, Phase::Precommit(_))
    }

    pub fn is_commit(self) -> bool {
        matches!(self, Phase::Commit(_))
    }
}

pub type AppStateUpdates = UpdateSet<Vec<u8>, Vec<u8>>;
pub type ValidatorSetUpdates = UpdateSet<VerifyingKey, Power>;

/// Intermediate representation of [ValidatorSetUpdates] for safe serialization and deserialization.
pub(crate) type ValidatorSetUpdatesBytes = UpdateSet<VerifyingKeyBytes, Power>;

impl TryFrom<ValidatorSetUpdatesBytes> for ValidatorSetUpdates {
    type Error = ed25519_dalek::SignatureError;

    fn try_from(value: ValidatorSetUpdatesBytes) -> Result<Self, Self::Error> {
        let mut new_inserts = <HashMap<VerifyingKey, Power>> :: new();
        let convert_and_insert_inserts = |(k, v): (&[u8; 32], &u64)| -> Result<(), Error> {
            let pk = VerifyingKey::from_bytes(k)?;
            new_inserts.insert(pk, *v); // Safety: Insert should always return None.
            Ok(())
        };
        let _ = value.inserts.keys().zip(value.inserts.values()).try_for_each(convert_and_insert_inserts);

        let mut new_deletes: HashSet<VerifyingKey> = HashSet::new();
        let convert_and_insert_deletes = |k: &[u8; 32]| -> Result<(), Error> {
            let pk = VerifyingKey::from_bytes(k)?;
            new_deletes.insert(pk); // Safety: Insert should never return false.
            Ok(())
        };
        let _ = value.deletes.iter().try_for_each(convert_and_insert_deletes);
        
        let new_validator_set_updates = Self {
            inserts: new_inserts,
            deletes: new_deletes,
        };
        Ok(new_validator_set_updates)
    }
}

impl Into<ValidatorSetUpdatesBytes> for &ValidatorSetUpdates {
    fn into(self) -> ValidatorSetUpdatesBytes {
        let mut new_inserts = <HashMap<VerifyingKeyBytes, Power>> :: new();
        self.inserts.keys().zip(self.inserts.values())
        .for_each(|(k,v)| match new_inserts.insert(k.to_bytes(), *v) {_ => ()}); //Safety: Insert should always return None.

        let mut new_deletes: HashSet<VerifyingKeyBytes> = HashSet::new();
        self.deletes.iter().for_each(|k| match new_deletes.insert(k.to_bytes()) {_ => ()}); //Safety: Insert should never return false.

        ValidatorSetUpdatesBytes {
            inserts: new_inserts,
            deletes: new_deletes,
        }
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct UpdateSet<K: Eq + Hash, V: Eq + Hash> {
    inserts: HashMap<K, V>,
    deletes: HashSet<K>,
}

impl<K: Eq + Hash, V: Eq + Hash> UpdateSet<K, V>
where
    K:,
{
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
/// The validator set maintains the list of validators in ascending order of their [public keys](VerifyingKey), and avails methods:
/// [ValidatorSet::validators] and [ValidatorSet::validators_and_powers] to get them in this order.
/// 
/// # Limits to total power
/// 
/// The total power of a validator set must not exceed `u128::MAX/2`.
#[derive(Clone)]
pub struct ValidatorSet {
    // The verifying keys of validators are included here in ascending order.
    validators: Vec<VerifyingKey>,
    powers: HashMap<VerifyingKey, Power>,
}

impl Default for ValidatorSet {
    fn default() -> Self {
        ValidatorSet::new()
    }
}

impl ValidatorSet {
    pub fn new() -> ValidatorSet {
        Self {
            validators: Vec::new(),
            powers: HashMap::new(),
        }
    }

    pub fn put(&mut self, validator: &VerifyingKey, power: Power) {
        if !self.powers.contains_key(validator) {
            let validator_bytes = validator.to_bytes();
            let insert_pos = self.validators.binary_search_by(|v| v.to_bytes().cmp(&validator_bytes)).unwrap_err();
            self.validators.insert(insert_pos, *validator);
        }

        self.powers.insert(*validator, power);
    }

    pub fn apply_updates(&mut self, updates: &ValidatorSetUpdates) {
        for (peer, new_power) in updates.inserts() {
            self.put(peer, *new_power);
        }

        for peer in updates.deletions() {
            self.remove(peer);
        }
    }

    pub fn power(&self, validator: &VerifyingKey) -> Option<&Power> {
        self.powers.get(validator)
    }

    pub fn total_power(&self) -> TotalPower {
        let mut total_power = 0; 
        for power in self.powers.values() {
            total_power += *power as TotalPower
        }
        total_power
    }

    pub fn contains(&self, validator: &VerifyingKey) -> bool {
        self.powers.contains_key(validator)
    }

    pub fn remove(&mut self, validator: &VerifyingKey) -> Option<(VerifyingKey, Power)> {
        let validator_bytes = validator.to_bytes();
        if let Ok(pos) = self.validators.binary_search_by(|v| v.to_bytes().cmp(&validator_bytes)) {
            self.validators.remove(pos);
            self.powers.remove_entry(validator)
        } else {
            None
        }
    }

    /// Get an iterator through validators' verifying keys which walks through them in ascending order.
    pub fn validators(&self) -> slice::Iter<VerifyingKey> {
        self.validators.iter()
    }

    /// Get a vector containing each validator and its power, in ascending order of the validators' verifying keys.
    pub fn validators_and_powers(&self) -> Vec<(VerifyingKey, u64)> {
        self.validators()
            .map(|v| (*v, *self.power(v).unwrap()))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.validators.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn position(&self, validator: &VerifyingKey) -> Option<usize> {
        let validator_bytes = validator.to_bytes();
        match self.validators.binary_search_by(|v| v.to_bytes().cmp(&validator_bytes)) {
            Ok(pos) => Some(pos),
            Err(_) => None,
        }
    }

    pub(crate) fn random(&self) -> Option<&VerifyingKey> {
        self.validators.choose(&mut rand::thread_rng())
    }
}

/// Intermediate representation of [ValidatorSet] for safe serialization and deserialization.
/// 
/// To serialize an instance of `ValidatorSet`, convert it a `ValidatorSetBytes` using this type's implementation of
/// `Into`, then, serialize the `ValidatorSetBytes` using Borsh. Reverse the steps to deserialize a `ValidatorSet`.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub(crate) struct ValidatorSetBytes {
    // The verifying keys of validators are included here in ascending order.
    validators: Vec<VerifyingKeyBytes>,
    powers: HashMap<VerifyingKeyBytes, Power>,
}

impl TryFrom<ValidatorSetBytes> for ValidatorSet {
    type Error = ed25519_dalek::SignatureError;

    fn try_from(value: ValidatorSetBytes) -> Result<Self, Self::Error> {
        let new_validators = value.validators.iter().flat_map(|pk_bytes| VerifyingKey::from_bytes(pk_bytes)).collect();

        let mut new_powers = <HashMap<VerifyingKey, Power>> :: new();
        let convert_and_insert = |(k, v): (&[u8; 32], &u64)| -> Result<(), Error> {
            let pk = VerifyingKey::from_bytes(k)?;
            new_powers.insert(pk, *v);
            Ok(())
        };
        let _ = value.powers.keys().zip(value.powers.values()).try_for_each(convert_and_insert);

        let new_validator_set = Self {
            validators: new_validators,
            powers: new_powers,
        };
        Ok(new_validator_set)
    }
}

impl Into<ValidatorSetBytes> for &ValidatorSet {
    fn into(self) -> ValidatorSetBytes {
        let new_validators = self.validators.iter().map(|pk| pk.to_bytes()).collect();

        let mut new_powers = <HashMap<VerifyingKeyBytes, Power>> :: new();
        self.powers.keys().zip(self.powers.values())
        .for_each(|(k,v)| match new_powers.insert(k.to_bytes(), *v) {_ => ()}); // Safety: Insert should always return None

        ValidatorSetBytes {
            validators: new_validators,
            powers: new_powers,
        }
    }
}

/// Helps leaders incrementally form [QuorumCertificate]s by combining votes for the same chain_id, view, block, and phase by replicas
/// in a given [validator set](ValidatorSet).
pub(crate) struct VoteCollector {
    chain_id: ChainID,
    view: ViewNumber,
    validator_set: ValidatorSet,
    validator_set_total_power: TotalPower,
    signature_sets: HashMap<(CryptoHash, Phase), (SignatureSet, TotalPower)>,
}

impl VoteCollector {
    pub(crate) fn new(
        chain_id: ChainID,
        view: ViewNumber,
        validator_set: ValidatorSet,
    ) -> VoteCollector {
        Self {
            chain_id,
            view,
            validator_set_total_power: validator_set.total_power(),
            validator_set,
            signature_sets: HashMap::new(),
        }
    }

    /// Adds the vote to a signature set for the specified view, block, and phase. Returning a quorum certificate
    /// if adding the vote allows for one to be created.
    ///
    /// If the vote is not signed correctly, or doesn't match the collector's view, or the signer is not part
    /// of its validator set, then this is a no-op.
    ///
    /// # Preconditions
    /// vote.is_correct(signer)
    ///
    /// # Panics
    /// vote.chain_id and vote.view must be the same as the chain_id and the view used to create this VoteCollector.
    pub(crate) fn collect(
        &mut self,
        signer: &VerifyingKey,
        vote: Vote,
    ) -> Option<QuorumCertificate> {
        if self.chain_id != vote.chain_id || self.view != vote.view {
            panic!()
        }

        // Check if the signer is actually in the validator set.
        if let Some(pos) = self.validator_set.position(signer) {
            // If the vote is for a new (block, phase) pair, prepare an empty signature set.
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.signature_sets.entry((vote.block, vote.phase))
            {
                e.insert((vec![None; self.validator_set.len()], 0));
            }

            let (signature_set, signature_set_power) = self
                .signature_sets
                .get_mut(&(vote.block, vote.phase))
                .unwrap();

            // If a vote for the (block, phase) from the signer hasn't been collected before, insert it into the signature set.
            if signature_set[pos].is_none() {
                signature_set[pos] = Some(vote.signature);
                *signature_set_power += *self.validator_set.power(signer).unwrap() as u128;

                // If inserting the vote makes the signature set form a quorum, then create a quorum certificate.
                if *signature_set_power >= QuorumCertificate::quorum(self.validator_set_total_power) {
                    let (signatures, _) = self
                        .signature_sets
                        .remove(&(vote.block, vote.phase))
                        .unwrap();
                    let collected_qc = QuorumCertificate {
                        chain_id: self.chain_id,
                        view: self.view,
                        block: vote.block,
                        phase: vote.phase,
                        signatures,
                    };

                    return Some(collected_qc);
                }
            }
        }

        None
    }
}

pub(crate) struct NewViewCollector {
    validator_set: ValidatorSet,
    total_power: TotalPower,
    collected_from: HashSet<VerifyingKey>,
    accumulated_power: TotalPower,
}

impl NewViewCollector {
    pub(crate) fn new(validator_set: ValidatorSet) -> NewViewCollector {
        Self {
            total_power: validator_set.total_power(),
            validator_set,
            collected_from: HashSet::new(),
            accumulated_power: 0,
        }
    }

    /// Notes that we have collected a new view message from the specified replica in the given view. Then, returns whether
    /// by collecting this message we have collected new view messages from a quorum of validators in this view. If the sender
    /// is not part of the validator set, then this function does nothing and returns false.
    pub(crate) fn collect(&mut self, sender: &VerifyingKey) -> bool {
        if !self.validator_set.contains(sender) {
            return false;
        }

        if !self.collected_from.contains(sender) {
            self.collected_from.insert(*sender);
            self.accumulated_power += *self.validator_set.power(sender).unwrap() as u128;
        }

        self.accumulated_power >= QuorumCertificate::quorum(self.total_power)
    }
}
