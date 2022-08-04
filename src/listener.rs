use std::thread;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use crate::NodeTree;

use Listener;

impl Listener {
    pub fn start() -> Listener { todo!() }

    fn work() -> thread::JoinHandle<()> {
        // Read from consensus_api::Handle.
        
        // Catch up using node_tree_api::Handle, if needed.

        // Put into either ProposalQueue or VoteQueue.
    }
}