use std::net::{TcpStream, SocketAddr};
use std::sync::{Arc, RwLock, mpsc};
use std::time::{Duration, Instant};
use std::thread;
use std::io;
use threadpool::ThreadPool;
use crate::msg_types::{ConsensusMsg, PublicAddress};
use crate::ipc::{ConnectionSet, RwTcpStream};
use crate::ipc::manager::{Manager, EstablisherRequest, SendRequest};


#[derive(Clone)]
pub(crate) struct Handle {
    connections: Arc<RwLock<ConnectionSet>>,
    to_establisher: mpsc::Sender<EstablisherRequest>,
    to_sender: mpsc::Sender<SendRequest>,
    receivers: ThreadPool, 
}

impl Handle {
    const N_RECEIVERS: usize = 4;

    pub fn new(connections: Arc<RwLock<ConnectionSet>>, to_establisher: mpsc::Sender<EstablisherRequest>, to_sender: mpsc::Sender<SendRequest>) -> Handle {
        Handle {
            connections,
            to_establisher,
            to_sender,
            receivers: ThreadPool::new(Self::N_RECEIVERS),
        }
    }

    pub fn update_connections(&self, new_addrs: Vec<(PublicAddress, SocketAddr)>) {
        self.to_establisher.send(EstablisherRequest::ReplaceConnectionSet(new_addrs)).unwrap()
    }

    pub fn send_to(&self, participant: PublicAddress, msg: ConsensusMsg) {
        self.to_sender.send(SendRequest::SendTo(participant, msg)).unwrap();
    }

    pub fn broadcast(&self, msg: ConsensusMsg) {
        self.to_sender.send(SendRequest::Broadcast(msg)).unwrap();
    }

    /// Receive a ConsensusMsg from an identified participant, waiting for at most the provided timeout.
    ///  
    /// This call can fail in a variety of different ways. These are all handled transparently by the function,
    /// but for completeness, it may fail because:
    /// 1. A connection to the participant has not been established, or
    /// 2. An IO timeout, or
    /// 3. The connection has likely been dropped.
    pub fn recv_from(&self, participant: &PublicAddress, timeout: Duration) -> Result<ConsensusMsg, RecvError> {
        let start = Instant::now();

        while start.elapsed() < timeout {
            match self.connections.read().unwrap().get(participant) {
                None => thread::yield_now(),
                Some(stream) => {
                    match ConsensusMsg::deserialize_from_stream(stream, &timeout.saturating_sub(start.elapsed())) {
                        Ok(msg) => return Ok(msg),
                        Err(_) => {
                            self.to_establisher.send(EstablisherRequest::Reconnect((participant.clone(), stream.peer_addr().unwrap()))).unwrap();
                            return Err(RecvError)
                        }
                    }
                },
            }
        }

        Err(RecvError)
    }

    /// Like `recv_from`, but tries to get a ConsensusMsg from one (any) of the connections in the ConnectionSet. 
    pub fn recv_from_any(&self, timeout: Duration) -> Result<ConsensusMsg, RecvError> {
        // Rough flow:
        // 1. In a threadpool, create as many tasks as there are open connections.
        // 2. Worker threads try to peek from each connection. If it manages to get a ConsensusMsg, it sends it
        //    back to the main thread.
        // Alt. If an undeserializable message appears or the connection fails in any way, send a Reconnect request
        //      to the Establisher.
        // 3. The moment the main thread receives a ConsensusMsg, remove that message from the socket on which in was
        //    received, and then return the ConsensusMsg.
        assert!(timeout > Manager::READ_TIMEOUT);
        let start = Instant::now();        

        let (to_main, from_workers) = mpsc::channel();
        let connections = self.connections.read().unwrap();

        // 1. Create as many Receiver tasks as there are open connections.
        for (public_addr, stream) in &*connections {
            let public_addr = public_addr.clone();
            let to_main = to_main.clone();
            let to_establisher = self.to_establisher.clone();
            let stream = stream.clone();
            self.receivers.execute(move || {
                // 2. As a Receiver, try to peek from connection.
                let time_left = timeout.saturating_sub(start.elapsed());
                match ConsensusMsg::deserialize_from_stream_peek(&stream, &time_left) {
                    Ok(msg) => to_main.send(msg).unwrap(),
                    Err(_) => {
                        to_establisher.send(EstablisherRequest::Reconnect((public_addr, stream.peer_addr().unwrap()))).unwrap();
                        return
                    }
                };
            })
        }

        // 3. As main, wait for a worker to send a ConsensusMsg.
        let time_left = timeout.saturating_sub(start.elapsed());
        let msg = from_workers.recv_timeout(time_left).map_err(|_| RecvError)?;

        // 4. Remove the peeked and deserialized bytes from the originating TcpStream.
        todo!();
    }
}

pub struct RecvError;

impl ConsensusMsg {
    fn deserialize_from_stream(stream: &RwTcpStream, timeout: &Duration) -> Result<ConsensusMsg, DeserializeFromStreamError> {
        todo!()
    }

    fn deserialize_from_stream_peek(stream: &RwTcpStream, timeout: &Duration) -> Result<ConsensusMsg, DeserializeFromStreamError> {
        todo!()
    }
}

enum DeserializeFromStreamError {
    DeserializeError,
    IoError(io::Error),
}
