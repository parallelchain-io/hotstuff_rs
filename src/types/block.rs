/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! `Block` type and its methods.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    block_tree::{
        accessors::internal::{BlockTreeError, BlockTreeSingleton},
        pluggables::KVStore,
    },
    hotstuff::types::PhaseCertificate,
    types::{
        crypto_primitives::{CryptoHasher, Digest},
        data_types::*,
    },
};

use super::signed_messages::Certificate;

/// Cryptographically hashed, `justify`-linked payload that mutates the
/// [app state and validator set](crate::app#two-app-mutable-states-app-state-and-validator-set) when
/// committed.
///
/// # Permissible variants of `justify.phase`s
///
/// `block.justify.phase` must be `Generic` or `Decide`. This invariant is enforced in two places:
/// 1. When a validator creates a `Block` using [`new`](Self::new).
/// 2. When a replica receives a `Proposal` containing `block` and checks the
///    [`safe_block`](crate::block_tree::invariants::safe_block) predicate.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct Block {
    pub height: BlockHeight,

    /// Library-computed cryptographic hash over `(height, justify, data_hash)`.
    pub hash: CryptoHash,

    /// `PhaseCertificate` linking this block with its parent block.
    pub justify: PhaseCertificate,

    /// `App`-provided cryptographic hash over `data`.
    pub data_hash: CryptoHash,
    pub data: Data,
}

impl Block {
    /// Create a new block with the specified `height`, `justify`, `data_hash`, and `data`, computing
    /// [`hash`](Self::hash) automatically.
    ///
    /// # Panics
    ///
    /// `justify.phase` must be `Generic` or `Decide`. This function panics otherwise.
    pub fn new(
        height: BlockHeight,
        justify: PhaseCertificate,
        data_hash: CryptoHash,
        data: Data,
    ) -> Block {
        assert!(justify.phase.is_generic() || justify.phase.is_decide());

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
        justify: &PhaseCertificate,
        data_hash: &CryptoHash,
    ) -> CryptoHash {
        let mut hasher = CryptoHasher::new();
        hasher.update(&height.try_to_vec().unwrap());
        hasher.update(&justify.try_to_vec().unwrap());
        hasher.update(&data_hash.try_to_vec().unwrap());
        CryptoHash::new(hasher.finalize().into())
    }

    /// Checks if hash and justify are cryptographically correct.
    pub fn is_correct<K: KVStore>(
        &self,
        block_tree: &BlockTreeSingleton<K>,
    ) -> Result<bool, BlockTreeError> {
        Ok(
            self.hash == Block::hash(self.height, &self.justify, &self.data_hash)
                && self.justify.is_correct(block_tree)?,
        )
    }
}
