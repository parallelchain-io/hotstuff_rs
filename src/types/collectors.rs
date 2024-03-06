/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for the generic [SignedMessage] and [Collector] traits.
//! Implementations used by the [Pacemaker][crate::pacemaker::types] and [HotStuff][crate::hotstuff::types] protocols 
//! can be found in the respective modules, since the values of the corresponding types are not
//! stored in persistent memory, i.e., the [block tree][crate::state::BlockTree].

use ed25519_dalek::Verifier;
pub use ed25519_dalek::{SigningKey, VerifyingKey, Signature};
pub use sha2::Sha256 as CryptoHasher;

use super::{
    basic::{ChainID, SignatureBytes, ViewNumber}, 
    certificates::Certificate,
    validators::ValidatorSet
};

/// A signed message must consist of:
/// 1. Message bytes [SignedMessage::message_bytes]: the values that the signature is over, and
/// 2. Signature bytes [SignedMessage::signature_bytes]: the signature in bytes.
/// Given the two values satisfying the above, and a public key of the signer, 
/// the signature can be verified against the message.
pub(crate) trait SignedMessage {
    
    // The values contained in the message that should be signed (represented as a vector of bytes).
    // A signed message should have a vector of bytes to sign over.
    fn message_bytes(&self) -> Vec<u8>;

    // The signature (in bytes) from the vote.
    // A vote must contain a signature.
    fn signature_bytes(&self) -> SignatureBytes;

    // Verifies the correctness of the signature given the values that should be signed.
    fn is_correct(&self, pk:&VerifyingKey) -> bool {
        let signature = Signature::from_bytes(&self.signature_bytes());
        pk.verify(
        &self.message_bytes(),
            &signature,
        )
        .is_ok()
    }

}

/// Collects [correct][SignedMessage::is_correct] [signed messages][SignedMessage] into a [Certificate].
pub(crate) trait Collector<S: SignedMessage, C: Certificate> {

    fn new(chain_id: ChainID, view: ViewNumber, validator_set: ValidatorSet) -> Self;

    fn collect(&mut self, signer: &VerifyingKey, message: S) -> Option<C>;
}
