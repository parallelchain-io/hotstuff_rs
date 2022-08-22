use std::thread;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::net::SocketAddr;
use indexmap::{map, IndexMap};
use crate::msg_types::{PublicAddress,  ParticipantSet};
use crate::progress_mode::ipc;

pub struct ConnectionSet {
    connections: Arc<Mutex<IndexMap<PublicAddress, ipc::Stream>>>,
    establisher: thread::JoinHandle<()>,
    to_establisher: mpsc::Sender<EstablisherRequest>,
}

impl ConnectionSet {
    pub fn new() -> ConnectionSet { todo!() }

    pub fn replace_set(&mut self, new_participant_set: ParticipantSet) { todo!() }

    // Queues the corresponding (PublicAddress, SocketAddr) pairs for re-establishment.
    pub fn reconnect(&mut self, socket_addrs: Vec<SocketAddr>) { todo!() }

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
    fn establisher(
        connections: Arc<Mutex<IndexMap<PublicAddress, ipc::Stream>>>,
        requests: mpsc::Receiver<EstablisherRequest>,
    ) -> thread::JoinHandle<()> { todo!() } 

    /// iff target_public_address < my_public_address, to_initiator.send...
    fn initiator(
        requests: mpsc::Receiver<EstablisherRequest>,
        established_conns: mpsc::Sender<(PublicAddress, ipc::Stream)>,
    ) -> thread::JoinHandle<()> { todo!() }

    /// else, to_listener.send...
    fn listener(
        requests: mpsc::Receiver<EstablisherRequest>,
        established_conns: mpsc::Sender<(PublicAddress, ipc::Stream)>,
    ) -> thread::JoinHandle<()> { todo!() }
}

enum EstablisherRequest {
    ReplaceConnectionSet(Vec<(PublicAddress, SocketAddr)>),
    Reconnect((PublicAddress, SocketAddr)),
}
