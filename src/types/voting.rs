/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for the generic [Vote] and [VoteCollector] traits.
//! Implementations used by the Pacemaker and HotStuff protocols can be found in
//! the respective modules.

pub use ed25519_dalek::{SigningKey, VerifyingKey, Signature};
pub use sha2::Sha256 as CryptoHasher;

use super::{
    basic::{ChainID, SignatureBytes, ViewNumber}, 
    certificates::Certificate,
    validators::ValidatorSet
};

pub trait Vote {
    
    // Converts relevant values contained in the vote into a message (vector of bytes) that should be signed.
    // A vote should have a vector of bytes to sign over.
    fn to_message_bytes(&self) -> Option<Vec<u8>>;

    // Obtains the signature from the vote.
    // A vote must contain a signature.
    fn signature(&self) -> SignatureBytes;

    // Verifies the correctness of the signature given the values that should be signed.
    fn is_correct(&self, pk:&VerifyingKey) -> bool {
        let signature = Signature::from_bytes(&self.signature());
        pk.verify(
        self.to_message_bytes(),
            &signature,
        )
        .is_ok()
    }

}

pub trait VoteCollector<V: Vote, C: Certificate> {

    fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> Self;

    fn collect(&mut self, signer: &VerifyingKey, vote: V) -> Option<C>;
}
