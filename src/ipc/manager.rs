use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::thread;
use std::sync::{Arc, RwLock, Mutex, mpsc};
use std::time;
use threadpool::ThreadPool;
use crate::msg_types::{PublicAddress, ConsensusMsg, SerDe};
use crate::ipc::{self, ConnectionSet};

/// ipc::Manager works in the background to implement the non-blocking sends, broadcasts, and establishment of new TCP connections
/// offered by ipc::Handle.
pub struct Manager {
    establisher: thread::JoinHandle<()>,
    sender: thread::JoinHandle<()>,
    // The listening side of sender is blocking, and is implemented directly on ipc::Handle. 
}

impl Manager {
    const N_SENDERS: usize = 4;

    const ESTABLISH_TIMEOUT: time::Duration = time::Duration::new(15, 0);
    const WRITE_TIMEOUT: time::Duration = Self::ESTABLISH_TIMEOUT;
    const READ_TIMEOUT: time::Duration = Self::WRITE_TIMEOUT; 

    const LISTENER_IP_ADDR: &'static str = "127.0.0.1:53410";

    fn new() -> (Manager, ipc::Handle) {
        let connections = Arc::new(RwLock::new(ConnectionSet::new()));
        let (to_establisher, establisher_from_handles) = mpsc::channel();
        let (to_sender, sender_from_handles ) = mpsc::channel();
        let manager = Manager {
            establisher: Self::establisher(connections.clone(), establisher_from_handles),
            sender: Self::sender(connections.clone(), sender_from_handles, to_establisher.clone()),
        };
        let handle = ipc::Handle {
            connections,
            to_establisher,
            to_sender: to_sender.clone()
        };

        (manager, handle)
    }

    // Spawns the establisher threads. These are responsible for establishing new connections upon ipc::Handle::update_connections being
    // called.
    fn establisher(connections: Arc<RwLock<ConnectionSet>>, requests: mpsc::Receiver<EstablisherRequest>) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let pending_connections = Arc::new(RwLock::new(HashMap::<PublicAddress, SocketAddr>::new())); 

            // Spawn the Listener thread.
            // The Listener thread listens on a TcpListener for incoming connections. If an incoming connection matches one in pending_
            // it places it in the ConnectionSet shared between the components of the IPC module.

            // TODO: when scoped threads become stable, these clones can be removed.
            let pending_connections_for_listener = pending_connections.clone();
            let connections_for_listener = connections.clone();
            let _ = thread::spawn(move || {
                let pending_connections = pending_connections_for_listener;
                let connections = connections_for_listener;

                let listener = TcpListener::bind(Self::LISTENER_IP_ADDR).expect("Irrecoverable: failed to bind IPC Manager Establisher listener");
                for stream in listener.incoming() {
                    if let Ok(stream) = stream {
                        stream.set_read_timeout(Some(Self::READ_TIMEOUT)).unwrap();
                        stream.set_write_timeout(Some(Self::WRITE_TIMEOUT)).unwrap();

                        let peer_addr = stream.peer_addr().unwrap();
                        let mut pending_connections = pending_connections.write().unwrap();

                        // Register the new stream in ConnectionSet to all of the PublicAddresses it is associated with.
                        let public_addrs: Vec<([u8; 32], SocketAddr)> = pending_connections
                            .iter()
                            .filter(|(_, _peer_addr)| **_peer_addr == peer_addr) 
                            .map(|(public_addr, peer_addr)| (public_addr.to_owned(), peer_addr.to_owned()))
                            .collect();
                        for (public_addr, _) in public_addrs {
                            connections.write().unwrap().insert(public_addr, stream.try_clone().unwrap());
                            pending_connections.remove(&public_addr);
                        }                        
                    } 
                }
            });

            // Spawn the Initiator thread.
            // The Initiator thread continually attempts to turn pending connections into actual TCP connections. If successful, it places
            // actual connections to the shared ConnectionSet.

            // TODO: when scoped threads become stable, this clone can be removed.
            let pending_connections_for_initiator = pending_connections.clone();
            let connections_for_initiator = connections.clone();
            let _ = thread::spawn(move || {
                let pending_connections = pending_connections_for_initiator;
                let connections = connections_for_initiator;

                loop {
                    for (public_addr, peer_addr) in &*pending_connections.read().unwrap() {
                        if let Ok(stream) = TcpStream::connect_timeout(peer_addr, Self::ESTABLISH_TIMEOUT) {
                            connections.write().unwrap().insert(*public_addr, stream);
                        }
                    } 
                }
            });

            // Update `pending_connections` and `connections` in response to EstablisherRequests.
            for request in requests {
                let mut pending_connections = pending_connections.write().unwrap();
                let mut connections = connections.write().unwrap();

                match request {
                    EstablisherRequest::ReplaceConnectionSet(new_addrs) => {
                        // Remove actual connections that should not be part of the new ConnectionSet.
                        connections.retain(|public_addr, socket_addr| {
                            new_addrs.contains(&(*public_addr, socket_addr.peer_addr().unwrap()))
                        });

                        // Re-populate pending connections with connections that should be part of the new
                        // ConnectionSet but isn't there yet.
                        pending_connections.clear();
                        for (public_addr, peer_addr) in new_addrs {
                            if connections.get(&public_addr).is_none() {
                                pending_connections.insert(public_addr, peer_addr);
                            }
                        }
                    },
                    EstablisherRequest::Reconnect(addrs) => {
                        for (public_addr, peer_addr) in addrs {
                            // TODO: describe scenarios in which this would be None instead. 
                            if connections.get(&public_addr).is_some() {
                                pending_connections.insert(public_addr, peer_addr);
                            }
                        }
                    },
                }
            }
        })
    } 

    // Spawns the sender threads. These are responsible for handling Send-Tos and Broadcasts in the background, allowing 
    // the corresponding methods in ipc::Handle to return quickly.
    fn sender(connections: Arc<RwLock<ConnectionSet>>, requests: mpsc::Receiver<SendRequest>, to_establisher: mpsc::Sender<EstablisherRequest>) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let workers = ThreadPool::new(Self::N_SENDERS);
    
            for request in requests {
                // When scoped threads become stable, this Arc can be removed.
                let errored_connections = Arc::new(Mutex::new(Vec::new()));

                match request {
                        SendRequest::SendTo(public_addr, msg) => {
                            // TODO: describe scenarios in which this would be None instead.
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
                        // TODO: distinguish between dropped and slow connections.

                        // Try to re-establish errored connections.
                        to_establisher.send(EstablisherRequest::Reconnect(errored_connections.to_vec())).unwrap();
                    }
                }
        })
    }
}

pub enum EstablisherRequest {
    ReplaceConnectionSet(Vec<(PublicAddress, SocketAddr)>),
    Reconnect(Vec<(PublicAddress, SocketAddr)>),
}

pub enum SendRequest {
    SendTo(PublicAddress, ConsensusMsg),
    Broadcast(ConsensusMsg),
}
