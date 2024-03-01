/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definitions for structured messages that are sent between replicas.
//!
//! This includes messages [used in the progress protocol](ProgressMessage), and those [used in the sync protocol](SyncMessage).

use borsh::{BorshDeserialize, BorshSerialize};

use crate::block_sync::messages::{AdvertiseBlock, BlockSyncMessage, BlockSyncRequest, BlockSyncResponse, BlockSyncTriggerMessage};
use crate::hotstuff::messages::HotStuffMessage;
use crate::pacemaker::messages::PacemakerMessage;

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

// This trait generalizes message types that have their own [Sender].
pub trait Sendable {}

impl Sendable for PacemakerMessage {}
impl Sendable for HotStuffMessage {}
impl Sendable for BlockSyncRequest {}
impl Sendable for BlockSyncResponse {}
impl Sendable for AdvertiseBlock {}
