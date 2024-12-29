//! Types that store updates to the App-mutable states.

use std::{
    collections::{hash_map, hash_set, HashMap, HashSet},
    hash::Hash,
};

use borsh::{BorshDeserialize, BorshSerialize};

use super::{
    crypto_primitives::{SignatureError, VerifyingKey},
    data_types::Power,
};

/// Generic set of key-value updates that are committed when a particular block is committed.
///
/// This generic type currently forms the basis of two concrete types: [`AppStateUpdates`] and
/// [`ValidatorSetUpdates`].
///
/// # Uniqueness of Key between `inserts` and `deletes`
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct UpdateSet<K: Eq + Hash, V: Eq + Hash> {
    /// Insertion updates that will be committed when a `Block` is committed.
    inserts: HashMap<K, V>,

    /// Deletion updates that will be committed when a `Block` is committed.
    deletes: HashSet<K>,
}

impl<K: Eq + Hash, V: Eq + Hash> UpdateSet<K, V>
where
    K:,
{
    /// Create a new `UpdateSet` with empty `inserts` and `deletes`.
    pub fn new() -> Self {
        Self {
            inserts: HashMap::new(),
            deletes: HashSet::new(),
        }
    }

    /// Schedule the insertion of a `key`-`value` pair when the block that corresponds to this `UpdateSet`
    /// gets committed.
    ///
    /// This cancels the deletion of `key`, if it has been scheduled using [`delete`](Self::delete).
    pub fn insert(&mut self, key: K, value: V) {
        self.deletes.remove(&key);
        self.inserts.insert(key, value);
    }

    /// Schedule the deletion of `key` when the block that corresponds to this `UpdateSet` gets committed.
    ///
    /// This cancels the insertion of `key`, if it has been scheduled using [`insert`](Self::insert).
    pub fn delete(&mut self, key: K) {
        self.inserts.remove(&key);
        self.deletes.insert(key);
    }

    /// Get whether the `UpdateSet` is scheduled to insert a value to `key` when the block that corresponds
    /// to this `UpdateSet` gets committed, and if so, returns a reference to that value.
    pub fn get_insert(&self, key: &K) -> Option<&V> {
        self.inserts.get(key)
    }

    /// Check whether the `UpdateSet` is scheduled to delete `key` when the block that corresponds to this
    /// `UpdateSet` gets committed.
    pub fn contains_delete(&self, key: &K) -> bool {
        self.deletes.contains(key)
    }

    /// Get an iterator over all of the key-value pairs that this `UpdateSet` will insert.
    pub fn inserts(&self) -> hash_map::Iter<K, V> {
        self.inserts.iter()
    }

    /// Get an iterator over all of the keys that this `UpdateSet` will delete.
    pub fn deletes(&self) -> hash_set::Iter<K> {
        self.deletes.iter()
    }
}

/// Set of key-value updates committed to the App State when a block is committed.
pub type AppStateUpdates = UpdateSet<Vec<u8>, Vec<u8>>;

/// Set of updates to the validator that are applied when a block is committed.
pub type ValidatorSetUpdates = UpdateSet<VerifyingKey, Power>;

/// Intermediate representation of [ValidatorSetUpdates] for safe serialization and deserialization.
///
/// ## Rationale
///
/// See the [related section](super::validator_set::ValidatorSetBytes#rationale) about `ValidatorSetBytes`.
pub type ValidatorSetUpdatesBytes = UpdateSet<VerifyingKeyBytes, Power>;

/// Internal type used for serializing and deserializing values of type [`VerifyingKey`].
pub type VerifyingKeyBytes = [u8; 32];

impl TryFrom<ValidatorSetUpdatesBytes> for ValidatorSetUpdates {
    type Error = SignatureError;

    fn try_from(vsu_bytes: ValidatorSetUpdatesBytes) -> Result<Self, Self::Error> {
        Ok(ValidatorSetUpdates {
            inserts: vsu_bytes
                .inserts()
                .map(|(vk_bytes, &power)| VerifyingKey::from_bytes(vk_bytes).map(|vk| (vk, power)))
                .collect::<Result<HashMap<VerifyingKey, Power>, Self::Error>>()?,

            deletes: vsu_bytes
                .deletes()
                .map(|vk_bytes| VerifyingKey::from_bytes(vk_bytes))
                .collect::<Result<HashSet<VerifyingKey>, Self::Error>>()?,
        })
    }
}

impl From<&ValidatorSetUpdates> for ValidatorSetUpdatesBytes {
    fn from(vsu: &ValidatorSetUpdates) -> Self {
        ValidatorSetUpdatesBytes {
            inserts: vsu.inserts().map(|(k, v)| (k.to_bytes(), *v)).collect(),

            deletes: vsu.deletes().map(|k| k.to_bytes()).collect(),
        }
    }
}
