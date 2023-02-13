use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use crate::messages::{SyncRequest, SyncResponse};
use crate::state::{BlockTree, KVStore};
use crate::networking::SyncServerStub;

fn start_sync_server<'a, K: KVStore<'a>>(
    block_tree: &BlockTree<'a, K>,
    sync_stub: SyncServerStub,
    shutdown_signal: Receiver<()>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match shutdown_signal.try_recv() {
                Ok(()) => return,
                Err(TryRecvError::Empty) => (),
                Err(TryRecvError::Disconnected) => panic!("Algorithm thread disconnected from main thread"),
            }

            let (origin, SyncRequest { highest_committed_block, limit }) = sync_stub.recv();
            let bt_snapshot = block_tree.snapshot();
            let blocks = bt_snapshot.blocks_from_tail(&highest_committed_block, limit);
            let highest_qc: QuorumCertificate = bt_snapshot.highest_qc();
            let response = SyncResponse { blocks, highest_qc };
            sync_stub.send(&origin, response);
        }   
    })
}