/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! The types and traits defined in [`crate::types`] are either common across the sub-protocols used by
//! Hotstuff-rs.
//!
//! Other types and traits, specific to the components of the hotstuff-rs protocol, can be found in
//! the respective directories.
//!
//! The types defined in [`crate::types::basic`] include:
//! 1. "Inert" types, i.e., those that are sent around and inspected, but have no active behavior. These
//!    types follow the newtype pattern and the API for using these types is defined in this module.
//! 2. The [`UpdateSet`] type, which represents generic-type state updates associated with committing a
//!    block.

use borsh::{BorshDeserialize, BorshSerialize};
use std::{
    collections::{hash_map, hash_set, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
    ops::{Add, AddAssign, Sub, SubAssign},
};

/// Id of the blockchain, used to identify the blockchain.
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct ChainID(u64);

impl ChainID {
    pub const fn new(int: u64) -> Self {
        Self(int)
    }

    pub const fn int(&self) -> u64 {
        self.0
    }
}

/// Height of an existing block in the blockchain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, BorshDeserialize, BorshSerialize)]
pub struct BlockHeight(u64);

impl BlockHeight {
    pub fn new(int: u64) -> Self {
        Self(int)
    }

    pub const fn int(&self) -> u64 {
        self.0
    }

    pub fn to_le_bytes(&self) -> [u8; 8] {
        self.0.to_le_bytes()
    }
}

impl Display for BlockHeight {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl AddAssign<u64> for BlockHeight {
    fn add_assign(&mut self, rhs: u64) {
        self.0.add_assign(rhs)
    }
}

impl Add<u64> for BlockHeight {
    type Output = BlockHeight;
    fn add(self, rhs: u64) -> Self::Output {
        BlockHeight::new(self.0.add(rhs))
    }
}

impl Sub<BlockHeight> for BlockHeight {
    type Output = u64;
    fn sub(self, rhs: BlockHeight) -> Self::Output {
        self.0 - rhs.0
    }
}

/// Set of blocks which represents the children of an existing block in the blockchain.
#[derive(Clone, PartialEq, Eq, BorshDeserialize, BorshSerialize, Default)]
pub struct ChildrenList(Vec<CryptoHash>);

impl ChildrenList {
    pub(crate) fn new(blocks: Vec<CryptoHash>) -> Self {
        Self(blocks)
    }

    pub const fn vec(&self) -> &Vec<CryptoHash> {
        &self.0
    }

    pub fn iter(&self) -> std::slice::Iter<'_, CryptoHash> {
        self.0.iter()
    }

    pub(crate) fn push(&mut self, value: CryptoHash) {
        self.0.push(value)
    }
}

/// The hash of a block. Given a [block][crate::types::block::Block]
/// the hash is obtained [like this][crate::types::block::Block::hash].
#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshDeserialize, BorshSerialize)]
pub struct CryptoHash([u8; 32]);

impl CryptoHash {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl Display for CryptoHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Debug for CryptoHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Data stored in a [block][crate::types::block::Block].
#[derive(Clone, PartialEq, Eq, Hash, BorshDeserialize, BorshSerialize)]
pub struct Data(Vec<Datum>);

impl Data {
    pub fn new(datum_vec: Vec<Datum>) -> Self {
        Self(datum_vec)
    }

    pub const fn vec(&self) -> &Vec<Datum> {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Datum> {
        self.0.iter()
    }
}

/// Number of [`Datum`] stored in a block's [`Data`].
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct DataLen(u32);

impl DataLen {
    pub fn new(len: u32) -> DataLen {
        Self(len)
    }

    pub const fn int(&self) -> u32 {
        self.0
    }
}

/// Single datum stored in a block's [`Data`].
#[derive(Clone, PartialEq, Eq, Hash, BorshDeserialize, BorshSerialize)]
pub struct Datum(Vec<u8>);

impl Datum {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub const fn bytes(&self) -> &Vec<u8> {
        &self.0
    }
}

/// Power of a validator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, BorshDeserialize, BorshSerialize)]
pub struct Power(u64);

impl Power {
    pub fn new(int: u64) -> Self {
        Self(int)
    }

    pub const fn int(&self) -> u64 {
        self.0
    }
}

/// Total power obtained via summing up the [`Power`]s of a set of validators.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, BorshDeserialize, BorshSerialize)]
pub struct TotalPower(u128);

impl TotalPower {
    pub(crate) fn new(int: u128) -> Self {
        Self(int)
    }

    pub const fn int(&self) -> u128 {
        self.0
    }
}

impl AddAssign<Power> for TotalPower {
    fn add_assign(&mut self, rhs: Power) {
        self.0.add_assign(rhs.0 as u128)
    }
}

/// Signature represented in bytes.
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct SignatureBytes([u8; 64]);

impl SignatureBytes {
    pub(crate) fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(&self) -> [u8; 64] {
        self.0
    }
}

/// Set of signatures, represented as a vector with the size of a given validator set.
/// The value at a particular position is either:
/// 1. None: if the a valid signature from the validator at the given position has not been obtained, or
/// 2. Some(signature_bytes): if signature_bytes has been obtained from the validator at the given position.
#[derive(Clone, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct SignatureSet(Vec<Option<SignatureBytes>>);

impl SignatureSet {
    pub const fn init() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn new(len: usize) -> Self {
        Self(vec![None; len])
    }

    pub const fn vec(&self) -> &Vec<Option<SignatureBytes>> {
        &self.0
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Option<SignatureBytes>> {
        self.0.iter()
    }

    pub fn get(&self, pos: usize) -> &Option<SignatureBytes> {
        &self.0[pos]
    }

    pub(crate) fn set(&mut self, pos: usize, value: Option<SignatureBytes>) {
        let signature_vec: &mut Vec<Option<SignatureBytes>> = self.0.as_mut();
        signature_vec[pos] = value
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// HotStuff view number.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshDeserialize, BorshSerialize,
)]
pub struct ViewNumber(u64);

impl ViewNumber {
    pub fn new(int: u64) -> Self {
        Self(int)
    }

    pub const fn init() -> Self {
        Self(0)
    }

    pub const fn int(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for ViewNumber {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl Add<u64> for ViewNumber {
    type Output = ViewNumber;

    fn add(self, rhs: u64) -> Self::Output {
        ViewNumber(self.0.add(rhs))
    }
}

impl Sub<u64> for ViewNumber {
    type Output = ViewNumber;

    fn sub(self, rhs: u64) -> Self::Output {
        ViewNumber(self.0.sub(rhs))
    }
}

impl Sub<ViewNumber> for ViewNumber {
    type Output = i64;

    fn sub(self, rhs: ViewNumber) -> Self::Output {
        (self.0 as i64).sub(rhs.0 as i64)
    }
}

/// How many views are in a [Pacemaker][crate::pacemaker] epoch.
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct EpochLength(u32);

impl EpochLength {
    pub fn new(int: u32) -> Self {
        Self(int)
    }

    pub const fn int(&self) -> u32 {
        self.0
    }
}

/// Size of a buffer (in bytes).
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct BufferSize(u64);

impl BufferSize {
    pub fn new(int: u64) -> Self {
        Self(int)
    }

    pub const fn int(&self) -> u64 {
        self.0
    }
}

impl AddAssign<u64> for BufferSize {
    fn add_assign(&mut self, rhs: u64) {
        self.0.add_assign(rhs)
    }
}

impl SubAssign<u64> for BufferSize {
    fn sub_assign(&mut self, rhs: u64) {
        self.0.sub_assign(rhs)
    }
}

/// Stores the updates associated with committing a given block.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct UpdateSet<K: Eq + Hash, V: Eq + Hash> {
    pub inserts: HashMap<K, V>,
    pub deletes: HashSet<K>,
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

    /// Get an iterator over all of the key-value pairs inserted by this [UpdateSet].
    pub fn inserts(&self) -> hash_map::Iter<K, V> {
        self.inserts.iter()
    }

    /// Get an iterator over all of the keys that are deleted by this [UpdateSet].
    pub fn deletions(&self) -> hash_set::Iter<K> {
        self.deletes.iter()
    }
}

pub type AppStateUpdates = UpdateSet<Vec<u8>, Vec<u8>>;
