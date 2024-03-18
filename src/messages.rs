/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas.
//!
//! This includes messages [used in the progress protocol](ProgressMessage), and those [used in the sync protocol](SyncMessage).

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Signature, VerifyingKey, Verifier};

use crate::block_sync::messages::{BlockSyncMessage, BlockSyncTriggerMessage};
use crate::hotstuff::messages::HotStuffMessage;
use crate::pacemaker::messages::PacemakerMessage;
use crate::types::basic::SignatureBytes;

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum Message {
    ProgressMessage(ProgressMessage),
    BlockSyncMessage(BlockSyncMessage),
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum ProgressMessage {
    HotStuffMessage(HotStuffMessage),
    PacemakerMessage(PacemakerMessage),
    BlockSyncTriggerMessage(BlockSyncTriggerMessage),
}

/// A signed message must consist of:
/// 1. Message bytes [SignedMessage::message_bytes]: the values that the signature is over, and
/// 2. Signature bytes [SignedMessage::signature_bytes]: the signature in bytes.
/// Given the two values satisfying the above, and a public key of the signer, 
/// the signature can be verified against the message.
pub(crate) trait SignedMessage {
    
    // The values contained in the message that should be signed (represented as a vector of bytes).
    // A signed message must have a vector of bytes to sign over.
    fn message_bytes(&self) -> Vec<u8>;

    // The signature (in bytes) from the vote.
    // A vote must contain a signature.
    fn signature_bytes(&self) -> SignatureBytes;

    // Verifies the correctness of the signature given the values that should be signed.
    fn is_correct(&self, pk:&VerifyingKey) -> bool {
        let signature = Signature::from_bytes(&self.signature_bytes().get_bytes());
        pk.verify(
            &self.message_bytes(),
            &signature,
        )
        .is_ok()
    }

}
