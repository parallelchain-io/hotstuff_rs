use std::sync::mpsc::TryRecvError;
use std::thread;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::net::{SocketAddr, TcpStream, TcpListener};
use std::io::ErrorKind;
use std::time::Duration;
use indexmap::{map, IndexMap};
use rand::Rng;
use crate::progress_mode::ipc;
use crate::msg_types::PublicAddr;
use crate::participants::ParticipantSet;

pub struct ConnectionSet {
    connections: Arc<Mutex<IndexMap<PublicAddr, ipc::Stream>>>,
    participant_set: (ParticipantSetVersion, ParticipantSet), 
    establisher: Establisher,
}

type ParticipantSetVersion = usize;

impl ConnectionSet {
    pub fn new() -> ConnectionSet { todo!() }

    pub fn replace_set(&mut self, new_participant_set: ParticipantSet) { todo!() }

    // Removes the connection identified by public_addr immediately, and schedules it for establishment later.
    pub fn reconnect(&mut self, public_addr: PublicAddr) -> bool { todo!() }

    pub fn get(&self, public_addr: PublicAddr) -> Option<&ipc::Stream> { todo!() }

    pub fn get_random(&self) -> Option<&ipc::Stream> { todo!() } // Returns None is ConnSet is empty.

    pub fn iter(&self) -> IterGuard {
        let conn_set = self.connections.lock().unwrap();
        IterGuard(conn_set)
    }
}

pub struct IterGuard<'a>(MutexGuard<'a, IndexMap<PublicAddr, ipc::Stream>>);

impl<'a> IntoIterator for &'a IterGuard<'a> {
    type Item = (&'a PublicAddr, &'a ipc::Stream);
    type IntoIter = map::Iter<'a, PublicAddr, ipc::Stream>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()    
    }
}

struct Establisher(thread::JoinHandle<()>);

impl Establisher {
    const MASTER_THREAD_SLEEP_TIME: Duration = Duration::new(1, 0);
    const INITIATOR_THREAD_SLEEP_TIME: Duration = Duration::new(1, 0);
    const LISTENER_THREAD_SLEEP_TIME: Duration = Duration::new(1, 0);
    const INITIATOR_TO_MASTER_SYNC_CHANNEL_BOUND: usize = 1000;
    const LISTENER_TO_MASTER_SYNC_CHANNEL_BOUND: usize = 1000;

    fn start(
        connections: Arc<Mutex<IndexMap<PublicAddr, ipc::Stream>>>,
        participant_set_and_version: Arc<Mutex<(ParticipantSetVersion, ParticipantSet)>>,
        listening_addr: SocketAddr,
        my_public_addr: PublicAddr,
    ) -> Establisher {  
        let master_thread = thread::spawn(move || {
            let mut ps_version = None;

            let initiator_tasks = Arc::new(Mutex::new(IndexMap::new()));
            let (from_initiator_to_master, from_initiator) = mpsc::sync_channel(Self::INITIATOR_TO_MASTER_SYNC_CHANNEL_BOUND);
            Self::start_initiator(Arc::clone(&initiator_tasks), from_initiator_to_master);

            let listener_tasks = Arc::new(Mutex::new(IndexMap::new()));
            let (from_listener_to_master, from_listener) = mpsc::sync_channel(Self::LISTENER_TO_MASTER_SYNC_CHANNEL_BOUND);
            Self::start_listener(listening_addr, Arc::clone(&listener_tasks), from_listener_to_master);

            loop {
                // 1. Lock participant_set.
                let ps_lock = participant_set_and_version.lock().unwrap();
                let (latest_ps_version, participant_set) = (ps_lock.0, &ps_lock.1);

                // 2. Check (using ParticipantSetVersion) if ParticipantSet has changed.
                if ps_version.is_none() || ps_version.unwrap() < latest_ps_version {
                    // If yes:
                    ps_version = Some(latest_ps_version);

                    // 2.1. Lock connections.
                    let connections = connections.lock().unwrap();
                    
                    // 2.2. Compute tasks = participant_set - connections.
                    let tasks = {
                        let mut tasks = Vec::new();
                        for (public_addr, socket_addr) in participant_set {
                            if !connections.contains_key(public_addr) {
                                tasks.push((*public_addr, *socket_addr));
                            } else if let Some(existing_stream) = connections.get(public_addr) {
                                if existing_stream.peer_addr() != *socket_addr {
                                    tasks.push((*public_addr, *socket_addr));
                                }
                            }
                        }
                        tasks
                    };

                    // 2.3. Drop the locks on connections and participant set.
                    drop(connections);
                    drop(ps_lock);
                    
                    // 2.4. Divvy up tasks to Initiator and Listener using the formula described in the comments above
                    // the `initiator` and `listener` functions.
                    let mut initiator_tasks = initiator_tasks.lock().unwrap();
                    initiator_tasks.clear();
                    let mut listener_tasks = listener_tasks.lock().unwrap();
                    listener_tasks.clear();
                    for ((public_addr, socket_addr)) in tasks {
                        if public_addr > my_public_addr {
                            initiator_tasks.insert(public_addr, socket_addr);
                        } else if public_addr < my_public_addr {
                            listener_tasks.insert(socket_addr, public_addr);
                        } else {
                            panic!("Application error: PublicAddr of target participant cannot be the same as this Participant's.")
                        } 
                    }
                }

                // 3. Collect newly established connections from:
                let mut new_conns = Vec::new();
                loop {
                    // 3.1. Initiator.
                    match from_initiator.try_recv() {
                        Ok(new_conn) => new_conns.push(new_conn),
                        Err(e) => match e {
                            TryRecvError::Empty => break,
                            _ => panic!("Programming error: Establisher Master thread lost connection to Initiator thread"),
                        }
                    }
                }

                loop {
                    // 3.2. Listener.
                    match from_listener.try_recv() {
                        Ok(new_conn) => new_conns.push(new_conn),
                        Err(e) => match e {
                            TryRecvError::Empty => break,
                            _ => panic!("Programming error: Establisher Master thread lost connection to Initiator thread"),
                        }
                    }
                }

                // 4. Lock participant_set and connections.
                let ps_lock = participant_set_and_version.lock().unwrap();
                let participant_set = &ps_lock.1;
                let mut connections = connections.lock().unwrap();
                
                // 5. If `new_conn` in participant_set but not in connections, insert to connections.
                for (public_addr, new_stream) in new_conns {
                    if participant_set.contains_key(&public_addr) && !connections.contains_key(&public_addr) {
                        connections.insert(public_addr, new_stream);
                    }
                }

                // 6. Drop the locks on participant_set and connections.
                drop(ps_lock);
                drop(connections);
                
                // 7. Sleep.
                thread::sleep(Self::MASTER_THREAD_SLEEP_TIME);
            }
        });

        Establisher(master_thread)
    } 

    /// iff target_public_address < my_public_address, initiator_tasks.insert...
    fn start_initiator(
        tasks: Arc<Mutex<IndexMap<PublicAddr, SocketAddr>>>,
        established_conns: mpsc::SyncSender<(PublicAddr, ipc::Stream)>,
    ) -> thread::JoinHandle<()> { 
        thread::spawn(move || {
            let mut rng = rand::thread_rng();
            loop {
                // 1. Lock tasks.
                let tasks_lock = tasks.lock().unwrap();

                // 2. Pick random task, if any, and try to establish connection.
                let random_idx = rng.gen_range(0..tasks_lock.len());
                if let Some((&public_addr, &socket_addr)) = tasks_lock.get_index(random_idx) {

                    // 3. Drop lock on tasks. 
                    drop(tasks_lock);
        
                    // 4. If successfully established connection, remove task from the tasks list and send the connection to Master.
                    match TcpStream::connect(socket_addr) {
                        Ok(new_stream) => {
                            // Safety: tasks is locked before the established connection is sent to Master to avoid the *possible* pathological scenario that goes:
                            // 1. Initiator sends established connection to Master.
                            // 2. Master receives and ignores the established connection because it is not in participant_set.
                            // 3. Main adds the ignored connection to participant_set.
                            // 4. Master adds the connection to tasks list.
                            // 5. Initiator removes the connection from the tasks list.
                            //
                            // At the end of this scenario, the ignored connection will never be re-established, even though it is in the latest
                            // participant_set. Locking tasks before sending the established connection forces step number 5 in the scenario to
                            // happen before step number 4, so that in the end of the scenario, the ignored connection will still be in the tasks
                            // list. Like so:
                            // 1. Initiator sends established connection to Master.
                            // -- Initiator locks the tasks list -- 
                            // 2. Master receives and ignores the established connection because it is not in participant_set.
                            // 3. Main adds the ignored connection to participant_set.
                            // 4. Initiator removes the connection from the tasks list.
                            // -- Initiator drops the lock on the tasks list --
                            // 5. Master adds the connection to tasks list.
                            let mut tasks_lock = tasks.lock().unwrap();
                            established_conns.send((public_addr, ipc::Stream::new(new_stream)))
                                .expect("Programming error: Initiator thread lost connection to Master thread.");
                            tasks_lock.remove(&public_addr);
                        },
                        Err(e) => if e.kind() != ErrorKind::TimedOut {
                            panic!("Programming error: unexpected error when trying the establish connection with remote peer.")
                        }
                    }
                } else {
                    //  2A. Sleep if there are no tasks.
                    thread::sleep(Self::INITIATOR_THREAD_SLEEP_TIME)
                }
            }
        })
    }

    /// iff target_public_address > my_public_address, listener_tasks.insert...
    fn start_listener(
        listening_addr: SocketAddr,
        tasks: Arc<Mutex<IndexMap<SocketAddr, PublicAddr>>>,
        established_conns: mpsc::SyncSender<(PublicAddr, ipc::Stream)>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let listener = TcpListener::bind(listening_addr)
                .expect(&format!("Configuration or Programming error: fail to bind TcpListener to addr: {}", listening_addr));

            // 1. Accept incoming streams.
            for incoming_stream in listener.incoming() {
                let incoming_stream = incoming_stream
                    .expect("Programming error: un-matched error when trying to accept incoming TcpStream."); 

                // 2. Lock tasks. 
                let mut tasks = tasks.lock().unwrap();

                // 3. Check if new stream is in the tasks list. If so, send the established stream to Master.
                let peer_addr = incoming_stream.peer_addr()
                    .expect("Programming error: un-matched error when trying to get peer_addr() of stream.");
                if let Some(public_addr) = tasks.get(&peer_addr) {
                    established_conns.send((*public_addr, ipc::Stream::new(incoming_stream))).unwrap();
                }

                // 4. Remove completed task from the tasks list.
                tasks.remove(&peer_addr);


                // 5. Drop lock on tasks.
            }
        })
    }
}

