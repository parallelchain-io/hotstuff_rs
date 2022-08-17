use std::net::SocketAddr;
use std::sync::{Arc, RwLock, mpsc};
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

    pub fn recv_from(&self, participant: PublicAddress) -> ConsensusMsg {
        todo!()
    }

    fn recv_from_any() -> ConsensusMsg {
        todo!()
    }
}

