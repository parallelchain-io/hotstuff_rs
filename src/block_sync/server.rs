/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implements the [BlockSyncServer] for the block sync protocol, which helps the replicas lagging 
//! behind catch up with the head of the blockchain in a safe and live manner. A replica might be
//! lagging behind for various reasons, such as network outage, downtime, or deliberate action
//! by Byzantine leaders.
//! 
//! The server's responsibility as part of the protocol is to:
//! 1. Respond to block sync requests ("block sync process" part of the protocol),
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
//! A block sync server notifies others about its availability and state of the block tree in two ways:
//! 1. AdvertiseQC: to let others know about the server's highestQC known, which may trigger sync if they are behind,
//! 2. AdvertiseBlock: to let others know about the server's highest committed block height, so that they can decide 
//!    if the server is a suitable sync peer.

use std::{
    cmp::max,
    sync::mpsc::{Receiver, Sender, TryRecvError}, 
    thread::{self, JoinHandle}, 
    time::{Duration, Instant, SystemTime}
};

use ed25519_dalek::VerifyingKey;

use crate::events::{Event, ReceiveSyncRequestEvent, SendSyncResponseEvent};
use crate::state::{block_tree_camera::BlockTreeCamera, kv_store::KVStore};
use crate::types::basic::BlockHeight;
use crate::networking::{BlockSyncServerStub, Network, SenderHandle};
use crate::types::{basic::ChainID, keypair::Keypair};

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
            event_publisher
        }
    }

    pub(crate) fn start(mut self) -> JoinHandle<()> {

        thread::spawn(move || loop {

            match self.shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => {
                    panic!("Algorithm thread disconnected from main thread")
                }
            }

            // 1. Respond to received block sync requests.
            if let Ok((
                origin,
                BlockSyncRequest {
                    start_height,
                    limit, 
                    chain_id },
            )) = self.receiver.recv_request()
            {   
                if chain_id != self.config.chain_id {
                    continue
                }

                Event::ReceiveSyncRequest(ReceiveSyncRequestEvent { timestamp: SystemTime::now(), peer: origin, start_height, limit}).publish(&self.event_publisher);
    
                let bt_snapshot = self.block_tree_camera.snapshot();

                let blocks_res = bt_snapshot
                    .blocks_from_height_to_newest(start_height, max(limit, self.config.request_limit));
                let highest_qc_res = bt_snapshot.highest_qc();

                match (blocks_res, highest_qc_res) {
                    (Ok(blocks), Ok(highest_qc)) => {
                        self.sender.send(origin, BlockSyncResponse{blocks: blocks.clone(), highest_qc: highest_qc.clone()});
    
                        Event::SendSyncResponse(SendSyncResponseEvent{timestamp: SystemTime::now(), peer: origin, blocks, highest_qc}).publish(&self.event_publisher)
                    },
                    _ => {}
                }
    
            }

            // 2. Advertise if needed:
            // - highestQC: to let others know about the server's highestQC known, which may trigger sync if they are behind,
            // - highest committed block height: to let others know about the server's highest committed block height, so that they can 
            //   decide if the server is a suitable sync peer.
            if Instant::now() - self.last_advertisement >= self.config.advertise_time {
                let highest_qc = self.block_tree_camera.snapshot().highest_qc()
                    .expect("Block sync server could not obtain the highestQC!");

                let highest_committed_block_height = 
                    match self.block_tree_camera.snapshot().highest_committed_block_height()
                        .expect("Could not obtain the highest committed block height!") {
                            Some(height) => height,
                            None => BlockHeight::new(0),
                        };
                    
                let advertise_qc_msg = BlockSyncAdvertiseMessage::advertise_qc(highest_qc);
                self.sender.broadcast(advertise_qc_msg);

                let advertise_block_msg = 
                    BlockSyncAdvertiseMessage::advertise_block(&self.config.keypair, self.config.chain_id, highest_committed_block_height);
                self.sender.broadcast(advertise_block_msg);

                self.last_advertisement = Instant::now()
            }


            thread::yield_now();
            
        })

    }

}

/// Immutable parameters that define the behaviour of the [BlockSyncServer].
pub(crate) struct BlockSyncServerConfiguration {
    pub(crate) chain_id: ChainID,
    pub(crate) keypair: Keypair,
    pub(crate) request_limit: u32,
    pub(crate) advertise_time: Duration,
}