use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::thread;
use std::sync::{Arc, RwLock, Mutex, mpsc};
use std::time;
use threadpool::ThreadPool;
use crate::msg_types::{PublicAddress, ConsensusMsg, SerDe};
use crate::ipc::{self, ConnectionSet};

pub struct Manager {
    establisher: thread::JoinHandle<()>,
    sender: thread::JoinHandle<()>,
    to_establisher: mpsc::Sender<EstablishRequest>,
    to_sender: mpsc::Sender<SendRequest>,
    // The listening side of sender is blocking, and is implemented directly on ipc::Handle. 
}

impl Manager {
    const N_SENDERS: usize = 4;
    const ESTABLISH_TIMEOUT: time::Duration = time::Duration::new(15, 0);
    const WRITE_TIMEOUT: time::Duration = Self::ESTABLISH_TIMEOUT;
    const READ_TIMEOUT: time::Duration = Self::WRITE_TIMEOUT; 
    const LISTENER_ADDR: &'static str = "127.0.0.1:8080";

    fn new() -> (Manager, ipc::Handle) {
        let connections = Arc::new(RwLock::new(ConnectionSet::new()));
        let (to_establisher, establisher_from_handles) = mpsc::channel();
        let (to_sender, sender_from_handles ) = mpsc::channel();
        let manager = Manager {
            establisher: Self::establisher(connections.clone(), establisher_from_handles),
            sender: Self::sender(connections.clone(), sender_from_handles, to_establisher.clone()),
            to_establisher: to_establisher.clone(),
            to_sender: to_sender.clone(),
        };
        let handle = ipc::Handle::new(connections, to_establisher, to_sender.clone());

        (manager, handle)
    }

    fn establisher(connections: Arc<RwLock<ConnectionSet>>, requests: mpsc::Receiver<EstablishRequest>) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let pending_connections = Arc::new(RwLock::new(HashMap::new())); 

            // Spawn the Listener thread.
            // TODO: when scoped threads become stable, these clones can be removed.
            let pending_connections_for_listener = pending_connections.clone();
            let connections_for_listener = connections.clone();
            let _ = thread::spawn(move || {
                let pending_connections = pending_connections_for_listener;
                let connections = connections_for_listener;

                let listener = TcpListener::bind(Self::LISTENER_ADDR).expect("Irrecoverable: failed to bind IPC Manager Establisher listener");
                for stream in listener.incoming() {
                    if let Ok(stream) = stream {
                        stream.set_read_timeout(Some(Self::READ_TIMEOUT)).unwrap();
                        stream.set_write_timeout(Some(Self::WRITE_TIMEOUT)).unwrap();
                        let peer_addr = stream.peer_addr().unwrap();
                        if let Some(public_addr) = pending_connections.read().unwrap().get(&peer_addr) {
                            connections.write().unwrap().insert(*public_addr, stream);
                            pending_connections.write().unwrap().remove(&peer_addr);
                        }
                    } 
                }
            });

            // Spawn the Initiator thread.
            // TODO: when scoped threads become stable, this clone can be removed.
            let pending_connections_for_initiator = pending_connections.clone();
            let _ = thread::spawn(move || {
                let pending_connections = pending_connections_for_initiator;
                loop {
                    for (socket_addr, public_addr) in &*pending_connections.read().unwrap() {
                        if let Ok(stream) = TcpStream::connect_timeout(socket_addr, Self::ESTABLISH_TIMEOUT) {
                            connections.write().unwrap().insert(*public_addr, stream);
                        }
                    } 
                }
            });

            // Place connection establishment requests into pending_connections.
            for EstablishRequest(addrs) in requests {
                let mut pending_connections = pending_connections.write().unwrap();
                for (public_addr, peer_addr) in addrs {
                    pending_connections.insert(peer_addr, public_addr);
                }
            }
        })
    } 

    fn sender(connections: Arc<RwLock<ConnectionSet>>, requests: mpsc::Receiver<SendRequest>, to_establisher: mpsc::Sender<EstablishRequest>) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let workers = ThreadPool::new(Self::N_SENDERS);
    
            for request in requests {
                // When scoped threads become stable, this Arc can be removed.
                let errored_connections = Arc::new(Mutex::new(Vec::new()));

                match request {
                        SendRequest::SendTo(public_addr, msg) => {
                            if let Some(mut stream) = connections.read().unwrap().get(&public_addr) {
                                if stream.write_all(&msg.serialize()).is_err() {
                                    errored_connections.lock().unwrap().push((public_addr.clone(), stream.peer_addr().unwrap()));
                                }
                            }
                        },
                        SendRequest::Broadcast(msg) => {
                            let connections = connections.read().unwrap();
                            for (public_addr, stream) in &*connections {
                                // TODO: when scoped threads become stable, these clones can be removed.
                                let errored_connections = errored_connections.clone();
                                let msg = msg.clone();
                                let public_addr = public_addr.clone();
                                let mut stream = stream.try_clone().unwrap();
                                workers.execute(move || {
                                    if stream.write_all(&msg.serialize()).is_err() {
                                        errored_connections.lock().unwrap().push((public_addr, stream.peer_addr().unwrap()));
                                    }
                                })
                            }  
                        },
                    }

                    let errored_connections = errored_connections.lock().unwrap();
                    if errored_connections.len() > 0 {
                        // Drop errored connections.
                        let mut connections = connections.write().unwrap();
                        for (public_addr, _) in &*errored_connections {
                            connections.remove(public_addr);
                        }

                        // Try to re-establish errored connections.
                        to_establisher.send(EstablishRequest(errored_connections.to_vec())).unwrap();
                    }
                }
        })
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        todo!()
    }
}

pub struct EstablishRequest(Vec<(PublicAddress, SocketAddr)>);

pub enum SendRequest {
    SendTo(PublicAddress, ConsensusMsg),
    Broadcast(ConsensusMsg),
}
