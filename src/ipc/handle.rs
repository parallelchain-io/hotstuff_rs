use std::net::{TcpStream, SocketAddr};
use std::sync::{Arc, RwLock, mpsc};
use std::time::{Duration, Instant};
use std::cmp;
use std::thread;
use threadpool::ThreadPool;
use crate::msg_types::{ConsensusMsg, PublicAddress};
use crate::ipc::ConnectionSet;
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

    /// Receive a ConsensusMsg from an identified participant. This waits for at most the provided
    /// `timeout_upper_bound`, or the configured `READ_TIMEOUT`, whichever is greater. If `timeout_upper_bound`
    /// is not set, it waits for `READ_TIMEOUT`.
    /// 
    /// This call can fail in a variety of different ways. These are all handled transparently by the function,
    /// but for completeness, it may fail because:
    /// 1. A connection to the participant has not been established, or
    /// 2. An IO timeout, or
    /// 3. The connection has likely been dropped.
    pub fn recv_from(&self, participant: &PublicAddress, timeout_upper_bound: Option<Duration>) -> Result<ConsensusMsg, RecvError> {
        let timeout = match timeout_upper_bound {
            Some(timeout) => cmp::max(Manager::READ_TIMEOUT, timeout),
            None => Manager::READ_TIMEOUT,
        };

        let beginning = Instant::now();
        while beginning.elapsed() < timeout {
            match self.connections.read().unwrap().get(participant) {
                None => thread::yield_now(),
                Some(stream) => return ConsensusMsg::deserialize_from_stream(stream.read(), &timeout),
            }
        }

        Err(RecvError)
    }

    // Like `recv_from`, but tries to get a ConsensusMsg from one (any) of the connections in the ConnectionSet. 
    pub fn recv_from_any(&self, timeout_upper_bound: Option<Duration>) -> Result<ConsensusMsg, RecvError> {
        // Rough flow:
        // 1. In a threadpool, create as many tasks as there are open connections.
        // 2. Worker threads try to peek from each connection. If it manages to get a ConsensusMsg, it sends it
        //    back to the main thread.
        // Alt. If an undeserializable message appears or the connection fails in any way, send a Reconnect request
        //      to the Establisher.
        // 3. The moment the main thread receives a ConsensusMsg, remove that message from the socket on which in was
        //    received, and then return the ConsensusMsg.
        todo!()
        // let (to_main, from_worker) = mpsc::channel();
        // let connections = self.connections.read().unwrap();
        // for (_, stream) in connections {

        //     let stream = stream.try_clone().unwrap();
        //     self.receivers.execute(move || {
                
        //     })
        // } 
    }
}

pub struct RecvError;

impl ConsensusMsg {
    fn deserialize_from_stream(stream: &TcpStream, timeout: &Duration) -> Result<ConsensusMsg, RecvError> {
        todo!()
    }

    fn deserialize_from_stream_peek(stream: &TcpStream, timeout: &Duration) -> Result<ConsensusMsg, RecvError> {
        todo!()
    }
}
