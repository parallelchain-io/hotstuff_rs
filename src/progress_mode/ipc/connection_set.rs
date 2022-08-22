use std::thread;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::net::SocketAddr;
use indexmap::{map, IndexMap};
use crate::progress_mode::ipc;
use crate::msg_types::PublicAddress;
use crate::participants::ParticipantSet;

pub struct ConnectionSet {
    connections: Arc<Mutex<IndexMap<PublicAddress, ipc::Stream>>>,
    maintainer: thread::JoinHandle<()>,
    to_maintainer: mpsc::Sender<MaintainerCmd>,
}

impl ConnectionSet {
    pub fn new() -> ConnectionSet { todo!() }

    pub fn replace_set(&mut self, new_participant_set: ParticipantSet) { todo!() }

    pub fn reconnect(&mut self, public_addr: PublicAddress) { todo!() }

    pub fn get(&self, public_addr: PublicAddress) -> Option<&ipc::Stream> { todo!() }

    pub fn get_random(&self) -> Option<&ipc::Stream> { todo!() } // Returns None is ConnSet is empty.

    pub fn iter(&self) -> IterGuard {
        let conn_set = self.connections.lock().unwrap();
        IterGuard(conn_set)
    }
}

pub struct IterGuard<'a>(MutexGuard<'a, IndexMap<PublicAddress, ipc::Stream>>);

impl<'a> IntoIterator for &'a IterGuard<'a> {
    type Item = (&'a PublicAddress, &'a ipc::Stream);
    type IntoIter = map::Iter<'a, PublicAddress, ipc::Stream>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()    
    }
}

impl ConnectionSet {
    fn maintainer(
        connections: Arc<Mutex<IndexMap<PublicAddress, ipc::Stream>>>,
        cmds: mpsc::Receiver<MaintainerCmd>,
    ) -> thread::JoinHandle<()> { 
        // 1. Get MaintainerCmd.
        // 2. If Replace, set target_conns = new_conns. Then, compute new_conns - cur_conns = tasks. Split tasks into two task lists according to the predicates in the comments below.
        // 3. established conn = recv_timeout from initiator and from listener. If conns is not in target_conns, discard. Else, assert that it is *not* in conns, then insert to conns.
        // 4. If Reconnect, then put in the appropriate task list (according to the predicates).
        todo!()
    } 

    /// iff target_public_address < my_public_address, initiator_tasks.insert...
    fn initiator(
        tasks: Arc<Mutex<IndexMap<PublicAddress, SocketAddr>>>,
        established_conns: mpsc::Sender<(PublicAddress, ipc::Stream)>,
    ) -> thread::JoinHandle<()> { 
        // 1. Pick random tasks, try to establish connection.
        // 2. If successful, remove from tasks and send established conn to maintainer.
        todo!()
    }

    /// iff target_public_address > my_public_address, listener_tasks.insert...
    fn listener(
        tasks: Arc<Mutex<IndexMap<PublicAddress, SocketAddr>>>,
        established_conns: mpsc::Sender<(PublicAddress, ipc::Stream)>,
    ) -> thread::JoinHandle<()> { 
        // 1. Listen for new streams.
        // 2. Check if new stream is in tasks. If so, initiate connection. 
        // 3. If successful, remove from tasks and send established conn to maintainer.
        todo!()
    }
}

enum MaintainerCmd {
    Replace(ParticipantSet),
    Reconnect(PublicAddress),
}
