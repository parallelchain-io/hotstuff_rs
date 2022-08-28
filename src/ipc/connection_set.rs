use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::net::IpAddr;
use std::collections::BTreeMap;
use std::thread;
use indexmap::{map, IndexMap};
use rand::Rng;
use rand::rngs::ThreadRng;
use crate::config::{IPCConfig, IdentityConfig};
use crate::ipc::{self, Establisher, EstablisherResult};
use crate::identity::{PublicAddr, ParticipantSet};

pub struct ConnectionSet {
    // To avoid deadlocks and to guarantee consistency, lock participant_set first, then connections, and finally pending_connections.
    participant_set: Arc<Mutex<ParticipantSet>>, 
    pending_connections: Arc<Mutex<BTreeMap<PublicAddr, IpAddr>>>,
    connections: Arc<Mutex<IndexMap<PublicAddr, Arc<ipc::Stream>>>>,

    // Randomness source for `get_random`.
    rng: Mutex<ThreadRng>,

    establisher: Establisher,
    establisher_receiver: thread::JoinHandle<()>,
}

type ParticipantSetVersion = usize;

impl ConnectionSet {
    pub fn new(identity_config: IdentityConfig, ipc_config: IPCConfig) -> ConnectionSet { 
        // 1. Create Establisher.
        let (establisher, from_establisher) = Establisher::new(ipc_config, identity_config.my_public_addr);
        
        // 2. Send Connect requests to Establisher for all participants in the static participant set.
        for target in identity_config.static_participant_set.iter() {
            establisher.connect_later((*target.0, *target.1));
        }

        let participant_set = Arc::new(Mutex::new(identity_config.static_participant_set.clone()));
        let pending_connections = Arc::new(Mutex::new(identity_config.static_participant_set));
        let connections = Arc::new(Mutex::new(IndexMap::new()));

        ConnectionSet {
            participant_set: Arc::clone(&participant_set),
            pending_connections: Arc::clone(&pending_connections),
            connections: Arc::clone(&connections),
            rng: Mutex::new(rand::thread_rng()),
            establisher,
            establisher_receiver: Self::start_establisher_receiver(from_establisher, participant_set, connections, pending_connections)
        }
    }

    // Removes connections not appearing in new_participant_set immediately, and schedules new connections for establishment later.
    pub fn replace_set(&mut self, new_participant_set: ParticipantSet) { 
        let mut participant_set = self.participant_set.lock().unwrap();
        let mut pending_connections = self.pending_connections.lock().unwrap();
        let mut connections = self.connections.lock().unwrap();

        // 1. Replace participant_set.
        participant_set.clear();
        participant_set.extend(new_participant_set.iter());

        // 2. Only keep existing connections that feature in new_participant_set. 
        connections.retain(|existing_public_addr, existing_stream| {
            if let Some(new_ip_addr) = new_participant_set.get(existing_public_addr) {
                if existing_stream.peer_addr().ip() == *new_ip_addr {
                    true
                } else {
                    false
                }
            } else {
                false
            }
        });

        // 3. - Compute 'stale pending connections': pending connections that no longer feature in the new participant set.
        //    - Remove stale pending connections from pending connections.
        let mut stale_pending_connections = Vec::new();
        pending_connections.retain(|pending_public_addr, pending_ip_addr| {
            if let Some(new_ip_addr) = new_participant_set.get(pending_public_addr) {
                pending_ip_addr == new_ip_addr
            } else {
                stale_pending_connections.push((*pending_public_addr, *pending_ip_addr));
                false
            }
        });

        // 4. Send Cancel commands to Establisher for stale pending connections.
        for stale_pending_connection in stale_pending_connections {
            self.establisher.cancel_later(stale_pending_connection)
        }

        // 5. - Compute 'new pending connections': (wannabe) connections in the new participant set that are not in pending
        //      connections.
        //    - Insert new pending connections to pending connections.
        let mut new_pending_connections = Vec::new();
        for (public_addr, ip_addr) in participant_set.iter() {
            if !pending_connections.contains_key(public_addr) {
                new_pending_connections.push((*public_addr, *ip_addr));
                pending_connections.insert(*public_addr, *ip_addr);
            }
        }

        // 6. Send Connect commands to Establisher for new pending connections.
        for new_pending_connection in new_pending_connections {
            self.establisher.connect_later(new_pending_connection);
        }
    }

    // Removes the connection identified by public_addr immediately, and schedules it for establishment later.
    pub fn reconnect(&self, target: (PublicAddr, IpAddr)) { 
        let mut connections = self.connections.lock().unwrap();
        let pending_connections = self.pending_connections.lock().unwrap();

        connections.remove(&target.0);
        if !pending_connections.contains_key(&target.0) {
            self.establisher.connect_later(target);
        }
    }

    pub fn get(&self, public_addr: &PublicAddr) -> Option<Arc<ipc::Stream>> { 
        Some(Arc::clone(self.connections.lock().unwrap().get(public_addr)?))
    }

    // Returns None if ConnectionSet is empty.
    pub fn get_random(&self) -> Option<(PublicAddr, Arc<ipc::Stream>)> { 
        let connections = self.connections.lock().unwrap();
        let random_idx = self.rng.lock().unwrap().gen_range(0..connections.len());
        match connections.get_index(random_idx) {
            Some((public_addr, stream)) => Some((*public_addr, Arc::clone(stream))),
            None => None,
        }
    }

    pub fn iter(&self) -> IterGuard {
        IterGuard(self.connections.lock().unwrap())
    }

    fn start_establisher_receiver(
        from_establisher: mpsc::Receiver<EstablisherResult>, 
        participant_set: Arc<Mutex<ParticipantSet>>,
        connections: Arc<Mutex<IndexMap<PublicAddr, Arc<ipc::Stream>>>>,
        pending_connections: Arc<Mutex<BTreeMap<PublicAddr, IpAddr>>>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            loop {
                let EstablisherResult((public_addr, new_stream)) = from_establisher.recv().expect("Programming error: channel between establisher_receiver and Establisher dropped.");
                let participant_set = participant_set.lock().unwrap();
                let mut connections = connections.lock().unwrap();
                let mut pending_connections = pending_connections.lock().unwrap();

                if participant_set.contains_key(&public_addr) {
                    if !connections.contains_key(&public_addr) {
                        connections.insert(public_addr, new_stream);
                        let _ = pending_connections.remove(&public_addr);
                    }
                }
            }
        })
    }
}

pub struct IterGuard<'a>(MutexGuard<'a, IndexMap<PublicAddr, Arc<ipc::Stream>>>);

impl<'a> IntoIterator for &'a IterGuard<'a> {
    type Item = (&'a PublicAddr, &'a Arc<ipc::Stream>);
    type IntoIter = map::Iter<'a, PublicAddr, Arc<ipc::Stream>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()    
    }
}

