use std::collections::hash_map;
use std::ops::Deref;
use std::thread;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::net::SocketAddr;
use std::iter::Iterator;
use crate::ipc::{self, ConnSet};
use crate::msg_types::{PublicAddress,  ParticipantSet};

pub struct ManagedConnSet {
    conn_set: Arc<Mutex<ConnSet>>,
    establisher: thread::JoinHandle<()>,
    to_establisher: mpsc::Sender<EstablisherRequest>,
}

impl ManagedConnSet {
    pub fn new(conn_set: Arc<Mutex<ConnSet>>) -> ManagedConnSet { todo!() }

    pub fn replace_connection_set(&mut self, new_participant_set: ParticipantSet) { todo!() }

    // Queues the corresponding (PublicAddress, SocketAddr) pairs for re-establishment.
    pub fn reconnect(&mut self, socket_addrs: Vec<SocketAddr>) { todo!() }

    pub fn get(&self, public_addr: PublicAddress) -> Option<&ipc::Stream> { todo!() }

    pub fn get_random(&self) -> Option<&ipc::Stream> { todo!() } // Returns None is ConnSet is empty.

    pub fn iter(&self) -> IterGuard {
        let conn_set = self.conn_set.lock().unwrap();
        IterGuard(conn_set)
    }
}

pub struct IterGuard<'a>(MutexGuard<'a, ConnSet>);

impl<'a> IntoIterator for &'a IterGuard<'a> {
    type Item = (&'a PublicAddress, &'a ipc::Stream);
    type IntoIter = hash_map::Iter<'a, PublicAddress, ipc::Stream>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()    
    }
}

impl ManagedConnSet {
    fn establisher(conn_set: Arc<Mutex<ConnSet>>, from_main: mpsc::Receiver<EstablisherRequest>) -> thread::JoinHandle<()> { todo!() } 
}

enum EstablisherRequest {
    ReplaceConnectionSet(Vec<(PublicAddress, SocketAddr)>),
    Reconnect((PublicAddress, SocketAddr)),
}
