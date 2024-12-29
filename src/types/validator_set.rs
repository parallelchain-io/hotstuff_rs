/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Types that store information about validator sets or updates to validator sets.

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::ed25519::Error;
use std::{collections::HashMap, slice};

use super::{
    data_types::{BlockHeight, Power, TotalPower},
    update_sets::{ValidatorSetUpdates, ValidatorSetUpdatesBytes, VerifyingKeyBytes},
};

pub use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

/// Stores the identities of validators and their voting powers.
///
/// ## Ordering of validators
///
/// `ValidatorSet` internally maintains the list of validators in ascending order of their
/// `VerifyingKey`s, and avails the methods [`validators`](ValidatorSet::validators),
/// [`validators_and_powers`](ValidatorSet::validators_and_powers), and
/// [`position`](ValidatorSet::position) that users can use to get them in this order.
///
/// ## Limits to total power
///
/// Users must make sure that the total power of the validator set does not exceed `u128::MAX/2`.
#[derive(Clone, PartialEq)]
pub struct ValidatorSet {
    // The verifying keys of validators are included here in ascending order.
    validators: Vec<VerifyingKey>,
    powers: HashMap<VerifyingKey, Power>,
}

impl Default for ValidatorSet {
    // Create an empty validator set.
    fn default() -> Self {
        ValidatorSet::new()
    }
}

impl ValidatorSet {
    /// Create an empty validator set.
    pub fn new() -> ValidatorSet {
        Self {
            validators: Vec::new(),
            powers: HashMap::new(),
        }
    }

    /// Put a `validator` with the specified `power` into the validator set, placing them in a position that
    /// preserves the [ordering of validators](Self#order-of-validators).
    ///
    /// If `validator` already exists in the validator set, this function updates its power instead.
    pub fn put(&mut self, validator: &VerifyingKey, power: Power) {
        if !self.contains(validator) {
            let validator_bytes = validator.to_bytes();
            let insert_pos = self
                .validators
                .binary_search_by(|v| v.to_bytes().cmp(&validator_bytes))
                .unwrap_err();
            self.validators.insert(insert_pos, *validator);
        }

        self.powers.insert(*validator, power);
    }

    /// Remove `validator` from the validator set, if it actually is in the validator set.
    ///
    /// If a validator is removed, then its `VerifyingKey` is returned with its power in the validator set
    /// before the removal. This `VerifyingKey` will be exactly equal to `validator`. If a validator is not
    /// removed, then this function will return `None`.
    pub fn remove(&mut self, validator: &VerifyingKey) -> Option<(VerifyingKey, Power)> {
        let validator_bytes = validator.to_bytes();
        if let Ok(pos) = self
            .validators
            .binary_search_by(|v| v.to_bytes().cmp(&validator_bytes))
        {
            self.validators.remove(pos);
            self.powers.remove_entry(validator)
        } else {
            None
        }
    }

    /// Apply validator set `updates` to the validator set. This entails calling `put` for all of the
    /// validators inserted, and calling `remove` for all of the validators deleted.
    pub fn apply_updates(&mut self, updates: &ValidatorSetUpdates) {
        for (peer, new_power) in updates.inserts() {
            self.put(peer, *new_power);
        }

        for peer in updates.deletes() {
            self.remove(peer);
        }
    }

    /// Get the power of the specified `validator` inside the validator set.
    pub fn power(&self, validator: &VerifyingKey) -> Option<&Power> {
        self.powers.get(validator)
    }

    /// Get the sum of the powers of all of the validators inside the validator set.
    pub fn total_power(&self) -> TotalPower {
        let mut total_power = TotalPower::new(0);
        for power in self.powers.values() {
            total_power += *power
        }
        total_power
    }

    /// Check whether the validator set contains `validator`.
    pub fn contains(&self, validator: &VerifyingKey) -> bool {
        self.powers.contains_key(validator)
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

    /// Get the number of validators currently in the validator set.
    pub fn len(&self) -> usize {
        self.validators.len()
    }

    /// Check whether the validator set is empty (i.e., `self.len() == 0`).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the index of the given `validator` in the [sorted order](Self#order-of-validators) of
    /// `VerifyingKey`s in the validator set, if it is actually in the validator set.
    pub fn position(&self, validator: &VerifyingKey) -> Option<usize> {
        let validator_bytes = validator.to_bytes();
        match self
            .validators
            .binary_search_by(|v| v.to_bytes().cmp(&validator_bytes))
        {
            Ok(pos) => Some(pos),
            Err(_) => None,
        }
    }

    /// Compute the total power that a certificate must match or exceed (`>=`) in order to count as a quorum
    /// under the validator set.
    pub(crate) fn quorum(&self) -> TotalPower {
        const TOTAL_POWER_OVERFLOW: &str =
            "Validator set power exceeds u128::MAX/2. Read the itemdoc for `ValidatorSet`.";

        TotalPower::new(
            (self
                .total_power()
                .int()
                .checked_mul(2)
                .expect(TOTAL_POWER_OVERFLOW)
                / 3)
                + 1,
        )
    }
}

/// Intermediate representation of [`ValidatorSet`] for safe serialization and deserialization.
///
/// To serialize an instance of `ValidatorSet`, convert it a `ValidatorSetBytes` using the former type's
/// implementation of `Into<ValidatorSetBytes>`, then, serialize the `ValidatorSetBytes` using Borsh.
/// Reverse the steps to deserialize a `ValidatorSet`.
///
/// ## Rationale
///
/// This type exists because it is not straightforward to implement `BorshSerialize` and
/// `BorshDeserialize` on `ValidatorSet`, since the latter type internally contains
/// [`ed25519_dalek::VerifyingKey`], which does not implement the Borsh traits.
///
/// This type is internally exactly like `ValidatorSet`, but replaces `VerifyingKey` with
/// `VerifyingKeyBytes`, and so is straightforward to serialize and deserialize. However, this also means
/// that instances of this type are not guaranteed to contain "valid" Ed25519 verifying keys, and
/// therefore conversion from this type into `ValidatorSet` using `TryFrom` is fallible.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub(crate) struct ValidatorSetBytes {
    // The verifying keys of validators are included here in ascending order.
    validators: Vec<VerifyingKeyBytes>,
    powers: HashMap<VerifyingKeyBytes, Power>,
}

impl TryFrom<ValidatorSetBytes> for ValidatorSet {
    type Error = ed25519_dalek::SignatureError;

    fn try_from(value: ValidatorSetBytes) -> Result<Self, Self::Error> {
        let new_validators = value
            .validators
            .iter()
            .flat_map(|pk_bytes| VerifyingKey::from_bytes(pk_bytes))
            .collect();

        let mut new_powers = <HashMap<VerifyingKey, Power>>::new();
        let convert_and_insert = |(k, v): (&[u8; 32], &Power)| -> Result<(), Error> {
            let pk = VerifyingKey::from_bytes(k)?;
            new_powers.insert(pk, *v);
            Ok(())
        };
        let _ = value
            .powers
            .keys()
            .zip(value.powers.values())
            .try_for_each(convert_and_insert);

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

        let mut new_powers = <HashMap<VerifyingKeyBytes, Power>>::new();
        self.powers
            .keys()
            .zip(self.powers.values())
            .for_each(|(k, v)| match new_powers.insert(k.to_bytes(), *v) {
                _ => (),
            }); // Safety: Insert should always return None

        ValidatorSetBytes {
            validators: new_validators,
            powers: new_powers,
        }
    }
}

/// Collection of basic information about the current (i.e., committed) validator set, and the validator
/// set history.
///
/// It is a protocol invariant that a Hotstuff-rs replica only needs to know the previous validator set
/// to validate PCs and TCs, hence we only store the previous validator set in the history.
#[derive(Clone)]
pub struct ValidatorSetState {
    committed_validator_set: ValidatorSet,
    previous_validator_set: ValidatorSet,
    update_height: Option<BlockHeight>,
    update_decided: bool,
}

impl ValidatorSetState {
    /// Create a new `ValidatorSetState`.
    pub fn new(
        committed_validator_set: ValidatorSet,
        previous_validator_set: ValidatorSet,
        update_height: Option<BlockHeight>,
        update_decided: bool,
    ) -> Self {
        Self {
            committed_validator_set,
            previous_validator_set,
            update_height,
            update_decided,
        }
    }

    /// Get the committed validator set.
    pub fn committed_validator_set(&self) -> &ValidatorSet {
        &self.committed_validator_set
    }

    /// Get the previous validator set.
    pub fn previous_validator_set(&self) -> &ValidatorSet {
        &self.previous_validator_set
    }

    /// Get the height of the block that caused the most recently committed (but perhaps not decided)
    /// validator set update.
    pub fn update_height(&self) -> &Option<BlockHeight> {
        &self.update_height
    }

    /// Get whether or not the latest validator set update (the one from
    /// [`previous_validator_set`](Self::previous_validator_set) to
    /// [`committed_validator_set`](Self::committed_validator_set)) has been decided.
    pub fn update_decided(&self) -> bool {
        self.update_decided
    }
}

/// Wraps around [`ValidatorSetUpdates`], providing additional information on whether the updates have
/// already been applied or not.
///
/// The ["Validator Set Updates Status" state variable](crate::block_tree::variables#validator-set) in
/// the block tree stores a mapping from blocks to their associated `ValidatorSetUpdatesStatus`.
pub enum ValidatorSetUpdatesStatus {
    /// The block does not update the validator set.
    None,

    /// The block's validator set updates have not been applied to the committed validator set yet.
    Pending(ValidatorSetUpdates),

    /// The block's validator set updates have been applied to the committed validator set.
    Committed,
}

impl ValidatorSetUpdatesStatus {
    /// Check whether the updates status is `Pending` or `Committed`.
    pub fn contains_updates(&self) -> bool {
        match self {
            Self::None => false,
            Self::Pending(_) | Self::Committed => true,
        }
    }

    /// Check whether the updates status is `Pending`.
    pub fn is_pending(&self) -> bool {
        match self {
            Self::Pending(_) => true,
            _ => false,
        }
    }
}

/// Intermediate representation of [`ValidatorSetUpdatesStatus`] for safe serialization and
/// deserialization.
///
/// ## Rationale
///
/// See the [related section](ValidatorSetBytes#rationale) about `ValidatorSetBytes`.

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum ValidatorSetUpdatesStatusBytes {
    None,
    Pending(ValidatorSetUpdatesBytes),
    Committed,
}

impl TryFrom<ValidatorSetUpdatesStatusBytes> for ValidatorSetUpdatesStatus {
    type Error = ed25519_dalek::SignatureError;

    fn try_from(value: ValidatorSetUpdatesStatusBytes) -> Result<Self, Self::Error> {
        Ok(match value {
            ValidatorSetUpdatesStatusBytes::None => ValidatorSetUpdatesStatus::None,
            ValidatorSetUpdatesStatusBytes::Pending(vs_updates_bytes) => {
                ValidatorSetUpdatesStatus::Pending(ValidatorSetUpdates::try_from(vs_updates_bytes)?)
            }
            ValidatorSetUpdatesStatusBytes::Committed => ValidatorSetUpdatesStatus::Committed,
        })
    }
}
