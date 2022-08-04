use std::thread;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use crate::NodeTree;

struct Actor;

impl Actor {
    pub fn start() -> Actor { todo!() }

    fn work() -> thread::JoinHandle<()> { 
        // Setup.


        // Leader.

        // Replica.
    }
}