/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for the 'block' type and its associated methods.

use borsh::{BorshDeserialize, BorshSerialize};
pub use sha2::Sha256 as CryptoHasher;
use sha2::Digest;

use crate::types::basic::*;
use crate::types::certificates::{Certificate, QuorumCertificate};
use crate::types::validators::ValidatorSet;


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
