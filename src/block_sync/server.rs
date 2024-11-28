/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implements the [`BlockSyncServer`] for the block sync protocol, which helps the replicas lagging
//! behind catch up with the head of the blockchain in a safe and live manner.
//!
//! A replica might be lagging behind for various reasons, such as network outage, downtime, or
//! deliberate action by Byzantine leaders.
//!
//! The server's responsibility as part of the protocol is to:
//! 1. Respond to block sync requests ("block sync process" part of the protocol).
//! 2. Periodically broadcast advertisements which serve to notify other replicas about the server's
//!    availability and view of the blockchain ("block sync trigger" and "block sync server selection").
//!
//! ## Block sync process
//!
//! As part of the sync process, the server responds to any received sync requests with blocks starting
//! from a given start height. The number of blocks sent back in a response is limited by a configurable
//! limit.
//!
//! The client side of this protocol is explained [here](crate::block_sync::client).
//!
//! ## Block sync trigger and block sync server selection
//!
//! Maintaining a sync server that periodically notifies other replicas about its state of the block
//! tree and its availability plays an important role in ensuring that lagging replicas try to sync
//! when there is evidence that they are lagging behind, and that they sync with a peer who is ahead
//! of them.
//!
//! A block sync server notifies others about its availability and state of the block tree by
//! broadcasting [`BlockSyncAdvertiseMessage`]s.

use std::{
    cmp::max,
    sync::mpsc::{Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

use ed25519_dalek::VerifyingKey;

use crate::{
    block_tree::{accessors::public::BlockTreeCamera, pluggables::KVStore},
    events::{Event, ReceiveSyncRequestEvent, SendSyncResponseEvent},
    networking::{network::Network, receiving::BlockSyncServerStub, sending::SenderHandle},
    types::{
        crypto_primitives::Keypair,
        data_types::{BlockHeight, ChainID},
    },
};

use super::messages::{BlockSyncAdvertiseMessage, BlockSyncRequest, BlockSyncResponse};

pub struct BlockSyncServer<N: Network + 'static, K: KVStore> {
    config: BlockSyncServerConfiguration,
    block_tree_camera: BlockTreeCamera<K>,
    last_advertisement: Instant,
    receiver: BlockSyncServerStub,
    sender: SenderHandle<N>,
    shutdown_signal: Receiver<()>,
    event_publisher: Option<Sender<Event>>,
}

impl<N: Network + 'static, K: KVStore> BlockSyncServer<N, K> {
    pub(crate) fn new(
        config: BlockSyncServerConfiguration,
        block_tree_camera: BlockTreeCamera<K>,
        requests: Receiver<(VerifyingKey, BlockSyncRequest)>,
        network: N,
        shutdown_signal: Receiver<()>,
        event_publisher: Option<Sender<Event>>,
    ) -> Self {
        Self {
            config,
            block_tree_camera,
            last_advertisement: Instant::now(),
            receiver: BlockSyncServerStub::new(requests),
            sender: SenderHandle::new(network),
            shutdown_signal,
            event_publisher,
        }
    }

    pub(crate) fn start(mut self) -> JoinHandle<()> {
        thread::spawn(move || loop {
            match self.shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => {
                    unreachable!("The Block Sync Server's `shutdown_signal` channel no longer has any senders connected to it")
                }
            }

            // 1. Respond to received block sync requests.
            if let Ok((
                origin,
                BlockSyncRequest {
                    start_height,
                    limit,
                    chain_id,
                },
            )) = self.receiver.recv_request()
            {
                // Ignore requests whose chain ID does not match ours.
                if chain_id != self.config.chain_id {
                    continue;
                }

                Event::ReceiveSyncRequest(ReceiveSyncRequestEvent {
                    timestamp: SystemTime::now(),
                    peer: origin,
                    start_height,
                    limit,
                })
                .publish(&self.event_publisher);

                // Get, from the block tree, the blocks and the PC that will be used to form the Block Sync Response.
                let bt_snapshot = self.block_tree_camera.snapshot();
                let blocks_res = bt_snapshot.blocks_from_height_to_newest(
                    start_height,
                    max(limit, self.config.request_limit),
                );
                let highest_pc_res = bt_snapshot.highest_pc();

                match (blocks_res, highest_pc_res) {
                    (Ok(blocks), Ok(highest_pc)) => {
                        // If there are blocks and a highest PC to return, send a Block Sync Response.
                        self.sender.send(
                            origin,
                            BlockSyncResponse {
                                blocks: blocks.clone(),
                                highest_pc: highest_pc.clone(),
                            },
                        );

                        Event::SendSyncResponse(SendSyncResponseEvent {
                            timestamp: SystemTime::now(),
                            peer: origin,
                            blocks,
                            highest_pc: highest_pc,
                        })
                        .publish(&self.event_publisher)
                    }
                    _ => {
                        // Otherwise, do not send a response.
                    }
                }
            }

            // 2. If the last advertisement was sent more than `advertise_time` duration ago, broadcast an:
            // - Advertise PC: to let others know about our local Highest PC, which may trigger them to start
            //   syncing if they find that they are behind.
            // - Advertise Block: to let others know about our local Highest Committed Block height, so that they
            //   can decide whether or not we are a suitable sync server for them.
            if Instant::now() - self.last_advertisement >= self.config.advertise_time {
                let highest_pc = self
                    .block_tree_camera
                    .snapshot()
                    .highest_pc()
                    .expect("Could not obtain the highest PC!");

                let highest_committed_block_height = match self
                    .block_tree_camera
                    .snapshot()
                    .highest_committed_block_height()
                    .expect("Could not obtain the highest committed block height!")
                {
                    Some(height) => height,
                    None => BlockHeight::new(0),
                };

                // Broadcast an Advertise PC message.
                let advertise_pc_msg = BlockSyncAdvertiseMessage::advertise_pc(highest_pc);
                self.sender.broadcast(advertise_pc_msg);

                // Broadcast an Advertise Block message.
                let advertise_block_msg = BlockSyncAdvertiseMessage::advertise_block(
                    &self.config.keypair,
                    self.config.chain_id,
                    highest_committed_block_height,
                );
                self.sender.broadcast(advertise_block_msg);

                // Update time of last advertisement.
                self.last_advertisement = Instant::now()
            }

            thread::yield_now();
        })
    }
}

/// Parameters that are used to configure the behaviour of the Block Sync Server. These should not
/// change after the server starts.
pub(crate) struct BlockSyncServerConfiguration {
    /// ID of the blockchain for which the server handles sync requests.
    pub(crate) chain_id: ChainID,

    /// Keypair used to sign Advertise Block messages.
    pub(crate) keypair: Keypair,

    /// Maximum number of blocks that this server can provide in a Block Sync Response.
    pub(crate) request_limit: u32,

    /// How often the sync server should broadcast [`BlockSyncAdvertiseMessage`]s.
    pub(crate) advertise_time: Duration,
}
