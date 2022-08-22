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
        // 2. If Replace, split into two EstablisherCmds using the predicates described in the comments below: one for initiator, one for listener.
        // 3. established conn = recv_timeout from initiator and from listener. if conn is in target_conns but not in connections, put it in
        //    connections.
        todo!()
    } 

    /// iff target_public_address < my_public_address, to_initiator.send...
    fn initiator(
        cmds: mpsc::Receiver<EstablisherCmd>,
        established_conns: mpsc::Sender<(PublicAddress, ipc::Stream)>,
    ) -> thread::JoinHandle<()> { 
        // 1. Get EstablisherCmd.
        //     - If ReplaceTasks, replace tasks with the new one. 
        //     - If AddTask, add new task. 
        // 2. Pick random tasks, start doing it.
        // 3. If successful, remove from tasks and send established conn to maintainer.
        todo!()
    }

    /// else, to_listener.send...
    fn listener(
        cmds: mpsc::Receiver<EstablisherCmd>,
        established_conns: mpsc::Sender<(PublicAddress, ipc::Stream)>,
    ) -> thread::JoinHandle<()> { 
        // 1. Same as initiator.1.
        // 2. Listen to serversocket up to a timeout.
        // 3. Check if initiated stream is in tasklist if so, initiate connection. 
        // 4. If successful, remove from tasks and send established conn to maintainer.
        todo!()
    }
}

enum MaintainerCmd {
    Replace(ParticipantSet),
    Reconnect(PublicAddress),
}

enum EstablisherCmd {
    ReplaceTasks(ParticipantSet),
    AddTask(PublicAddress),
}
