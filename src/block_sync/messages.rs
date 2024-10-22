/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas as part of the Block Sync
//! protocol.
//!
//! ## Messages
//!
//! The Block Sync protocol defines two categories of messages:
//!
//! 1. Block Sync Protocol messages ([`BlockSyncMessage`]): exchanged between a sync client and sync
//!    server when the client is trying to sync with the server.
//! 2. Block Sync Server Advertisements ([`BlockSyncAdvertiseMessage`]): periodically broadcasted by
//!    sync servers to update the sync clients on:
//!     1. Their availability and commitment to providing blocks at least up to a given height
//!        ([`AdvertiseBlock`]).
//!     2. Whether the quorum is making progress in a future view, as evidenced by the server's local
//!        Highest PC ([`AdvertisePC`]).

use std::mem;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    hotstuff::types::PhaseCertificate,
    types::{block::*, crypto_primitives::Keypair, data_types::*, signed_messages::SignedMessage},
};

/// Messages exchanged between a sync server and a sync client when syncing.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum BlockSyncMessage {
    BlockSyncRequest(BlockSyncRequest),
    BlockSyncResponse(BlockSyncResponse),
}

impl BlockSyncMessage {
    pub fn block_sync_request(
        chain_id: ChainID,
        start_height: BlockHeight,
        limit: u32,
    ) -> BlockSyncMessage {
        BlockSyncMessage::BlockSyncRequest(BlockSyncRequest {
            chain_id,
            start_height,
            limit,
        })
    }

    pub fn block_sync_response(
        blocks: Vec<Block>,
        highest_pc: PhaseCertificate,
    ) -> BlockSyncMessage {
        BlockSyncMessage::BlockSyncResponse(BlockSyncResponse { blocks, highest_pc })
    }
}

/// Sync request sent by a sync client to a sync server. The request includes:
/// 1. Chain ID: for identifying the blockchain the client is interested in,
/// 2. Start height: the height starting from which the client wants to obtain blocks.
/// 3. Limit: Max. number of blocks that the client wants to obtain in a response.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct BlockSyncRequest {
    pub chain_id: ChainID,
    pub start_height: BlockHeight,
    pub limit: u32,
}

/// Sync response sent by a sync server to a sync client requesting blocks. The response includes:
/// 1. Blocks: entire [`Block`]s that the client can validate and insert into their blockchain,
/// 2. HighestPC: highest-viewed [`PhaseCertificate`] known to the server, which the client can
///    validate and use to find out what is the latest consensus decision is.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct BlockSyncResponse {
    pub blocks: Vec<Block>,
    pub highest_pc: PhaseCertificate,
}

/// Messages periodically broadcasted by the sync server to update clients about their availability,
/// commitment to providing blocks up to a given height, and knowledge of the latest consensus decision.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub enum BlockSyncAdvertiseMessage {
    AdvertiseBlock(AdvertiseBlock),
    AdvertisePC(AdvertisePC),
}

impl BlockSyncAdvertiseMessage {
    pub(crate) fn advertise_block(
        me: &Keypair,
        chain_id: ChainID,
        highest_committed_block_height: BlockHeight,
    ) -> Self {
        let message = &(chain_id, highest_committed_block_height)
            .try_to_vec()
            .unwrap();
        let signature = me.sign(message);
        BlockSyncAdvertiseMessage::AdvertiseBlock(AdvertiseBlock {
            chain_id,
            highest_committed_block_height,
            signature,
        })
    }

    pub(crate) fn advertise_pc(highest_pc: PhaseCertificate) -> Self {
        BlockSyncAdvertiseMessage::AdvertisePC(AdvertisePC { highest_pc })
    }

    pub fn chain_id(&self) -> ChainID {
        match self {
            BlockSyncAdvertiseMessage::AdvertiseBlock(msg) => msg.chain_id,
            BlockSyncAdvertiseMessage::AdvertisePC(msg) => msg.highest_pc.chain_id,
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            BlockSyncAdvertiseMessage::AdvertiseBlock(_) => mem::size_of::<AdvertiseBlock>() as u64,
            BlockSyncAdvertiseMessage::AdvertisePC(_) => mem::size_of::<AdvertisePC>() as u64,
        }
    }
}

/// A message periodically broadcasted by the sync server to:
/// 1. Let clients know that the server is available,
/// 2. Commit to providing blocks at least up to the highest committed block height, as included in this
///    message, in case a client decides to sync with the sync server.
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

/// A message periodically broadcasted by the sync server to let clients know about the Highest
/// `PhaseCertificate` the server knows.
///
/// This information may serve as an evidence for the fact that a client is lagging behind, and thus
/// make the client trigger the sync process with some server.
#[derive(Clone, BorshSerialize, BorshDeserialize)]
pub struct AdvertisePC {
    pub highest_pc: PhaseCertificate,
}
