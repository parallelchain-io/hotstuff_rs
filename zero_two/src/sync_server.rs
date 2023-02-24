/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
    
    Authors: Alice Lim
*/

use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
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

            let (origin, SyncRequest { highest_committed_block, limit }) = sync_stub.recv_request();
            
            let bt_snapshot = block_tree.snapshot();
            let blocks = bt_snapshot.blocks_from_tail_to_newest_block(highest_committed_block.as_ref(), limit);
            let highest_qc = bt_snapshot.highest_qc();

            sync_stub.send_response(origin, SyncResponse { blocks, highest_qc });
        }   
    })
}