/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

//! The server side of the block sync protocol.
//! 
//! ## Block sync protocol
//! 
//! When the [algorithm thread](crate::algorithm) sees a message containing a quorum certificate from a future view,
//! this is taken to indicate that a quorum of validators are making progress at a higher view. This causes the replica
//! to start syncing.
//! 
//! The sync protocol is a request-response protocol that proceeds as follows:
//! 1. The lagging replica selects a random peer from its current validator set (the "sync replica").
//! 2. It then sends the sync replica a [sync request](crate::messages::SyncRequest) telling them the highest committed 
//!    block that it knows (if it knows any committed block).
//! 3. The sync replica responds with a [sync response](crate::messages::SyncResponse) containing the highest quorum
//!    certificate it knows, and a sequence of blocks extending from the lagging replica's highest committed block 
//!    (if the request specifies it) or the beginning of the block tree (otherwise).
//! 4. Upon receiving the response, the lagging replica checks how many blocks are included in it:
//!     - If the response contains no blocks, then the replica returns to executing the progress protocol. 
//!     - If it does contain blocks, then the replica validates the included blocks and quorum certificate and inserts 
//!       them into its block tree, and then jumps back to step 2.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use crate::logging::debug;
use crate::messages::{SyncRequest, SyncResponse};
use crate::state::{BlockTreeCamera, KVStore};
use crate::networking::{Network, SyncServerStub};

pub(crate) fn start_sync_server<K: KVStore, N: Network + 'static>(
    block_tree: BlockTreeCamera<K>,
    mut sync_stub: SyncServerStub<N>,
    shutdown_signal: Receiver<()>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => panic!("Algorithm thread disconnected from main thread"),
            }

            if let Some((origin, SyncRequest { highest_committed_block, limit })) = sync_stub.recv_request() {
                debug::received_sync_request(&origin);
                
                let bt_snapshot = block_tree.snapshot();
                let blocks = bt_snapshot.blocks_from_tail_to_newest_block(highest_committed_block.as_ref(), limit);
                let highest_qc = bt_snapshot.highest_qc();

                sync_stub.send_response(origin, SyncResponse { blocks, highest_qc });
            }
            thread::yield_now();
        }   
    })
}