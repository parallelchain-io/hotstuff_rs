use std::net::SocketAddr;
use std::sync::{Arc, RwLock, mpsc};
use std::time::Duration;
use crate::msg_types::{ConsensusMsg, PublicAddress};
use crate::ipc::ConnectionSet;
use crate::ipc::manager::{EstablisherRequest, SendRequest};

#[derive(Clone)]
pub struct Handle {
    pub(in crate::ipc) connections: Arc<RwLock<ConnectionSet>>,
    pub(in crate::ipc) to_establisher: mpsc::Sender<EstablisherRequest>,
    pub(in crate::ipc) to_sender: mpsc::Sender<SendRequest>,
}

impl Handle {
    pub fn update_connections(&self, new_addrs: Vec<(PublicAddress, SocketAddr)>) {
        self.to_establisher.send(EstablisherRequest::ReplaceConnectionSet(new_addrs)).unwrap()
    }

    pub fn send_to(&self, participant: PublicAddress, msg: ConsensusMsg) {
        self.to_sender.send(SendRequest::SendTo(participant, msg)).unwrap();
    }

    pub fn broadcast(&self, msg: ConsensusMsg) {
        self.to_sender.send(SendRequest::Broadcast(msg)).unwrap();
    }

    /// timeout = max(timeout, READ_TIMEOUT).
    pub fn recv_from(&self, participant: PublicAddress, timeout: Option<Duration>) -> ConsensusMsg {
        todo!()
    }

    fn recv_from_any(timeout: Option<Duration>) -> ConsensusMsg {
        // Rough flow:
        // 1. In a threadpool, spawn as many worker threads as there are open connections.
        // 2. Worker threads try to peek from each connection. If it manages to get a ConsensusMsg, it sends it
        //    back to the main thread.
        // Alt. If an undeserializable message appears or the connection fails in any way, send a Reconnect request
        //      to the Establisher.
        // 3. The moment the main thread receives a ConsensusMsg, remove that message from the socket on which in was
        //    received, and then return the ConsensusMsg.
        todo!()
    }
}

