/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for the [ValidatorSet] and [ValidatorSetUpdates] types and their associated methods.

use std::{
    collections::{HashMap, HashSet},
    slice,
};
use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::ed25519::Error;
use rand::seq::SliceRandom;
pub use ed25519_dalek::{SigningKey, VerifyingKey, Signature};

use super::basic::{BlockHeight, CryptoHash, Power, TotalPower, UpdateSet};

/// Internal type used for serializing and deserializing values of type [VerifyingKey].
type VerifyingKeyBytes = [u8; 32];

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
        let mut total_power = TotalPower::new(0); 
        for power in self.powers.values() {
            total_power += *power
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
    pub fn validators_and_powers(&self) -> Vec<(VerifyingKey, Power)> {
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

    pub(crate) fn quorum(&self) -> TotalPower {
        const TOTAL_POWER_OVERFLOW: &str = "Validator set power exceeds u128::MAX/2. Read the itemdoc for Validator Set.";

        TotalPower::new(
        (self.total_power()
            .int()
            .checked_mul(2)
            .expect(TOTAL_POWER_OVERFLOW)
            / 3
            )
            + 1
        )
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
        let convert_and_insert = |(k, v): (&[u8; 32], &Power)| -> Result<(), Error> {
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

pub type ValidatorSetUpdates = UpdateSet<VerifyingKey, Power>;

/// Intermediate representation of [ValidatorSetUpdates] for safe serialization and deserialization.
pub(crate) type ValidatorSetUpdatesBytes = UpdateSet<VerifyingKeyBytes, Power>;

impl TryFrom<ValidatorSetUpdatesBytes> for ValidatorSetUpdates {
    type Error = ed25519_dalek::SignatureError;

    fn try_from(value: ValidatorSetUpdatesBytes) -> Result<Self, Self::Error> {
        let mut new_inserts = <HashMap<VerifyingKey, Power>> :: new();
        let convert_and_insert_inserts = |(k, v): (&[u8; 32], &Power)| -> Result<(), Error> {
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

/// Read-only interface for obtaining information about the current (i.e., committed) validator set, and 
/// the validator set history. It is a protocol invariant that a hotstuff-rs replica only needs to know 
/// the previous validator set to validate QCs and TCs, hence we only store the previous validator set 
/// in the history.
#[derive(Clone)]
pub struct ValidatorSetState {
    committed_validator_set: ValidatorSet,
    previous_validator_set: ValidatorSet,
    update_height: BlockHeight,
    update_complete: bool,
}

// TODO: add a block_tree method that returns ValidatorSetState. This method shall replace the calls
// to block_tree.committed_validator_set() in most places.

impl ValidatorSetState {
    pub(crate) fn new(
        committed_validator_set: ValidatorSet, 
        previous_validator_set: ValidatorSet,
        update_height: BlockHeight,
        update_complete: bool,
    ) -> Self {
        Self { 
            committed_validator_set, 
            previous_validator_set,
            update_height,
            update_complete,
        }
    }

    pub fn committed_validator_set(&self) -> &ValidatorSet {
        &self.committed_validator_set
    }

    pub fn previous_validator_set(&self) -> &ValidatorSet {
        &self.previous_validator_set
    }

    pub fn update_height(&self) -> &BlockHeight {
        &self.update_height
    }

    pub fn update_complete(&self) -> bool {
        self.update_complete
    }

}

