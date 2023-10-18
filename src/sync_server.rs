/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! The server side of the block sync protocol.
//!
//! When the [algorithm thread](crate::algorithm) sees a message containing a quorum certificate from a future view,
//! this is taken to indicate that a quorum of validators are making progress at a higher view. This causes the replica
//! to start syncing.
//!
//! ## Block sync protocol
//!
//! The sync protocol is a request-response protocol whereby replicas try to download blocks it may
//! have missed during downtime or network outage.
//!
//! The protocol proceeds as follows:
//! 1. The lagging replica selects a random peer from its current validator set (the "sync replica").
//! 2. It then sends the sync replica a [sync request](crate::messages::SyncRequest) asking them to start syncing from
//!    a start height: the replica's highest committed block's height + 1 (or 0, if the replica hasn't committed any
//!    blocks). It also specifies how many blocks at most it wants to receive.
//! 3. The sync replica responds with a [sync response](crate::messages::SyncResponse) containing a chain starting from
//!    the request's start height and of length at most the maximum of the sync replica's limit and the request's limit.
//! 4. Upon receiving the response, the lagging replica checks the blocks that are included in it:
//!     - If the response contains no new blocks, then the replica returns to executing the progress protocol.
//!     - If it does contain new blocks, then the replica validates the blocks and quorum certificate and inserts them
//!       into its block tree, and then jumps back to step 2.

use crate::events::{Event, ReceiveSyncRequestEvent, SendSyncResponseEvent};
use crate::messages::{SyncRequest, SyncResponse};
use crate::networking::{Network, SyncServerStub};
use crate::state::{BlockTreeCamera, KVStore};
use std::cmp::max;
use std::sync::mpsc::{Receiver, TryRecvError, Sender};
use std::thread::{self, JoinHandle};
use std::time::SystemTime;

pub(crate) fn start_sync_server<K: KVStore, N: Network + 'static>(
    block_tree: BlockTreeCamera<K>,
    mut sync_stub: SyncServerStub<N>,
    shutdown_signal: Receiver<()>,
    sync_request_limit: u32,
    event_publisher: Option<Sender<Event>>,
) -> JoinHandle<()> {
    thread::spawn(move || loop {
        match shutdown_signal.try_recv() {
            Ok(()) => return,
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => {
                panic!("Algorithm thread disconnected from main thread")
            }
        }

        if let Some((
            origin,
            SyncRequest {
                start_height,
                limit,
            },
        )) = sync_stub.recv_request()
        {
            Event::ReceiveSyncRequest(ReceiveSyncRequestEvent { timestamp: SystemTime::now(), peer: origin, start_height, limit}).publish(&event_publisher);

            let bt_snapshot = block_tree.snapshot();
            let blocks = bt_snapshot
                .blocks_from_height_to_newest(start_height, max(limit, sync_request_limit));
            let highest_qc = bt_snapshot.highest_qc();

            sync_stub.send_response(origin, SyncResponse { blocks: blocks.clone(), highest_qc: highest_qc.clone() });

            Event::SendSyncResponse(SendSyncResponseEvent { timestamp: SystemTime::now(), peer: origin, blocks, highest_qc}).publish(&event_publisher)
        }
        thread::yield_now();
    })
}
