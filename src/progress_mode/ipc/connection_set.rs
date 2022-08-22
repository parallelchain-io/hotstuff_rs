use std::thread;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::net::SocketAddr;
use indexmap::{map, IndexMap};
use crate::progress_mode::ipc;
use crate::msg_types::PublicAddress;
use crate::participants::ParticipantSet;

pub struct ConnectionSet {
    connections: Arc<Mutex<IndexMap<PublicAddress, ipc::Stream>>>,
    participant_set: (ParticipantSetVersion, ParticipantSet), 
    maintainer: thread::JoinHandle<()>,
    to_maintainer: mpsc::Sender<MaintainerCmd>,
}

type ParticipantSetVersion = usize;

impl ConnectionSet {
    pub fn new() -> ConnectionSet { todo!() }

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
        participant_set: Arc<Mutex<(ParticipantSetVersion, ParticipantSet)>>,
    ) -> thread::JoinHandle<()> { 
        // 1. Lock participant_set.

        // 2. Check (using ParticipantSetVersion) if ParticipantSet has changed.
        
        // 3. If no, continue, if yes:
        //    1. Lock connections.
        //    2. Compute tasks = participant_set - connections.
        //    3. Divvy up tasks to initiator and listener using the formulae in the comments below.
        //    4. Unlock connections.

        // 4. Unlock participant_set.

        // 5. Wait a timeout to receive newly established connection `new_conn` from Initiator or Listener. 

        // 6. Lock participant_set.
        
        // 7. Lock connections.
        
        // 8. If `new_conn` in participant_set but not in connections, insert to connections.
        
        // 9. Unlock participant_set.

        // 10. Unlock connections.
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
