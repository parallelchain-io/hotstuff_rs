use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    block_sync::messages::{
        BlockSyncAdvertiseMessage, BlockSyncMessage, BlockSyncRequest, BlockSyncResponse,
    },
    hotstuff::messages::HotStuffMessage,
    pacemaker::messages::PacemakerMessage,
    types::basic::{ChainID, ViewNumber},
};

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum Message {
    ProgressMessage(ProgressMessage),
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

/// A message that serves to advance the consensus process, which may involve:
/// 1. Participating in consesus via a [`HotStuffMessage`],
/// 2. Syncing views with other replicas via a [`PacemakerMessage`] (required for consensus),
/// 3. Triggering block sync on seeing a [`BlockSyncAdvertiseMessage`], which indicates that
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
