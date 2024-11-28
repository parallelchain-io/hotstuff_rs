/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Types that exist only to store bytes, and do not have any major "active" behavior.

use std::{
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
    ops::{Add, AddAssign, Sub, SubAssign},
};

use borsh::{BorshDeserialize, BorshSerialize};

/// Number that uniquely identifies a blockchain.
///
/// Every block in the same block tree should share the same `ChainID`, which in turn should be unique
/// between different block trees. All replicas that replicate the same block tree should be configured
/// to use the same `ChainID`.
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct ChainID(u64);

impl ChainID {
    /// Create a new `ChainID` with an `int` value.
    pub const fn new(int: u64) -> Self {
        Self(int)
    }

    /// Get the `u64` value of this `ChainID`.
    pub const fn int(&self) -> u64 {
        self.0
    }
}

/// Height of a block in the block tree.
///
/// Starts at 0 for Genesis Blocks (blocks that contain the
/// [`genesis_pc`](crate::hotstuff::types::PhaseCertificate::genesis_pc)), and increases by 1 for every
/// subsequent "level" of blocks connected by [`justify`](crate::types::block::Block::justify) links.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, BorshDeserialize, BorshSerialize)]
pub struct BlockHeight(u64);

impl BlockHeight {
    /// Create a new `BlockHeight` with an `int` inner value.
    pub fn new(int: u64) -> Self {
        Self(int)
    }

    /// Get the inner `u64` value of this `BlockHeight`.
    pub const fn int(&self) -> u64 {
        self.0
    }

    /// Get the little-endian representation of the inner `u64` value of this `BlockHeight`.
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

/// List of children of a particular block.
///
/// The "children" of a `block` is the set of blocks that directly extend `block` through
/// [`justify`](crate::types::block::Block::justify) links.
///
/// Instances of this type are stored in the block tree's
/// ["Block to Children"](crate::block_tree::variables#blocks) state variable.
///
/// # Uniqueness
///
/// Currently, `ChildrenList` does not enforce uniqueness. This means, for example, that
/// [`push`](Self::push)-ing the same `CryptoHash` twice into a `ChildrenList` will result in that hash
/// appearing twice in the `ChildrenList`.
#[derive(Clone, PartialEq, Eq, BorshDeserialize, BorshSerialize, Default)]
pub struct ChildrenList(Vec<CryptoHash>);

impl ChildrenList {
    /// Create a new `ChildrenList` wrapping around `blocks`.
    pub(crate) fn new(blocks: Vec<CryptoHash>) -> Self {
        Self(blocks)
    }

    /// Get a reference to the inner `Vec<CryptoHash>` value of this `ChildrenList`.
    pub const fn vec(&self) -> &Vec<CryptoHash> {
        &self.0
    }

    /// Iterate through the hashes of the blocks in this `ChildrenList`.
    pub fn iter(&self) -> std::slice::Iter<'_, CryptoHash> {
        self.0.iter()
    }

    /// Add `hash` to this `ChildrenList`.
    pub(crate) fn push(&mut self, hash: CryptoHash) {
        self.0.push(hash)
    }
}

/// 32-byte cryptographic hash.
///
/// # Choice of cryptographic hash function
///
/// The type signature of this type allows instances of `CryptoHash` to be produced by any cryptographic
/// hash function with a 32-byte output. However, within HotStuff-rs, `CryptoHash`-es are only encountered
/// in two contexts, both of them inside blocks:
/// 1. [`data_hash`](super::block::Block::data_hash): this `CryptoHash` can be any 32-byte cryptographic
///    hash.
/// 2. [`hash`](super::block::Block#structfield.hash): this `CryptoHash` is always a SHA256 hash.
#[derive(Clone, Copy, PartialEq, Eq, Hash, BorshDeserialize, BorshSerialize)]
pub struct CryptoHash([u8; 32]);

impl CryptoHash {
    /// Create a new `CryptoHash` wrapping `bytes`.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the inner `[u8; 32]` value of this `CryptoHash`.
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

/// Ed25519 digital signature.
///
/// Within HotStuff-rs, these are produced using the [`ed25519_dalek`] crate, whose main definitions
/// are re-exported from the [`crypto_primitives`](super::crypto_primitives) module.
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct SignatureBytes([u8; 64]);

impl SignatureBytes {
    /// Create a new `SignatureBytes` wrapping `bytes`.
    pub(crate) fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Get the inner `[u8; 64]` value of this `CryptoHash`.
    pub const fn bytes(&self) -> [u8; 64] {
        self.0
    }
}

/// Arbitrary, indexable data provided by an [`App`](crate::app::App) to HotStuff-rs to be stored
/// in a [`Block`](super::block::Block).
///
/// # Querying only a part of `Data`
///
/// Library users can choose between two methods of
/// [`BlockTreeSnapshot`](crate::block_tree::accessors::public::BlockTreeSnapshot) to get a `Block`'s
/// `Data`:
/// 1. [`block_data`](crate::block_tree::accessors::public::BlockTreeSnapshot::block_data) gets the whole of
///    `block.data` in a single call.
/// 2. [`block_datum`](crate::block_tree::accessors::public::BlockTreeSnapshot::block_datum) gets only
///    `block.datum.vec()[datum_index]` in a single call.
///
/// The first method is simple, but may be overkill and cause unnecessary I/O operations if a use case
/// only requires getting a small part of `block.data`. In contrast, the second method allows users to
/// get only the parts of `block.data` that a use case needs.
#[derive(Clone, PartialEq, Eq, Hash, BorshDeserialize, BorshSerialize)]
pub struct Data(Vec<Datum>);

impl Data {
    /// Create a new `Data` wrapping `datum_vec`.
    pub fn new(datum_vec: Vec<Datum>) -> Self {
        Self(datum_vec)
    }

    /// Get a reference to the inner `Vec<Datum>` of this `Data`.
    pub const fn vec(&self) -> &Vec<Datum> {
        &self.0
    }

    /// Get how many `Datum`s are in this `Data`.
    pub fn len(&self) -> DataLen {
        DataLen::new(self.0.len() as u32)
    }

    /// Iterate through the `Datum`s that are in this `Data` in the order they were provided in to
    /// [`new`](Self::new).
    pub fn iter(&self) -> std::slice::Iter<'_, Datum> {
        self.0.iter()
    }
}

/// Number of [`Datum`] stored in a block's [`Data`].
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct DataLen(u32);

impl DataLen {
    /// Create a new `DataLen` wrapping `len`.
    pub fn new(len: u32) -> DataLen {
        Self(len)
    }

    /// Get the inner `u32` of this `DataLen`.
    pub const fn int(&self) -> u32 {
        self.0
    }
}

/// Unit of [`Data`] that can be queried individually from the [block tree](crate::block_tree).
///
/// Read [Querying only a part of `Data`](Data#querying-only-a-part-of-data) for the rationale behind
/// why `Data` is split into `Datum`s.
#[derive(Clone, PartialEq, Eq, Hash, BorshDeserialize, BorshSerialize)]
pub struct Datum(Vec<u8>);

impl Datum {
    /// Create a new `Datum` wrapping `bytes`.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Get a reference to the inner `Vec<u8>` of this `Datum`.
    pub const fn bytes(&self) -> &Vec<u8> {
        &self.0
    }
}

/// Weight of a specific validator's votes in consensus decisions.
///
/// The higher the power, the more weight the validator's votes have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, BorshDeserialize, BorshSerialize)]
pub struct Power(u64);

impl Power {
    /// Create a new `Power` wrapping `int`.
    pub fn new(int: u64) -> Self {
        Self(int)
    }

    /// Get the inner `u64` value of this `Power`.
    pub const fn int(&self) -> u64 {
        self.0
    }
}

/// Sum of the [`Power`]s of all validators in a [`ValidatorSet`](super::validator_set::ValidatorSet).
///
/// The inner type that this newtype wraps around is `u128`, which is bigger than inner `u64` that
/// `Power` wraps around. This is so that summing up large `Power`s do not cause `TotalPower`'s inner
/// value to overflow.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, BorshDeserialize, BorshSerialize)]
pub struct TotalPower(u128);

impl TotalPower {
    /// Create a new `TotalPower` wrapping `int`.
    pub(crate) fn new(int: u128) -> Self {
        Self(int)
    }

    /// Get the inner `u128` value of this `TotalPower`.
    pub const fn int(&self) -> u128 {
        self.0
    }
}

impl AddAssign<Power> for TotalPower {
    fn add_assign(&mut self, rhs: Power) {
        self.0.add_assign(rhs.0 as u128)
    }
}

/// An ordered list of [`SignatureBytes`] from the same
/// [`ValidatorSet`](super::validator_set::ValidatorSet).
///
/// # Ordering
///
/// `SignatureBytes` should appear in `SignatureSet` in an order that corresponds to the `ValidatorSet`
/// from which the signatures come from.
///
/// Specifically, this means that if `signature_bytes` was created by a `validator` in a
/// `validator_set`, then it should appear in `SignatureSet`s corresponding to `validator_set` in the
/// [`self.validator_set.position(validator)`](super::validator_set::ValidatorSet::position) position.
///
/// Users of this type are responsible for enforcing this order **manually**, in particular, every
/// time they call [`set`](Self::set) to insert a signature into `SignatureSet`. Failing to uphold this
/// order may cause the [`Certificate`](super::signed_messages::Certificate) that holds this
/// `SignatureSet` to fail validation and be ignored by other replicas.
///
/// # Optionality
///
/// A `SignatureSet` created using [`new`](Self::new) initially contains `vec![None; len]`. As `set` is
/// called, these `None`s will be replaced with `Some(signature_bytes)`.
///
/// This means that the value at any particular position on the `SignatureSet` is either:
/// 1. `None`: if the a valid signature from the validator at the given position has not been obtained, or
/// 2. `Some(signature_bytes)`: if signature_bytes has been obtained from the validator at the given position.
#[derive(Clone, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct SignatureSet(Vec<Option<SignatureBytes>>);

impl SignatureSet {
    /// Create the `SignatureSet` that forms a part of the
    /// [`genesis_pc`](crate::hotstuff::types::PhaseCertificate::genesis_pc).
    ///
    /// The `SignatureSet` contains an empty `Vec` (of length 0).
    pub const fn genesis() -> Self {
        Self(Vec::new())
    }

    /// Create a new `SignatureSet` initially containing `len` `None`s.
    pub(crate) fn new(len: usize) -> Self {
        Self(vec![None; len])
    }

    /// Get a reference to the inner `Vec<Option<SignatureBytes>>` of this `SignatureSet`.
    pub const fn vec(&self) -> &Vec<Option<SignatureBytes>> {
        &self.0
    }

    /// Get an iterator over the `Option<SignatureBytes>`s in this `SignatureSet`.
    pub fn iter(&self) -> std::slice::Iter<'_, Option<SignatureBytes>> {
        self.0.iter()
    }

    /// Get a reference to the `Option<SignatureBytes>` at position `pos` inside this `SignatureSet`.
    pub fn get(&self, pos: usize) -> &Option<SignatureBytes> {
        &self.0[pos]
    }

    /// Set the value at `pos` in this `SignatureSet` to be `signature`.
    ///
    /// # Panics
    ///
    /// Panics if `pos` is `>=` [`len`](Self::len).
    pub(crate) fn set(&mut self, pos: usize, signature: Option<SignatureBytes>) {
        let signature_vec: &mut Vec<Option<SignatureBytes>> = self.0.as_mut();
        signature_vec[pos] = signature
    }

    /// Get the length of the inner `Vec<Option<SignatureBytes>>` of this `SignatureSet`.
    ///
    /// Note that because the inner vector contains `Option`s, this will most often not correspond exactly
    /// to how many signatures are in this `SignatureSet`. However, it will always correspond to exactly
    /// how many validators are in the `ValidatorSet` that corresponds to this `SignatureSet`.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// View number. Starts at 0 and increases by 1 every time the [Pacemaker](crate::pacemaker) times out.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, BorshDeserialize, BorshSerialize,
)]
pub struct ViewNumber(u64);

impl ViewNumber {
    /// Create a new `ViewNumber` wrapping `int`.
    pub fn new(int: u64) -> Self {
        Self(int)
    }

    /// Get the initial `ViewNumber`, which is 0.
    pub const fn init() -> Self {
        Self(0)
    }

    /// Get the inner `u64` of this `ViewNumber`.
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

/// Configurable number of views in a [Pacemaker](crate::pacemaker) epoch.
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct EpochLength(u32);

impl EpochLength {
    /// Create a new `EpochLength` wrapping `int`.
    pub fn new(int: u32) -> Self {
        Self(int)
    }

    /// Get the inner `u32` value of this `EpochLength`.
    pub const fn int(&self) -> u32 {
        self.0
    }
}

/// Size of a buffer (in bytes).
#[derive(Clone, Copy, PartialEq, Eq, BorshDeserialize, BorshSerialize)]
pub struct BufferSize(u64);

impl BufferSize {
    /// Create a new `BufferSize` wrapping `int`.
    pub fn new(int: u64) -> Self {
        Self(int)
    }

    /// Get the inner `u64` value of this `BufferSize`.
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
