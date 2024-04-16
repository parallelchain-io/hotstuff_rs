/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas as part of the [BlockSync]
//! protocol.
//! 
//! Note: the struct definitions may be subject to change as we flesh out the details of the 
//! [BlockSync] protocol.

use std::mem;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    hotstuff::types::QuorumCertificate, 
    messages::{Message, ProgressMessage}, 
    types::{basic::*,block::*}
};

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum BlockSyncMessage {
    BlockSyncRequest(BlockSyncRequest),
    BlockSyncResponse(BlockSyncResponse),
}

// Messages exchanged as part of the block sync protocol.
impl BlockSyncMessage {
    pub fn block_sync_request(chain_id: ChainID, start_height: BlockHeight, limit: u32) -> BlockSyncMessage {
        BlockSyncMessage::BlockSyncRequest(BlockSyncRequest{chain_id, start_height, limit})
    }

    pub fn block_sync_response(blocks: Vec<Block>, highest_qc: QuorumCertificate) -> BlockSyncMessage {
        BlockSyncMessage::BlockSyncResponse(BlockSyncResponse{blocks, highest_qc})
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct BlockSyncRequest {
    pub chain_id: ChainID,
    pub start_height: BlockHeight,
    pub limit: u32,
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct BlockSyncResponse {
    pub blocks: Vec<Block>,
    pub highest_qc: QuorumCertificate,
}

// Messages that may trigger sync, exchanged as part of the normal progress protocol.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum BlockSyncTriggerMessage {
    AdvertiseBlock(AdvertiseBlock)
}

impl BlockSyncTriggerMessage {
    pub fn advertise_block(chain_id: ChainID, block: CryptoHash, block_qc: QuorumCertificate) -> Self {
        BlockSyncTriggerMessage::AdvertiseBlock(AdvertiseBlock{chain_id, block, block_qc})
    }

    pub fn chain_id(&self) -> ChainID {
        match self {
            BlockSyncTriggerMessage::AdvertiseBlock(msg) => msg.chain_id
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            BlockSyncTriggerMessage::AdvertiseBlock(msg) => mem::size_of::<AdvertiseBlock>() as u64,
        }
    }

}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct AdvertiseBlock {
    pub chain_id: ChainID,
    pub block: CryptoHash,
    pub block_qc: QuorumCertificate, // highest known qc for the block
    //TODO: pub signature: SignatureBytes, // necessary to authenticate the sender of this message!
}
