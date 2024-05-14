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
    messages::SignedMessage, 
    types::{basic::*,block::*, keypair::Keypair}
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
pub enum BlockSyncAdvertiseMessage {
    AdvertiseBlock(AdvertiseBlock),
    AdvertiseQC(AdvertiseQC),
}

impl BlockSyncAdvertiseMessage {
    pub fn advertise_block(me: &Keypair, chain_id: ChainID, highest_committed_block_height: BlockHeight) -> Self {
        let message = &(chain_id, highest_committed_block_height)
            .try_to_vec()
            .unwrap();
        let signature = me.sign(message);
        BlockSyncAdvertiseMessage::AdvertiseBlock(AdvertiseBlock{chain_id, highest_committed_block_height, signature})
    }

    pub fn advertise_qc(highest_qc: QuorumCertificate) -> Self {
        BlockSyncAdvertiseMessage::AdvertiseQC(AdvertiseQC{highest_qc})
    }

    pub fn chain_id(&self) -> ChainID {
        match self {
            BlockSyncAdvertiseMessage::AdvertiseBlock(msg) => msg.chain_id,
            BlockSyncAdvertiseMessage::AdvertiseQC(msg) => msg.highest_qc.chain_id
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            BlockSyncAdvertiseMessage::AdvertiseBlock(msg) => mem::size_of::<AdvertiseBlock>() as u64,
            BlockSyncAdvertiseMessage::AdvertiseQC(msg) => mem::size_of::<AdvertiseQC>() as u64,
        }
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct AdvertiseBlock {
    pub chain_id: ChainID,
    pub highest_committed_block_height: BlockHeight,
    pub signature: SignatureBytes,
}

impl SignedMessage for AdvertiseBlock {
    fn message_bytes(&self) -> Vec<u8> {
        (self.chain_id, self.highest_committed_block_height)
            .try_to_vec()
            .unwrap()
    }

    fn signature_bytes(&self) -> SignatureBytes {
        self.signature
    }
}

#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct AdvertiseQC {
    pub highest_qc: QuorumCertificate,
}