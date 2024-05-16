/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas.
//!
//! This includes messages [used in the progress protocol](ProgressMessage), and those 
//! [used in the block sync protocol](BlockSyncMessage).

use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Signature, VerifyingKey, Verifier};

use crate::block_sync::messages::{AdvertiseBlock, BlockSyncMessage, BlockSyncRequest, BlockSyncResponse, BlockSyncAdvertiseMessage};
use crate::hotstuff::messages::HotStuffMessage;
use crate::pacemaker::messages::PacemakerMessage;
use crate::types::basic::{ChainID, SignatureBytes, ViewNumber};

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum Message {
    ProgressMessage(ProgressMessage),
    BlockSyncMessage(BlockSyncMessage),
}

/// A message that serves to advance the consensus process, which may involve:
/// 1. Participating in consesus via a [HotStuffMessage],
/// 2. Syncing views with other replicas via a [PacemakerMessage] (required for consensus),
/// 3. Triggering block sync on seeing a [BlockSyncTriggerMessage], which indicates that
///    the replica is lagging behind the others (required for consensus).
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum ProgressMessage {
    HotStuffMessage(HotStuffMessage),
    PacemakerMessage(PacemakerMessage),
    BlockSyncAdvertiseMessage(BlockSyncAdvertiseMessage),
}

impl ProgressMessage {
    pub fn chain_id(&self) -> ChainID {
        match self {
            ProgressMessage::HotStuffMessage(msg) => msg.chain_id(),
            ProgressMessage::PacemakerMessage(msg) => msg.chain_id(),
            ProgressMessage::BlockSyncAdvertiseMessage(msg) => msg.chain_id(),
        }
    }

    pub fn view(&self) -> Option<ViewNumber> {
        match self {
            ProgressMessage::HotStuffMessage(msg) => Some(msg.view()),
            ProgressMessage::PacemakerMessage(msg) => Some(msg.view()),
            ProgressMessage::BlockSyncAdvertiseMessage(_) => None,
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            ProgressMessage::HotStuffMessage(msg) => msg.size(),
            ProgressMessage::PacemakerMessage(msg) => msg.size(),
            ProgressMessage::BlockSyncAdvertiseMessage(msg) => msg.size(),
        }
    }

    pub fn is_block_sync_trigger_msg(&self) -> bool {
        match self {
            ProgressMessage::BlockSyncAdvertiseMessage(_) => true,
            _ => false,
        }
    }
}

/// A signed message must consist of:
/// 1. Message bytes [SignedMessage::message_bytes]: the values that the signature is over, and
/// 2. Signature bytes [SignedMessage::signature_bytes]: the signature in bytes.
/// Given the two values satisfying the above, and a public key of the signer, 
/// the signature can be verified against the message.
pub(crate) trait SignedMessage: Clone {
    
    // The values contained in the message that should be signed (represented as a vector of bytes).
    // A signed message must have a vector of bytes to sign over.
    fn message_bytes(&self) -> Vec<u8>;

    // The signature (in bytes) from the vote.
    // A vote must contain a signature.
    fn signature_bytes(&self) -> SignatureBytes;

    // Verifies the correctness of the signature given the values that should be signed.
    fn is_correct(&self, pk:&VerifyingKey) -> bool {
        let signature = Signature::from_bytes(&self.signature_bytes().bytes());
        pk.verify(
            &self.message_bytes(),
            &signature,
        )
        .is_ok()
    }
}

/// A cacheable message can be inserted into the 
/// [progress message buffer](crate::networking::ProgressMessageStub).
/// 
/// For this, we require that:
/// 1. The message is associated with a view,
/// 2. The message size is statically known and depends on a particular enum variant.
pub(crate) trait Cacheable {
    fn view(&self) -> ViewNumber;

    fn size(&self) -> u64;
}

impl From<PacemakerMessage> for Message {
    fn from(value: PacemakerMessage) -> Self {
        Message::ProgressMessage(ProgressMessage::PacemakerMessage(value))
    }
}

impl From<BlockSyncRequest> for Message {
    fn from(value: BlockSyncRequest) -> Self {
        Message::BlockSyncMessage(BlockSyncMessage::BlockSyncRequest(value))
    }
}

impl From<BlockSyncResponse> for Message {
    fn from(value: BlockSyncResponse) -> Self {
        Message::BlockSyncMessage(BlockSyncMessage::BlockSyncResponse(value))
    }
}

impl From<BlockSyncAdvertiseMessage> for Message {
    fn from(value: BlockSyncAdvertiseMessage) -> Self {
        Message::ProgressMessage(ProgressMessage::
            BlockSyncAdvertiseMessage(
                value
            )
        )
    }
}

impl From<HotStuffMessage> for Message {
    fn from(value: HotStuffMessage) -> Self {
        Message::ProgressMessage(ProgressMessage::HotStuffMessage(value))
    }
}
