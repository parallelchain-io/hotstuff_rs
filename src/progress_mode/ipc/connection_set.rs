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

    // For consistency, does *not* remove the connections that do not feature in new_participant_set immediately, instead, schedules for the maintainer thread to do this,
    // as well as establish new connections, later.
    pub fn replace_set(&mut self, new_participant_set: ParticipantSet) { todo!() }

    // Removes the connection identified by public_addr immediately, and schedules it for establishment later.
    pub fn reconnect(&mut self, public_addr: PublicAddress) -> bool { todo!() }

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
        // 1. Get EstablisherCmd.
        // 2A. If Replace, set target_conns = new_conns, and tasks = new_conns - cur_conns. Split tasks into two task lists according to the predicates in the comments below.
        // 2B. If Reconnect, then put in the appropriate task list (according to the predicates).
        // 3. established conn = recv_timeout from initiator and from listener. If conn is in target_connections and *not* in conns, insert to connections.
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
