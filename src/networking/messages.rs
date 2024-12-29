//! Exhaustive enumerations around every message variant used in HotStuff-rs.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    block_sync::messages::{
        BlockSyncAdvertiseMessage, BlockSyncMessage, BlockSyncRequest, BlockSyncResponse,
    },
    hotstuff::messages::HotStuffMessage,
    pacemaker::messages::PacemakerMessage,
    types::data_types::{ChainID, ViewNumber},
};

/// All message variants used in HotStuff-rs.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum Message {
    /// See: [`ProgressMessage`].
    ProgressMessage(ProgressMessage),

    /// See: [`BlockSyncMessage`].
    BlockSyncMessage(BlockSyncMessage),
}

impl From<HotStuffMessage> for Message {
    fn from(value: HotStuffMessage) -> Self {
        Message::ProgressMessage(ProgressMessage::HotStuffMessage(value))
    }
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
        Message::ProgressMessage(ProgressMessage::BlockSyncAdvertiseMessage(value))
    }
}

/// Message variants sent or received by the [`algorithm`](crate::algorithm) thread.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum ProgressMessage {
    /// See [`HotStuffMessage`].
    HotStuffMessage(HotStuffMessage),

    /// See [`PacemakerMessage`].
    PacemakerMessage(PacemakerMessage),

    /// See [`BlockSyncAdvertiseMessage`],
    BlockSyncAdvertiseMessage(BlockSyncAdvertiseMessage),
}

impl ProgressMessage {
    /// Get the `chain_id` field of the inner message.
    pub fn chain_id(&self) -> ChainID {
        match self {
            ProgressMessage::HotStuffMessage(msg) => msg.chain_id(),
            ProgressMessage::PacemakerMessage(msg) => msg.chain_id(),
            ProgressMessage::BlockSyncAdvertiseMessage(msg) => msg.chain_id(),
        }
    }

    /// Get the `view` field of the inner message.
    pub fn view(&self) -> Option<ViewNumber> {
        match self {
            ProgressMessage::HotStuffMessage(msg) => Some(msg.view()),
            ProgressMessage::PacemakerMessage(msg) => Some(msg.view()),
            ProgressMessage::BlockSyncAdvertiseMessage(_) => None,
        }
    }

    /// Get the size of the inner message.
    pub fn size(&self) -> u64 {
        match self {
            ProgressMessage::HotStuffMessage(msg) => msg.size(),
            ProgressMessage::PacemakerMessage(msg) => msg.size(),
            ProgressMessage::BlockSyncAdvertiseMessage(msg) => msg.size(),
        }
    }

    /// Check whether the inner message is a [`BlockSyncAdvertiseMessage`].
    pub fn is_block_sync_trigger_msg(&self) -> bool {
        match self {
            ProgressMessage::BlockSyncAdvertiseMessage(_) => true,
            _ => false,
        }
    }
}
